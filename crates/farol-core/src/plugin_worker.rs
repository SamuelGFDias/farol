//! Worker assíncrono do plugin — spawn do processo filho, handshake, ciclo
//! de `widget/get` e `action/invoke`, e detecção de crash (T020, T021, T026,
//! T033, T038).
//!
//! Modelado como uma `iced::Subscription` de longa duração (padrão D5 de
//! `research.md`): a função [`worker`] é passada para `Subscription::run`,
//! que a executa como um `Stream` dentro do próprio executor tokio que o
//! `iced` já embarca (feature `tokio` do crate `iced`, D4) — nunca um
//! segundo runtime tokio criado manualmente.
//!
//! Fluxo:
//! 1. `spawn()` do processo filho do plugin. Falha ⟹ [`WorkerEvent::SpawnFailed`],
//!    stream encerra (equivalente a `Unavailable{FailedToStart}` depois de
//!    interpretado por `update.rs`) — T043, já coberto desde T020.
//! 2. Sucesso ⟹ o worker registra um canal de entrada (`mpsc`) e emite
//!    [`WorkerEvent::Ready`] com o `Sender` — é assim que `update.rs` passa a
//!    poder mandar pedidos para este worker (D5, passo 2).
//! 3. Envia `handshake/hello` e aguarda a resposta com timeout de
//!    [`RPC_TIMEOUT_CONTROL`] (T021). Resultado ⟹ [`WorkerEvent::HandshakeCompleted`].
//!    Se o handshake não resultar em `Ready`, o worker encerra o stream —
//!    `Unavailable` é terminal nesta feature (data-model.md §3), não há
//!    motivo para o worker continuar vivo.
//! 4. Se `Ready`: loop recebendo [`WorkerInput`] pelo canal de entrada —
//!    `RequestWidget` (T026, disparado por um tick de refresh) responde com
//!    [`WorkerEvent::WidgetGetCompleted`] e um timeout aqui encerra o worker
//!    (D6 — timeout no ciclo de refresh marca a conexão inteira como
//!    indisponível); `InvokeAction` (T033, disparado por um clique de
//!    "Fetch") responde com [`WorkerEvent::ActionInvokeCompleted`] usando o
//!    orçamento próprio [`RPC_TIMEOUT_ACTION`] (ou `timeout_hint_ms` da
//!    ação) — ao contrário de `widget/get`, um erro ou timeout aqui NUNCA
//!    encerra o worker nem marca `Unresponsive` (D6, `contracts/action-protocol.md`),
//!    só é reportado como falha pontual daquela ação.
//!    Concorrentemente a cada uma dessas operações (incluindo o período
//!    ocioso entre pedidos), o worker também observa `child.wait()` via
//!    `tokio::select!` (T038, D6) — se o processo morrer a qualquer momento,
//!    mesmo sem nenhuma requisição em voo, o worker emite
//!    [`WorkerEvent::Crashed`] imediatamente e encerra.
//!
//! Nota sobre correlação de `id` JSON-RPC: este worker fala com o plugin de
//! forma estritamente sequencial — nunca há mais de uma requisição em voo
//! por vez (handshake, depois um `widget/get`/`action/invoke` de cada vez).
//! Por isso a implementação não faz correlação de `id` requisição↔resposta
//! explícita (assume que a próxima linha lida de `stdout` é a resposta da
//! última requisição escrita); seria necessário se o protocolo permitisse
//! pipelining, o que esta feature não exercita.

use std::io;
use std::process::Stdio;
use std::time::Duration;

use farol_protocol::{
    decode, encode, ActionInvokeParams, ActionInvokeRequest, ActionInvokeResponse,
    ActionInvokeResult, ActionTarget, HandshakeHello, HandshakeHelloRequest,
    HandshakeHelloResponse, HandshakeHelloResult, ProtocolVersion, RequestId, WidgetGetParams,
    WidgetGetRequest, WidgetGetResponse, WidgetGetResult,
};
use iced::futures::channel::mpsc;
use iced::futures::sink::SinkExt;
use iced::futures::{Stream, StreamExt};
use iced::stream;
use iced::Subscription;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

/// Comando usado para iniciar o processo do plugin. Não há plugin Python
/// real ainda nesta subtarefa (T023-T025 são de outra pessoa/onda) — até que
/// `plugins/git-local/main.py` exista, `spawn()` deste comando MAY falhar
/// (binário `python3` ausente do `PATH`) ou, se `python3` existir mas o
/// script não, o processo filho MAY morrer imediatamente ao tentar abrir o
/// arquivo. Qualquer um desses casos é o comportamento correto e esperado
/// desta subtarefa: `Unavailable{FailedToStart}` (spawn falhou) ou
/// `Unavailable{Unresponsive}` (processo subiu mas nunca respondeu ao
/// handshake dentro do timeout) — nunca um pânico do core.
pub const PLUGIN_COMMAND: &str = "python3";

/// Argumentos do comando acima. Caminho relativo à raiz do repositório —
/// só resolve corretamente se `farol-core` for executado com o `cwd` na
/// raiz do repo (ex.: via `cargo run` a partir da raiz). Ponto de
/// fragilidade conhecido, aceitável nesta fase de walking skeleton.
pub const PLUGIN_ARGS: &[&str] = &["plugins/git-local/main.py"];

/// Orçamento de timeout para chamadas de controle (`handshake/hello` e
/// `widget/get`) — `protocol/SPEC.md` §7.1 / `RPC_TIMEOUT_CONTROL`. Não se
/// aplica a `action/invoke` (`RPC_TIMEOUT_ACTION`, US2, fora do escopo desta
/// subtarefa).
pub const RPC_TIMEOUT_CONTROL: Duration = Duration::from_secs(5);

/// Orçamento de timeout default para `action/invoke` (`protocol/SPEC.md`
/// §7.2 / `RPC_TIMEOUT_ACTION`) — desacoplado de [`RPC_TIMEOUT_CONTROL`]
/// (D6, `contracts/action-protocol.md`): uma ação como `git.fetch` pode ir à
/// rede e legitimamente demorar muito mais que uma chamada de controle
/// local. Usado apenas quando a `ActionDeclaration` correspondente não
/// sugere um `timeout_hint_ms` próprio (T033).
pub const RPC_TIMEOUT_ACTION: Duration = Duration::from_secs(120);

/// Versão de protocolo que este core fala (`protocol/SPEC.md` §6.4). Usada
/// tanto para preencher `HandshakeHello.protocol_version` quanto como o lado
/// "core" da checagem de compatibilidade (D7,
/// `ProtocolVersion::is_compatible_with`).
const CORE_PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion { major: 0, minor: 1 };

/// Identificação informativa deste core no handshake (`HandshakeHello.core_name`).
const CORE_NAME: &str = "farol-core";

/// Pedidos que `update.rs` pode enviar ao worker pelo canal recebido em
/// [`WorkerEvent::Ready`].
#[derive(Debug, Clone)]
pub enum WorkerInput {
    /// Pede uma atualização do widget `widget_id` (T026 — disparado por um
    /// tick de refresh periódico depois que a conexão está `Ready`).
    RequestWidget { widget_id: String },
    /// Invoca uma ação sobre um alvo (T033 — disparado por um clique de
    /// "Fetch" na UI depois que a conexão está `Ready`).
    InvokeAction {
        /// MUST corresponder ao `id` de uma `ActionDeclaration` previamente
        /// declarada pelo plugin com `enabled: true` (`update.rs` só envia
        /// isto quando a UI mostrou a ação habilitada — T032).
        action_id: String,
        /// Ecoado literalmente do `target` da `ActionDeclaration` que
        /// motivou esta invocação — nunca reconstruído aqui.
        target: ActionTarget,
        /// `timeout_hint_ms` da `ActionDeclaration`, quando o plugin
        /// sugeriu um. Ausente ⟹ o worker aplica [`RPC_TIMEOUT_ACTION`].
        timeout_hint_ms: Option<u64>,
    },
}

/// Resultado da tentativa de handshake — o que [`WorkerEvent::HandshakeCompleted`]
/// carrega. Mapeado para `PluginState` em `update.rs` (T022).
#[derive(Debug, Clone)]
pub enum HandshakeOutcome {
    /// Handshake respondido dentro do timeout, com versão compatível
    /// (`ProtocolVersion::is_compatible_with`, D7).
    Ready(HandshakeHelloResult),
    /// Handshake respondido dentro do timeout, mas com versão incompatível
    /// (FR-005).
    VersionIncompatible {
        plugin_version: ProtocolVersion,
        core_version: ProtocolVersion,
    },
    /// Sem resposta dentro de `RPC_TIMEOUT_CONTROL`, ou o plugin recusou o
    /// handshake com um erro JSON-RPC, ou a linha de resposta não pôde ser
    /// decodificada, ou o processo fechou `stdout` (EOF) antes de
    /// responder. Todos esses casos convergem para `Unresponsive`: do ponto
    /// de vista do core, um plugin que não entrega um `HandshakeHelloResult`
    /// válido a tempo é, na prática, indistinguível de um plugin travado
    /// (mesmo raciocínio de D6 para o ciclo de refresh).
    Unresponsive,
}

/// Resultado de uma chamada `widget/get` — o que [`WorkerEvent::WidgetGetCompleted`]
/// carrega.
#[derive(Debug, Clone)]
pub enum WidgetOutcome {
    /// Resposta de sucesso.
    Success(WidgetGetResult),
    /// Erro pontual do plugin (ex.: `scan_root` inacessível) — por contrato
    /// (`widget-protocol.md`) isso NÃO torna a conexão indisponível; só
    /// sinaliza o erro para esta chamada específica.
    PluginError(String),
    /// Timeout, decode inválido ou EOF — contribui para `Unresponsive`
    /// (D6): o worker encerra a conexão após emitir este evento.
    Unresponsive,
}

/// Resultado de uma chamada `action/invoke` — o que
/// [`WorkerEvent::ActionInvokeCompleted`] carrega. Ao contrário de
/// [`WidgetOutcome`], nenhuma variante aqui encerra o worker nem marca
/// `PluginState = Unavailable` (D6, `contracts/action-protocol.md`/`error-model.md`
/// §"Regra geral") — toda falha é pontual da chamada, fundida por
/// `update.rs` (T035) no `RepositoryViewModel` do repositório-alvo.
#[derive(Debug, Clone)]
pub enum ActionOutcome {
    /// Resposta de sucesso — `result.repo` é o estado pós-ação (ex.:
    /// pós-fetch), já com `ahead`/`behind` atualizados.
    Success(ActionInvokeResult),
    /// Erro estruturado devolvido pelo plugin (ex.: `-32001 fetch_failed`)
    /// ou erro de I/O local ao falar com o processo — `target` identifica o
    /// repositório afetado para `update.rs` conseguir associar o erro sem
    /// depender de o plugin ecoar `target` de volta em `error.data`.
    PluginError { target: ActionTarget, message: String },
    /// A resposta não chegou dentro do orçamento (`RPC_TIMEOUT_ACTION` ou
    /// `timeout_hint_ms`) — reportado à UI como erro pontual daquela ação
    /// (equivalente a `-32002 action_timeout`, sintetizado pelo core, não
    /// pelo plugin). NÃO contribui para `Unresponsive`/`Unavailable` (D6).
    Timeout { target: ActionTarget },
}

/// Eventos emitidos pelo worker no stream da `Subscription` — cada um vira
/// uma `Message` do `iced` (mapeado em `main.rs`/`update.rs`).
#[derive(Debug, Clone)]
pub enum WorkerEvent {
    /// O processo subiu e o canal de entrada está pronto para receber
    /// pedidos. Emitido antes do handshake completar (D5, passo 2) — quem
    /// consome isso ainda não deve mandar pedidos além do que o próprio
    /// worker dispara internamente (o handshake), já que a conexão só fica
    /// `Ready` depois do handshake bem-sucedido.
    Ready(mpsc::Sender<WorkerInput>),
    /// `spawn()` do processo filho falhou (T020 — binário ausente/não
    /// executável, ou qualquer outro erro de SO ao iniciar o processo).
    SpawnFailed(String),
    /// Resultado do handshake (T021/T022).
    HandshakeCompleted(HandshakeOutcome),
    /// Resultado de um ciclo de `widget/get` (T026).
    WidgetGetCompleted(WidgetOutcome),
    /// Resultado de uma invocação de `action/invoke` (T033).
    ActionInvokeCompleted(ActionOutcome),
    /// O processo filho terminou inesperadamente enquanto a conexão estava
    /// `Ready` (T038, FR-019, D6) — detectado via `child.wait()` concorrente
    /// à leitura de stdout/processamento de pedidos, nunca dependente de
    /// haver uma requisição em voo no momento. `String` é um detalhe
    /// legível (status de saída, quando disponível) para exibição/log; a
    /// transição para `PluginState::Unavailable { reason: Crashed, .. }` é
    /// aplicada por `update.rs`.
    Crashed(String),
}

/// `Subscription` do worker de um plugin — deve ser incluída no retorno de
/// `Farol::subscription` (main.rs/update.rs) para que o `iced` efetivamente
/// rode o worker (D5: uma `Subscription` só produz efeitos enquanto for
/// devolvida ao runtime a cada ciclo).
pub fn subscription() -> Subscription<WorkerEvent> {
    Subscription::run(worker)
}

/// Corpo do worker — ver a documentação do módulo para o fluxo completo.
///
/// Assinatura exigida por `Subscription::run` (`fn() -> S` — ponteiro de
/// função sem captura, D5/D4): por isso os parâmetros de configuração
/// (`PLUGIN_COMMAND`, `RPC_TIMEOUT_CONTROL`, etc.) são constantes do módulo
/// em vez de argumentos.
fn worker() -> impl Stream<Item = WorkerEvent> {
    stream::channel(16, |mut output| async move {
        let mut command = Command::new(PLUGIN_COMMAND);
        command
            .args(PLUGIN_ARGS)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                let _ = output
                    .send(WorkerEvent::SpawnFailed(format!(
                        "falha ao iniciar '{PLUGIN_COMMAND} {}': {err}",
                        PLUGIN_ARGS.join(" ")
                    )))
                    .await;
                return;
            }
        };

        let Some(mut stdin) = child.stdin.take() else {
            let _ = output
                .send(WorkerEvent::SpawnFailed(
                    "processo do plugin não expôs stdin".to_string(),
                ))
                .await;
            return;
        };
        let Some(stdout) = child.stdout.take() else {
            let _ = output
                .send(WorkerEvent::SpawnFailed(
                    "processo do plugin não expôs stdout".to_string(),
                ))
                .await;
            return;
        };
        let mut reader = BufReader::new(stdout);

        // D5, passo 2: registra o canal de entrada assim que o processo está
        // de pé — antes mesmo do handshake terminar. `update.rs` só efetivamente
        // usa este `Sender` depois que a conexão estiver `Ready` (T026), mas
        // guardá-lo cedo simplifica o fluxo do worker (um único ponto de saída
        // do `Sender`, sem estado intermediário extra).
        let (input_sender, mut input_receiver) = mpsc::channel::<WorkerInput>(16);
        if output.send(WorkerEvent::Ready(input_sender)).await.is_err() {
            // A `Subscription` foi descartada pelo runtime (app encerrando) —
            // nada a fazer além de deixar `child` ser dropado (kill_on_drop).
            return;
        }

        // --- Handshake (T021) ---

        let mut next_request_id: i64 = 2; // id=1 é o handshake em si.
        let hello_request = HandshakeHelloRequest::new(
            RequestId::Integer(1),
            HandshakeHello {
                protocol_version: CORE_PROTOCOL_VERSION,
                core_name: CORE_NAME.to_string(),
            },
        );

        let handshake_outcome = if write_line(&mut stdin, &hello_request).await.is_err() {
            HandshakeOutcome::Unresponsive
        } else {
            match tokio::time::timeout(
                RPC_TIMEOUT_CONTROL,
                read_response::<HandshakeHelloResponse>(&mut reader),
            )
            .await
            {
                Err(_elapsed) => HandshakeOutcome::Unresponsive,
                Ok(Err(_io_or_decode_error)) => HandshakeOutcome::Unresponsive,
                Ok(Ok(None)) => HandshakeOutcome::Unresponsive, // EOF antes de responder.
                Ok(Ok(Some(HandshakeHelloResponse::Error { .. }))) => {
                    HandshakeOutcome::Unresponsive
                }
                Ok(Ok(Some(HandshakeHelloResponse::Success { result, .. }))) => {
                    if result
                        .protocol_version
                        .is_compatible_with(&CORE_PROTOCOL_VERSION)
                    {
                        HandshakeOutcome::Ready(result)
                    } else {
                        HandshakeOutcome::VersionIncompatible {
                            plugin_version: result.protocol_version,
                            core_version: CORE_PROTOCOL_VERSION,
                        }
                    }
                }
            }
        };

        let is_ready = matches!(handshake_outcome, HandshakeOutcome::Ready(_));
        if output
            .send(WorkerEvent::HandshakeCompleted(handshake_outcome))
            .await
            .is_err()
        {
            return;
        }
        if !is_ready {
            // `Unavailable` é terminal nesta feature (data-model.md §3) — o
            // worker encerra; não há retry automático.
            return;
        }

        // --- Ciclo de widget/get e action/invoke (T026, T033), com
        // detecção concorrente de crash (T038, D6) ---
        //
        // Cada `tokio::select!` abaixo corre `wait_for_crash(&mut child)`
        // ao lado da operação normal — tanto no período ocioso (esperando o
        // próximo `WorkerInput`) quanto durante o próprio ciclo de I/O de
        // uma requisição já em voo. Se o processo morrer em qualquer um
        // desses momentos, o branch de crash vence a corrida e o worker
        // emite `WorkerEvent::Crashed` imediatamente, sem esperar nenhum
        // timeout de RPC.

        loop {
            let input = tokio::select! {
                detail = wait_for_crash(&mut child) => {
                    let _ = output.send(WorkerEvent::Crashed(detail)).await;
                    return;
                }
                maybe_input = input_receiver.next() => maybe_input,
            };

            let Some(input) = input else {
                // Canal de entrada fechado — o lado core (`Farol`) foi
                // descartado (app encerrando). Nada a fazer além de deixar
                // `child` ser dropado (`kill_on_drop`).
                return;
            };

            match input {
                WorkerInput::RequestWidget { widget_id } => {
                    let request_id = RequestId::Integer(next_request_id);
                    next_request_id += 1;
                    let request = WidgetGetRequest::new(request_id, WidgetGetParams { widget_id });

                    let widget_outcome = tokio::select! {
                        detail = wait_for_crash(&mut child) => {
                            let _ = output.send(WorkerEvent::Crashed(detail)).await;
                            return;
                        }
                        outcome = perform_widget_get(&mut stdin, &mut reader, request) => outcome,
                    };

                    let is_unresponsive = matches!(widget_outcome, WidgetOutcome::Unresponsive);
                    if output
                        .send(WorkerEvent::WidgetGetCompleted(widget_outcome))
                        .await
                        .is_err()
                    {
                        return;
                    }
                    if is_unresponsive {
                        // D6: timeout num ciclo de refresh marca a conexão
                        // inteira como indisponível — o worker encerra, sem
                        // tentar de novo.
                        return;
                    }
                }
                WorkerInput::InvokeAction {
                    action_id,
                    target,
                    timeout_hint_ms,
                } => {
                    let request_id = RequestId::Integer(next_request_id);
                    next_request_id += 1;
                    let timeout = timeout_hint_ms
                        .map(Duration::from_millis)
                        .unwrap_or(RPC_TIMEOUT_ACTION);
                    let request = ActionInvokeRequest::new(
                        request_id,
                        ActionInvokeParams {
                            action_id,
                            target: target.clone(),
                        },
                    );

                    let action_outcome = tokio::select! {
                        detail = wait_for_crash(&mut child) => {
                            let _ = output.send(WorkerEvent::Crashed(detail)).await;
                            return;
                        }
                        outcome = perform_action_invoke(&mut stdin, &mut reader, request, target, timeout) => outcome,
                    };

                    // Ao contrário de `widget/get`, uma falha aqui NUNCA
                    // encerra o worker nem contribui para `Unresponsive`
                    // (D6, `contracts/action-protocol.md`) — só é reportada
                    // como erro pontual daquela ação; o loop continua.
                    if output
                        .send(WorkerEvent::ActionInvokeCompleted(action_outcome))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
            }
        }
    })
}

/// Aguarda a saída do processo filho e formata um detalhe legível para
/// `WorkerEvent::Crashed` (T038, FR-019). Chamada sempre de dentro de
/// `tokio::select!` junto com outra operação do worker — nunca isolada em
/// sequência, senão bloquearia o worker até o processo morrer em vez de
/// continuar processando pedidos normalmente. Segura para ser chamada
/// repetidamente (uma vez por iteração do loop/seleção): `Child::wait`
/// devolve o mesmo resultado depois de já ter sido chamada, e descartar a
/// future antes dela resolver (quando o outro branch do `select!` vence)
/// não mata nem afeta o processo — só `Child` sendo dropado (`kill_on_drop`)
/// faz isso.
async fn wait_for_crash(child: &mut Child) -> String {
    match child.wait().await {
        Ok(status) => format!("processo do plugin encerrou inesperadamente: {status}"),
        Err(err) => {
            format!("processo do plugin encerrou inesperadamente (status indisponível: {err})")
        }
    }
}

/// Executa um ciclo completo de `widget/get`: escreve o request, aguarda a
/// resposta dentro do orçamento `RPC_TIMEOUT_CONTROL` e traduz o resultado
/// em `WidgetOutcome`. Extraído do corpo do loop principal (T038) para poder
/// ser corrido dentro de um `tokio::select!` junto com `wait_for_crash`.
async fn perform_widget_get(
    stdin: &mut ChildStdin,
    reader: &mut BufReader<ChildStdout>,
    request: WidgetGetRequest,
) -> WidgetOutcome {
    if write_line(stdin, &request).await.is_err() {
        return WidgetOutcome::Unresponsive;
    }
    match tokio::time::timeout(RPC_TIMEOUT_CONTROL, read_response::<WidgetGetResponse>(reader))
        .await
    {
        Err(_elapsed) => WidgetOutcome::Unresponsive,
        Ok(Err(_io_or_decode_error)) => WidgetOutcome::Unresponsive,
        Ok(Ok(None)) => WidgetOutcome::Unresponsive,
        Ok(Ok(Some(WidgetGetResponse::Error { error, .. }))) => {
            WidgetOutcome::PluginError(error.message)
        }
        Ok(Ok(Some(WidgetGetResponse::Success { result, .. }))) => WidgetOutcome::Success(result),
    }
}

/// Executa um ciclo completo de `action/invoke` (T033): escreve o request,
/// aguarda a resposta dentro de `timeout` (`RPC_TIMEOUT_ACTION` ou
/// `timeout_hint_ms` da ação, já resolvido pelo chamador) e traduz o
/// resultado em `ActionOutcome`. `target` é mantido pelo chamador (não
/// consumido pelo request) para poder identificar o repositório-alvo em
/// qualquer variante de falha, já que o plugin não é obrigado a ecoá-lo de
/// volta em `error.data`.
async fn perform_action_invoke(
    stdin: &mut ChildStdin,
    reader: &mut BufReader<ChildStdout>,
    request: ActionInvokeRequest,
    target: ActionTarget,
    timeout: Duration,
) -> ActionOutcome {
    if write_line(stdin, &request).await.is_err() {
        return ActionOutcome::PluginError {
            target,
            message: "falha ao escrever o pedido no stdin do plugin".to_string(),
        };
    }
    match tokio::time::timeout(timeout, read_response::<ActionInvokeResponse>(reader)).await {
        Err(_elapsed) => ActionOutcome::Timeout { target },
        Ok(Err(_io_or_decode_error)) => ActionOutcome::PluginError {
            target,
            message: "resposta do plugin não pôde ser decodificada".to_string(),
        },
        Ok(Ok(None)) => ActionOutcome::PluginError {
            target,
            message: "processo do plugin fechou stdout antes de responder".to_string(),
        },
        Ok(Ok(Some(ActionInvokeResponse::Error { error, .. }))) => ActionOutcome::PluginError {
            target,
            message: error.message,
        },
        Ok(Ok(Some(ActionInvokeResponse::Success { result, .. }))) => {
            ActionOutcome::Success(result)
        }
    }
}

/// Serializa `message` como uma linha NDJSON (`farol_protocol::encode`) e a
/// escreve em `stdin`, garantindo o flush (sem isso o plugin pode nunca ver
/// os bytes, já que pipes são bufferizados).
async fn write_line<T: serde::Serialize>(
    stdin: &mut ChildStdin,
    message: &T,
) -> io::Result<()> {
    let line = encode(message).map_err(io::Error::other)?;
    stdin.write_all(line.as_bytes()).await?;
    stdin.flush().await
}

/// Lê linhas de `reader` até obter uma mensagem decodificável (ignorando
/// linhas em branco — `protocol/SPEC.md` §4 exige isso do lado que lê) ou
/// até o processo fechar `stdout` (`Ok(None)`, EOF).
async fn read_response<T: serde::de::DeserializeOwned>(
    reader: &mut BufReader<ChildStdout>,
) -> io::Result<Option<T>> {
    let mut line = String::new();
    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line).await?;
        if bytes_read == 0 {
            return Ok(None);
        }
        match decode::<T>(&line) {
            Ok(Some(message)) => return Ok(Some(message)),
            Ok(None) => continue, // linha em branco — ignorada silenciosamente.
            Err(err) => return Err(io::Error::other(err)),
        }
    }
}

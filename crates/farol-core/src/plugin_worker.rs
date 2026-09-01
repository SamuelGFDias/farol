//! Worker assíncrono do plugin — spawn do processo filho, handshake, ciclo
//! de `widget/get` e `action/invoke`, e detecção de crash (T020, T021, T026,
//! T033, T038; T011-T019 da feature 002 — protocolo `"0.2"`, múltiplos
//! plugins, injeção de `required_config`).
//!
//! Modelado como uma `iced::Subscription` de longa duração (padrão D5 de
//! `research.md`): a função [`worker`] é passada para
//! `Subscription::run_with_id` (T015 — antes `Subscription::run`, um
//! ponteiro de função sem captura; passou a ser `run_with_id` porque agora
//! cada conexão precisa de uma configuração própria — comando, args, nome do
//! plugin — capturada por `worker`, não mais constantes globais), que a
//! executa como um `Stream` dentro do próprio executor tokio que o `iced` já
//! embarca (feature `tokio` do crate `iced`, D4) — nunca um segundo runtime
//! tokio criado manualmente.
//!
//! Fluxo:
//! 1. `spawn()` do processo filho do plugin, com cada valor já persistido em
//!    `config.toml`/`secrets.toml` para este plugin injetado como variável
//!    de ambiente (T018 — ver [`stored_plugin_config_values`] para a decisão
//!    de design completa). Falha ⟹ [`WorkerEvent::SpawnFailed`], stream
//!    encerra (equivalente a `Unavailable{FailedToStart}` depois de
//!    interpretado por `update.rs`) — T043, já coberto desde T020.
//! 2. Sucesso ⟹ o worker registra um canal de entrada (`mpsc`) e emite
//!    [`WorkerEvent::Ready`] com o `Sender` — é assim que `update.rs` passa a
//!    poder mandar pedidos para este worker (D5, passo 2).
//! 3. Envia `handshake/hello` e aguarda a resposta com timeout de
//!    [`RPC_TIMEOUT_CONTROL`] (T021). A resposta é primeiro decodificada
//!    como `serde_json::Value` (não mais diretamente como
//!    `HandshakeHelloResponse` tipado) e interpretada por
//!    [`interpret_handshake_response`] — correção C1/T011, ver a
//!    documentação daquela função para o porquê. Resultado ⟹
//!    [`WorkerEvent::HandshakeCompleted`]. Se o handshake não resultar em
//!    `Ready` (versão compatível — `Ready` aqui não significa
//!    `PluginState::Ready`, ver nota em [`HandshakeOutcome::Ready`]), o
//!    worker encerra o stream — `Unavailable` é terminal para
//!    `VersionIncompatible`/`Unresponsive`/`FailedToStart`/`Crashed` nesta
//!    feature (data-model.md §3), não há motivo para o worker continuar
//!    vivo. Quando o handshake É compatível mas `required_config` está
//!    incompleto (T019), o worker segue vivo normalmente (`update.rs` é
//!    quem decide não chamar `widget/get` nesse caso).
//! 4. Se a versão for compatível: loop recebendo [`WorkerInput`] pelo canal
//!    de entrada — `RequestWidget` (T026, disparado por um tick de refresh)
//!    responde com [`WorkerEvent::WidgetGetCompleted`] e um timeout aqui
//!    encerra o worker (D6 — timeout no ciclo de refresh marca a conexão
//!    inteira como indisponível); `InvokeAction` (T033, disparado por um
//!    clique de "Fetch") responde com [`WorkerEvent::ActionInvokeCompleted`]
//!    usando o orçamento próprio [`RPC_TIMEOUT_ACTION`] (ou
//!    `timeout_hint_ms` da ação) — ao contrário de `widget/get`, um erro ou
//!    timeout aqui NUNCA encerra o worker nem marca `Unresponsive` (D6,
//!    `contracts/action-protocol.md`), só é reportado como falha pontual
//!    daquela ação.
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
    ActionInvokeResult, ActionTarget, HandshakeHello, HandshakeHelloRequest, HandshakeHelloResponse,
    HandshakeHelloResult, ProtocolVersion, RequestId, WidgetGetParams, WidgetGetRequest,
    WidgetGetResponse, WidgetGetResult,
};
// `RequiredConfigItem` (novo em v0.2) ainda não está na lista de re-exports de
// `crates/farol-protocol/src/lib.rs` (`pub use messages::{...}`) — gap na entrega prévia dessa
// dependência, fora do escopo desta subtarefa tocar (`farol-protocol` é off-limits). Referenciado
// aqui via `farol_protocol::messages::RequiredConfigItem` (o módulo e o tipo são ambos `pub`).
use farol_protocol::messages::RequiredConfigItem;
use iced::futures::channel::mpsc;
use iced::futures::sink::SinkExt;
use iced::futures::{Stream, StreamExt};
use iced::stream;
use iced::Subscription;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

/// Configuração de spawn de um plugin conhecido (T014, correção C2 parte 1).
///
/// Substitui as constantes `PLUGIN_COMMAND`/`PLUGIN_ARGS` de antes desta
/// feature — comando e argumentos deixam de ser globais e passam a variar
/// por conexão, já que agora existe mais de um plugin conhecido
/// simultaneamente (T015, correção C2 parte 2). `plugin_name` acumula um
/// segundo papel além de identificar a conexão: é a chave usada para
/// localizar `config.toml`/`secrets.toml` (`crate::config_store`/
/// `crate::secrets_store`) e para derivar o prefixo de variável de ambiente
/// injetada no spawn (`FAROL_PLUGIN_<PLUGIN_NAME>_<NAME>`, T018, D8) —
/// MUST bater exatamente com o `plugin_name` que o próprio processo declara
/// de volta no `handshake/hello` (nenhuma correlação de protocolo garante
/// isso automaticamente; é uma convenção do registro fixo em
/// [`known_plugins`], responsabilidade de quem adicionar uma entrada nova
/// manter os dois lados consistentes).
#[derive(Debug, Clone)]
pub struct PluginSpawnConfig {
    pub plugin_name: String,
    pub command: String,
    pub args: Vec<String>,
}

/// Registro fixo dos plugins conhecidos por este core (T014/T015, correção
/// C2) — `git-local` (feature 001) e `uptime-kuma` (feature 002). Um
/// registry federado/descoberto em runtime é Fora de Escopo do `spec.md`
/// desta feature; esta lista é deliberadamente hardcoded.
///
/// Caminho de `args` relativo à raiz do repositório — só resolve
/// corretamente se `farol-core` for executado com o `cwd` na raiz do repo
/// (ex.: via `cargo run` a partir da raiz). Ponto de fragilidade conhecido,
/// herdado da feature 001, aceitável nesta fase.
pub fn known_plugins() -> Vec<PluginSpawnConfig> {
    vec![
        PluginSpawnConfig {
            plugin_name: "git-local".to_string(),
            command: "python3".to_string(),
            args: vec!["plugins/git-local/main.py".to_string()],
        },
        PluginSpawnConfig {
            plugin_name: "uptime-kuma".to_string(),
            command: "python3".to_string(),
            args: vec!["plugins/uptime-kuma/main.py".to_string()],
        },
    ]
}

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
/// "core" da checagem de compatibilidade (D7, `ProtocolVersion::is_compatible_with`).
///
/// **Correção H1 (T012)**: `"0.1"` → `"0.2"` — bump normativo de
/// `research.md` D1 desta feature (`specs/002-uptime-kuma-plugin/research.md`).
const CORE_PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion { major: 0, minor: 2 };

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
/// carrega. Mapeado para `PluginState` em `update.rs` (T022/T019).
#[derive(Debug, Clone)]
pub enum HandshakeOutcome {
    /// Handshake respondido dentro do timeout, com versão compatível
    /// (`ProtocolVersion::is_compatible_with`, D7). **Nota (T019, D8)**:
    /// "Ready" aqui significa só "handshake válido, versão compatível" — NÃO
    /// implica `PluginState::Ready` diretamente. `update.rs` MUST checar
    /// `all_required_config_present` e, se `false`, transicionar para
    /// `Unavailable{NotConfigured}` em vez de `Ready` (a conexão continua
    /// viva/o worker continua rodando em ambos os casos; só o que `update.rs`
    /// faz com o resultado difere).
    Ready {
        result: HandshakeHelloResult,
        /// `true` ⟹ todo item de `result.required_config` resolveu contra
        /// `config.toml`/`secrets.toml` no momento em que este handshake
        /// completou (mesma fonte de dados usada para injetar variáveis de
        /// ambiente no spawn, T018 — ver [`required_config_fully_present`]
        /// para a checagem exata e a nota de design sobre por que ela é
        /// recomputada aqui, e não apenas "lembrada" do que foi de fato
        /// injetado no spawn).
        all_required_config_present: bool,
    },
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
    /// Resultado do handshake (T021/T022, T011/T019).
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
///
/// **T015 (correção C2 parte 2)**: usa `Subscription::run_with_id` em vez de
/// `Subscription::run` — antes desta feature, `worker` era um ponteiro de
/// função sem captura (`fn() -> S`, único plugin conhecido, comando/args
/// hardcoded em constantes globais); agora `worker` precisa capturar
/// `config` (comando/args/nome variam por plugin conhecido, T014), o que
/// `Subscription::run` não permite. `run_with_id` identifica a `Subscription`
/// pelo `plugin_name` — é isso que permite ao `iced` manter uma conexão por
/// plugin conhecido rodando simultaneamente, sem uma recriar/matar a outra
/// entre re-renders.
///
/// **Correção (T023, execução real com múltiplos plugins)**: devolve
/// `Subscription<(String, WorkerEvent)>` em vez de `Subscription<WorkerEvent>`
/// — o `plugin_name` já sai embutido em cada item do stream, produzido aqui
/// via `futures::StreamExt::map` (sem a restrição abaixo, por não passar
/// pelo `Subscription::map` do `iced`) em vez de deixar `update.rs` anexá-lo
/// depois. Antes desta correção, `update.rs::subscription` fazia
/// `plugin_worker::subscription(...).map(move |event| Message::Worker {
/// plugin_name: worker_plugin_name.clone(), event })` — um closure que
/// **captura** `worker_plugin_name`. `iced::Subscription::map` exige
/// `size_of::<F>() == 0` (`debug_assert!` em `iced_futures::subscription`,
/// mensagem "the closure ... is capturing") justamente para impedir esse
/// padrão, porque a forma como o runtime identifica/dedupa subscriptions
/// entre re-renders depende do tipo do closure, não do seu conteúdo
/// capturado — um closure capturante quebraria essa identidade em silêncio.
/// Isso nunca apareceu nos testes unitários (que exercitam `update.rs`
/// diretamente, sem passar pelo runtime `iced`) — só um `cargo run` real
/// (T023) o revela, como panic em `main` assim que a primeira `Subscription`
/// é montada.
///
/// **T032 (D8) — parâmetro `setup_attempt`**: novo, além de `config`. Compõe
/// o `id` da `Subscription` junto com `plugin_name`
/// (`format!("{plugin_name}-{setup_attempt}")`) — é o mecanismo de
/// reconexão descrito em `research.md` D8 ("Decisão — tela de setup"): o
/// chamador (`update.rs::subscription`) passa
/// `slot.connection.setup_attempt` (`model::PluginConnection`, incrementado
/// ao processar a submissão da tela de setup daquele plugin); quando esse
/// valor muda, o `id` muda, e o `iced` — que identifica/deduplica
/// `Subscription`s pelo `id` entre re-renders — encerra a `Subscription`
/// antiga (matando o processo filho anterior, `kill_on_drop` já configurado
/// em `worker`) e inicia esta função de novo do zero, com `worker(config)`
/// spawnando um processo novo que já enxerga as variáveis de ambiente
/// recém-persistidas em `config.toml`/`secrets.toml`. `setup_attempt` é
/// passado por valor (um `u32`, `Copy`) como argumento desta função — nunca
/// capturado por um closure de `Subscription::map` (ver a nota acima sobre
/// a armadilha de `size_of::<F>() == 0`); o único `.map()` aqui embaixo
/// continua zero-sized, usando só seu próprio parâmetro `event`.
pub fn subscription(config: PluginSpawnConfig, setup_attempt: u32) -> Subscription<(String, WorkerEvent)> {
    let id = format!("{}-{setup_attempt}", config.plugin_name);
    let plugin_name = config.plugin_name.clone();
    let stream = worker(config).map(move |event| (plugin_name.clone(), event));
    Subscription::run_with_id(id, stream)
}

/// Probe leve de `protocol_version`, usado pela correção C1 (T011) — ver
/// [`interpret_handshake_response`].
#[derive(serde::Deserialize)]
struct HandshakeSuccessVersionProbe {
    protocol_version: ProtocolVersion,
}

#[derive(serde::Deserialize)]
struct HandshakeResponseVersionProbe {
    result: Option<HandshakeSuccessVersionProbe>,
}

/// Interpreta a linha bruta de resposta do handshake (já decodificada como
/// `serde_json::Value`, não mais como `HandshakeHelloResponse` tipado
/// diretamente) — **correção C1/T011**.
///
/// Problema que esta função resolve: antes desta correção, a resposta do
/// handshake era desserializada como `HandshakeHelloResponse` tipado
/// **antes** de checar `protocol_version`. Com `Capability` tagueado por
/// `kind` (T008), a resposta `v0.1` de um plugin desatualizado
/// (`{"capabilities": ["exec"]}`, formato `string[]`) não desserializa em
/// nenhuma variante de `HandshakeHelloResponse` — o decode falhava primeiro
/// e o core nunca chegava a comparar a versão, caindo em `Unresponsive` em
/// vez de `VersionIncompatible` (Cenário 8 de `quickstart.md`, feature 002).
///
/// Correção: extrai só `protocol_version` via [`HandshakeResponseVersionProbe`]
/// **antes** de tentar o decode tipado completo — esse probe só exige que
/// `result.protocol_version` exista e seja uma string `"MAJOR.MINOR"`
/// válida, sem se importar com a forma do restante do objeto
/// (`capabilities`, `required_config`, ...). Se o probe conseguir extrair a
/// versão e ela for incompatível, retorna `VersionIncompatible` imediatamente,
/// sem sequer tentar o decode tipado completo (que falharia de qualquer
/// forma para um payload `v0.1`, mas por um motivo diferente e menos
/// informativo). Só quando o probe não se aplica (resposta de erro
/// JSON-RPC, sem campo `result`, ou JSON malformado) ou a versão já é
/// compatível, o código segue para o decode tipado completo — preservando o
/// comportamento anterior para esses dois casos (que já convergiam para
/// `Unresponsive`/`Ready`).
fn interpret_handshake_response(raw: serde_json::Value, plugin_name: &str) -> HandshakeOutcome {
    if let Ok(HandshakeResponseVersionProbe {
        result: Some(probe_result),
    }) = serde_json::from_value::<HandshakeResponseVersionProbe>(raw.clone())
    {
        if !probe_result
            .protocol_version
            .is_compatible_with(&CORE_PROTOCOL_VERSION)
        {
            return HandshakeOutcome::VersionIncompatible {
                plugin_version: probe_result.protocol_version,
                core_version: CORE_PROTOCOL_VERSION,
            };
        }
    }

    match serde_json::from_value::<HandshakeHelloResponse>(raw) {
        Err(_decode_error) => HandshakeOutcome::Unresponsive,
        Ok(HandshakeHelloResponse::Error { .. }) => HandshakeOutcome::Unresponsive,
        Ok(HandshakeHelloResponse::Success { result, .. }) => {
            if !result
                .protocol_version
                .is_compatible_with(&CORE_PROTOCOL_VERSION)
            {
                // Alcançável só se o probe acima não tiver decodificado
                // (ex.: campo ausente do payload por algum motivo) — mantém
                // a checagem como salvaguarda mesmo assim.
                return HandshakeOutcome::VersionIncompatible {
                    plugin_version: result.protocol_version,
                    core_version: CORE_PROTOCOL_VERSION,
                };
            }
            let all_required_config_present =
                required_config_fully_present(plugin_name, &result.required_config);
            HandshakeOutcome::Ready {
                result,
                all_required_config_present,
            }
        }
    }
}

/// **T018 (D8) — decisão de implementação, documentada por instrução
/// explícita da subtarefa**: `required_config` só é conhecido **depois** do
/// handshake (é o próprio plugin quem o declara na resposta) — no momento
/// em que o processo é spawnado, o core ainda não sabe quais nomes de
/// variável esperar. Duas alternativas foram consideradas:
///
/// 1. Spawnar sem nenhuma variável de ambiente do plugin e, só depois do
///    handshake revelar `required_config`, respawnar o processo (matando o
///    primeiro) já com as variáveis certas — mais fiel a "só injeta o que é
///    declaradamente necessário", mas exige uma segunda tentativa de spawn
///    inteira (handshake → kill → respawn → handshake de novo) só para
///    injetar env vars, adicionando uma complexidade de fluxo (e uma latência
///    de duplo handshake) desproporcional ao problema.
/// 2. **(Escolhida)** Injetar, já na primeira tentativa de spawn, **todo**
///    valor já persistido para este `plugin_name` em `config.toml`/
///    `secrets.toml` (T016/T017) — não filtrado por `required_config`
///    (que ainda não existe neste ponto). Um valor injetado que a versão
///    atual do plugin não declarar mais em `required_config` simplesmente
///    não é lido por ele (sem efeito colateral observável: o plugin só lê
///    as variáveis cujo nome ele mesmo deriva do seu próprio
///    `required_config`, D8) — o único custo é uma variável de ambiente a
///    mais no processo filho, que já herda dezenas de outras do processo
///    pai de qualquer forma.
///
/// Depois do handshake, [`required_config_fully_present`] refaz a mesma
/// consulta (`config_store`/`secrets_store`), desta vez filtrada pelos
/// itens que o `required_config` recém-recebido efetivamente declara, para
/// decidir `Ready` vs. `Unavailable{NotConfigured}` (T019, `update.rs`) —
/// mesma fonte de verdade (disco) usada aqui, então a decisão é consistente
/// com o que foi de fato injetado, sem precisar carregar um registro
/// separado de "quais env vars foram setadas nesta tentativa de spawn".
/// Risco aceito conscientemente: uma mutação de `config.toml`/`secrets.toml`
/// entre o spawn e a conclusão do handshake (janela de milissegundos, sem
/// concorrência esperada nesta feature) poderia, em teoria, fazer a checagem
/// pós-handshake divergir do que foi realmente injetado — não coberto por
/// nenhum requisito desta feature, sinalizado aqui para revisão.
fn stored_plugin_config_values(plugin_name: &str) -> Vec<(String, String)> {
    let mut values: Vec<(String, String)> = crate::config_store::load_plugin_config(plugin_name)
        .into_iter()
        .collect();
    values.extend(crate::secrets_store::load_plugin_secrets(plugin_name));
    values
}

/// Deriva o nome da variável de ambiente injetada para um item
/// (`plugin_name`, `name`) — convenção normativa de `research.md` D8
/// (`FAROL_PLUGIN_<PLUGIN_MAIÚSCULO>_<NAME_MAIÚSCULO>`, com qualquer
/// caractere não alfanumérico virando `_`), aplicada aqui de forma
/// determinística e idêntica ao que `plugins/uptime-kuma/config.py`/
/// `secrets.py` (T021/T022, fora do escopo desta subtarefa) devem aplicar
/// do lado do plugin — nenhum dos dois lados transmite o nome já prefixado
/// por protocolo, ambos derivam independentemente.
fn env_var_name(plugin_name: &str, item_name: &str) -> String {
    format!(
        "FAROL_PLUGIN_{}_{}",
        shout_snake(plugin_name),
        shout_snake(item_name)
    )
}

fn shout_snake(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' })
        .collect()
}

/// `true` ⟹ todo item de `required_config` resolve contra `config.toml`
/// (não-secreto) ou `secrets.toml` (secreto), conforme a flag `secret` de
/// cada item — usa `config_store::resolve_required_config_value`, a mesma
/// função que decide de qual dos dois arquivos ler (T016/T017/T018, D8).
/// Ver [`stored_plugin_config_values`] para a nota de design completa sobre
/// por que esta checagem é recomputada aqui, e não apenas "lembrada" da
/// injeção de spawn.
fn required_config_fully_present(plugin_name: &str, required_config: &[RequiredConfigItem]) -> bool {
    required_config.iter().all(|item| {
        crate::config_store::resolve_required_config_value(plugin_name, &item.name, item.secret)
            .is_some()
    })
}

/// Corpo do worker — ver a documentação do módulo para o fluxo completo.
///
/// Recebe `config` por valor (movido para dentro do `async move` do stream)
/// — desde T015/correção C2 parte 2, `worker` deixou de ser um ponteiro de
/// função sem captura (exigido por `Subscription::run`) porque agora precisa
/// variar por plugin conhecido; `Subscription::run_with_id` (ver
/// [`subscription`]) aceita qualquer `Stream`, não só um `fn() -> S`, o que
/// libera esta função para capturar `config` livremente.
fn worker(config: PluginSpawnConfig) -> impl Stream<Item = WorkerEvent> {
    stream::channel(16, move |mut output| async move {
        let mut command = Command::new(&config.command);
        command
            .args(&config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);

        // T018 (D8): injeta cada valor já persistido para este plugin como
        // variável de ambiente do processo filho — ver a documentação de
        // `stored_plugin_config_values` para a decisão de design completa
        // (por que não filtrado por `required_config`, ainda desconhecido
        // neste ponto).
        for (key, value) in stored_plugin_config_values(&config.plugin_name) {
            command.env(env_var_name(&config.plugin_name, &key), value);
        }

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                let _ = output
                    .send(WorkerEvent::SpawnFailed(format!(
                        "falha ao iniciar '{} {}': {err}",
                        config.command,
                        config.args.join(" ")
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

        // --- Handshake (T021, correção C1/T011) ---

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
                read_response::<serde_json::Value>(&mut reader),
            )
            .await
            {
                Err(_elapsed) => HandshakeOutcome::Unresponsive,
                Ok(Err(_io_or_decode_error)) => HandshakeOutcome::Unresponsive,
                Ok(Ok(None)) => HandshakeOutcome::Unresponsive, // EOF antes de responder.
                Ok(Ok(Some(raw_value))) => {
                    interpret_handshake_response(raw_value, &config.plugin_name)
                }
            }
        };

        let is_ready = matches!(handshake_outcome, HandshakeOutcome::Ready { .. });
        if output
            .send(WorkerEvent::HandshakeCompleted(handshake_outcome))
            .await
            .is_err()
        {
            return;
        }
        if !is_ready {
            // `Unavailable` é terminal para este caso nesta feature
            // (data-model.md §3) — o worker encerra; não há retry
            // automático. (Quando `is_ready` é `true` mas
            // `all_required_config_present` é `false` — `NotConfigured`,
            // T019 — o worker segue vivo abaixo; é `update.rs` quem decide
            // não chamar `widget/get` para essa conexão.)
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

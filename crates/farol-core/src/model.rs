//! Modelo de estado interno do core (o `Model` do padrão Model-Update-View do
//! iced) — data-model.md §2 ("Estado interno do core") e §3 ("Transições de
//! estado — `PluginState`").
//!
//! Estas estruturas nunca são serializadas para o plugin; existem só para o
//! core interpretar, ao longo do tempo, as entidades de protocolo trocadas
//! com o plugin. As entidades de *wire* (o que de fato trafega no handshake e
//! em `widget/get`) vêm de `farol_protocol` — reconciliação feita no Passo 0
//! desta subtarefa (T020-T029): os tipos locais temporários que existiam
//! aqui antes da dependência de `farol-protocol` existir foram removidos.

/// Identidade do plugin, capturada a partir de um handshake bem-sucedido
/// (data-model.md §1.6/§2.1: "Preenchida após handshake bem-sucedido
/// (`plugin_name`, `protocol_version`, `capabilities`)").
///
/// Decisão de design (documentada aqui, conforme pedido pela subtarefa):
/// este é um subconjunto de `farol_protocol::HandshakeHelloResult`, não o
/// `HandshakeHelloResult` inteiro. Motivo: `HandshakeHelloResult` também
/// carrega `widgets` e `actions`, que já têm lugar próprio no modelo local
/// (`PluginConnection::widgets` congela os widgets separadamente, e as
/// ações chegam via `WidgetItem::fetch_action` em cada `widget/get`, não
/// pelo handshake — ver `contracts/handshake.md`, nota de sequenciamento).
/// Guardar o resultado inteiro aqui duplicaria esses dois campos em dois
/// lugares do `Model`; este subconjunto evita a duplicação mantendo só o
/// que `data-model.md` §2.1 de fato atribui a `identity`.
///
/// **Correção H2 (T013)**: perdeu `derive(Eq)` — `capabilities` agora é
/// `farol_protocol::CapabilityManifest`, que carrega `Capability::Unknown`
/// (via `UnknownCapability.extra: serde_json::Map<String, serde_json::Value>`),
/// e `serde_json::Value` não implementa `Eq` (só `PartialEq`, por causa de
/// `f64` em números). `PartialEq` continua suficiente para os usos atuais
/// (comparação em teste/asserção), só `Eq`/`Hash` deixam de estar
/// disponíveis.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginIdentity {
    pub plugin_name: String,
    pub protocol_version: farol_protocol::ProtocolVersion,
    pub capabilities: farol_protocol::CapabilityManifest,
}

/// Motivo pelo qual a conexão com um plugin está no estado `Unavailable`
/// (data-model.md §2.1 e §3).
///
/// Uso interno/diagnóstico (D6) — a UI desta feature só precisa distinguir
/// "Unavailable" de "Starting/Handshaking/Ready"; não precisa distinguir
/// estes quatro casos visualmente entre si.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnavailableReason {
    /// O processo filho nem chegou a subir (binário ausente/não executável).
    FailedToStart,
    /// Handshake concluiu, mas as versões de protocolo são incompatíveis
    /// (FR-005, D7).
    VersionIncompatible,
    /// O processo terminou inesperadamente depois de já estar `Ready`
    /// (FR-019).
    Crashed,
    /// O processo está vivo, mas não respondeu dentro do
    /// `RPC_TIMEOUT_CONTROL` — seja no handshake, seja num ciclo de refresh
    /// (FR-019, D6). Não cobre o timeout de uma ação pontual
    /// (`RPC_TIMEOUT_ACTION` / `action/invoke`) — esse caso não produz este
    /// estado (ver `contracts/action-protocol.md`).
    Unresponsive,
    /// **Novo (T019, D8)**: o handshake completou (versão compatível), mas o
    /// core comparou o `required_config` declarado pelo plugin contra o que
    /// conseguiu resolver/injetar como variável de ambiente no spawn
    /// (`plugin_worker::required_config_fully_present`, T018) e encontrou ao
    /// menos um item sem valor armazenado (`config.toml`/`secrets.toml`
    /// ausente ou incompleto para este plugin).
    ///
    /// **Diferente das quatro variantes acima, esta MUST NOT ser tratada como
    /// terminal** — é a única exceção à regra "`Unavailable` é terminal
    /// nesta feature" herdada da feature 001 (`PluginState`, abaixo):
    /// `data-model.md` §3.2 desta feature (002) documenta um caminho de
    /// volta a `Starting`/`Handshaking` via uma tela de setup dentro do
    /// próprio Farol (`SetupForm`) que, ao ser submetida, persiste os
    /// valores em `config.toml`/`secrets.toml` e reinicia a `Subscription`
    /// do worker deste plugin com as novas variáveis de ambiente já
    /// injetadas. Essa tela de setup (e o mecanismo de reconexão que ela
    /// dispara) é tarefa futura (T029-T035 do `tasks.md` desta feature,
    /// fora do escopo desta subtarefa) — este comentário só registra que o
    /// tipo já MUST modelar `NotConfigured` como não-terminal, para não
    /// travar essa tela futura num dead-end de máquina de estados.
    NotConfigured,
}

/// Estado da conexão com um plugin (data-model.md §3 — máquina de estados).
///
/// Diagrama de transições (data-model.md §3):
///
/// ```text
/// Starting     ──(spawn falhou)───────────────────► Unavailable{FailedToStart}
/// Starting     ──(spawn ok)───────────────────────► Handshaking
/// Handshaking  ──(timeout RPC_TIMEOUT_CONTROL)────► Unavailable{Unresponsive}
/// Handshaking  ──(resposta, versão incompatível)──► Unavailable{VersionIncompatible}
/// Handshaking  ──(resposta, versão compatível, required_config OK)──► Ready
/// Handshaking  ──(resposta, versão compatível, required_config faltando)──► Unavailable{NotConfigured}
/// Ready        ──(child.wait() resolve)───────────► Unavailable{Crashed}
/// Ready        ──(timeout RPC_TIMEOUT_CONTROL)────► Unavailable{Unresponsive}
/// Ready        ──(widget/get e action/invoke ok)──► Ready (permanece)
/// Unavailable{FailedToStart,VersionIncompatible,Crashed,Unresponsive} ──(nenhuma transição)► (terminal)
/// Unavailable{NotConfigured} ──(usuário submete a tela de setup, T029-T035)──► Starting/Handshaking (não-terminal, T019)
/// ```
///
/// `Unavailable` é terminal nesta feature para quatro das cinco variantes —
/// reinício automático de plugin continua Fora de Escopo do `spec.md` para
/// elas. `NotConfigured` (T019, D8 de `specs/002-uptime-kuma-plugin/research.md`)
/// é a única exceção: precisa de um caminho de volta, documentado em
/// `UnavailableReason::NotConfigured` acima.
///
/// T017 define apenas este tipo; a lógica de transição fica para tasks
/// posteriores (T022, T026), que vivem fora do escopo desta subtarefa.
#[derive(Debug, Clone, PartialEq)]
pub enum PluginState {
    /// Processo filho sendo iniciado (`spawn()` em andamento).
    Starting,
    /// Processo vivo, handshake enviado, aguardando `HandshakeHelloResult`.
    Handshaking,
    /// Handshake concluído com versão compatível; widget(s) disponíveis
    /// para renderização.
    Ready,
    /// Estado terminal exibido como "indisponível" na UI (FR-020),
    /// distinguível de `Starting`/`Handshaking` (que são estados de
    /// "carregando", não de erro).
    Unavailable {
        reason: UnavailableReason,
        detail: String,
    },
}

/// Conexão do core com um plugin — agrega o estado da máquina de estados,
/// a identidade capturada no handshake e os widgets declarados
/// (data-model.md §2.1).
#[derive(Debug, Clone, PartialEq)]
pub struct PluginConnection {
    /// Estado atual da conexão — ver `PluginState`.
    pub state: PluginState,
    /// Preenchida após handshake bem-sucedido (`plugin_name`,
    /// `protocol_version`, `capabilities`). `None` enquanto `state` é
    /// `Starting`/`Handshaking`, ou se o handshake nunca chegou a
    /// completar (`Unavailable{FailedToStart}`/`Unavailable{Unresponsive}`
    /// antes da resposta).
    pub identity: Option<PluginIdentity>,
    /// Congelado no handshake (FR-006 — a lista de widgets não muda
    /// depois). Vazio até o handshake completar. Tipo real de
    /// `farol_protocol` (reconciliação do Passo 0).
    pub widgets: Vec<farol_protocol::WidgetDeclaration>,
    /// Últimos itens recebidos de `widget/get` para o widget `status-grid`
    /// (T028), enriquecidos com o estado de UI local de US2
    /// (`RepositoryViewModel`, data-model.md §2.2 — `fetch_in_flight`,
    /// `last_error`). A fusão de um novo `widget/get` com este vetor
    /// preserva `fetch_in_flight`/`last_error` dos repositórios já
    /// conhecidos (ver `update::merge_widget_items`) — um refresh periódico
    /// não deve apagar o feedback de uma ação em andamento/com erro.
    pub items: Vec<RepositoryViewModel>,
    /// Erro pontual da última chamada de `widget/get` (ex.: `scan_root`
    /// inacessível), quando houver. Por contrato (`widget-protocol.md`),
    /// um erro aqui NÃO muda `state` — os últimos `items` conhecidos são
    /// mantidos e só este campo é atualizado, para a UI poder sinalizar o
    /// problema sem perder os dados já exibidos.
    pub last_widget_error: Option<String>,
    /// T029/T031 (`data-model.md` §3.1) — análogo de `items`/
    /// `last_widget_error` acima, mas para o widget `monitor-status-grid`
    /// do plugin `uptime-kuma`. Populado por `update::handle_widget_outcome`
    /// a partir da variante `farol_protocol::messages::WidgetItems::Monitor`.
    /// Continua com o `Default` vazio (`MonitorWidgetViewModel::default()`)
    /// para qualquer conexão cujo widget declarado seja `status-grid`
    /// (`git-local`) — nunca populado nesse caso.
    pub monitor_widget: MonitorWidgetViewModel,
    /// T017 (`specs/004-vpn-status-plugin/data-model.md` §2.1) — análogo de
    /// `monitor_widget` acima, mas para o widget `vpn-status` do plugin
    /// `openfortivpn-vpn`. Populado por `update::handle_widget_outcome`/
    /// `handle_action_outcome` a partir das variantes
    /// `farol_protocol::messages::WidgetItems::Vpn`/
    /// `ActionInvokeResult::Vpn` (T024/T031, fora do escopo de T013-T019).
    /// Continua com o `Default` vazio (`VpnWidgetViewModel::default()`) para
    /// qualquer conexão cujo widget declarado não seja `"vpn-status"` —
    /// mesma regra já aplicada a `monitor_widget`.
    pub vpn_widget: VpnWidgetViewModel,
    /// T030 (D8) — formulário de setup ativo para esta conexão, presente
    /// se e somente se `state == Unavailable{reason: NotConfigured, ..}`
    /// (`update::handle_handshake_outcome` constrói/limpa este campo junto
    /// com a transição de `state`). `None` em qualquer outro estado.
    pub setup_form: Option<SetupForm>,
    /// T032 (D8) — contador de "tentativa de setup". Incrementado ao
    /// processar a submissão do formulário de setup deste plugin; usado
    /// para compor o `id` da `Subscription` do worker
    /// (`plugin_worker::subscription`, ver `research.md` D8 "Decisão — tela
    /// de setup"), forçando o `iced` a encerrar a conexão antiga (mata o
    /// processo filho anterior, `kill_on_drop`) e iniciar uma nova sempre
    /// que o valor muda — é assim que o novo processo passa a enxergar as
    /// variáveis de ambiente recém-persistidas em `config.toml`/
    /// `secrets.toml`.
    pub setup_attempt: u32,
}

/// Repositório exibido na UI, somando `farol_protocol::WidgetItem` (repo +
/// declaração de ação de fetch, como o plugin as enviou) com estado de UI
/// local que não vem do protocolo (data-model.md §2.2, US2/T032-T036).
///
/// `fetch_in_flight`/`last_error` existem só para o core saber desenhar
/// feedback de "em andamento"/"erro" por repositório — nunca são enviados ao
/// plugin nem influenciam `fetch_action.enabled` (T032: o core nunca decide
/// esse campo por conta própria, só o reflete).
#[derive(Debug, Clone, PartialEq)]
pub struct RepositoryViewModel {
    /// Último dado recebido de `widget/get` para este repositório, ou
    /// substituído diretamente pelo `GitRepository` pós-fetch de um
    /// `action/invoke` bem-sucedido (FR-018) — sem precisar de um
    /// `widget/get` extra para refletir o novo `ahead`/`behind`.
    pub repo: farol_protocol::GitRepository,
    /// Última declaração de ação de fetch conhecida para este repositório.
    pub fetch_action: farol_protocol::ActionDeclaration,
    /// `true` enquanto uma invocação de `action/invoke` está pendente para
    /// este repositório (T036) — estado de UI local, não de protocolo;
    /// evita a UI permitir clicar "Fetch" de novo antes da resposta
    /// anterior chegar (não há requisito formal de concorrência na spec,
    /// mas o Model precisa de *algum* estado para não desenhar dois
    /// indicadores conflitantes — data-model.md §2.2).
    pub fetch_in_flight: bool,
    /// Erro da última invocação de fetch para este repositório (FR-018),
    /// limpo no próximo sucesso. `None` enquanto nenhum fetch falhou ainda
    /// (ou o repositório nunca teve fetch invocado).
    pub last_error: Option<String>,
}

/// Estado de UI do widget `monitor-status-grid` do plugin `uptime-kuma`
/// (`specs/002-uptime-kuma-plugin/data-model.md` §3.1, T029) — análogo, para
/// este widget, de `items`/`last_widget_error` em `PluginConnection` para o
/// widget `status-grid`.
///
/// **Tipo de `last_error`**: `Option<String>`, não um tipo `PluginError`
/// dedicado. Não existe nenhum tipo genérico de erro assim em
/// `farol_protocol` (só `farol_protocol::ErrorObject`, o envelope JSON-RPC
/// completo) — o padrão já estabelecido nesta base para "erro pontual de um
/// ciclo de `widget/get`, guardado no `Model`" é
/// `PluginConnection::last_widget_error: Option<String>`, alimentado por
/// `plugin_worker::WidgetOutcome::PluginError(String)`, que por sua vez já
/// descarta `ErrorObject.data`/`reason` e guarda só `error.message` (texto
/// legível). Este tipo segue o mesmo padrão, em vez de inventar um novo tipo
/// de erro estruturado só para este widget.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MonitorWidgetViewModel {
    /// Último `items` recebido com sucesso de `widget/get` para o
    /// `widget_id` `"uptime-kuma-monitors"`. Mantido inalterado quando uma
    /// chamada de `widget/get` retorna erro pontual (`protocol/SPEC.md`
    /// §5.2) — o core preserva o último estado bom para exibição, mesmo
    /// mecanismo já usado para `RepositoryViewModel`/`items`.
    pub monitors: Vec<farol_protocol::messages::MonitorStatusItem>,
    /// Preenchido quando a última `widget/get` para este widget retornou
    /// `error` (`not_configured`/`metrics_unreachable`/`metrics_parse_error`);
    /// limpo no próximo sucesso. Distinto de `PluginState::Unavailable` — um
    /// erro de leitura pontual não muda o estado geral de disponibilidade da
    /// conexão (FR-017).
    pub last_error: Option<String>,
}

/// Estado de UI do widget `vpn-status` do plugin `openfortivpn-vpn`
/// (`specs/004-vpn-status-plugin/data-model.md` §2.1, T017) — análogo, para
/// este widget, de `monitor_widget` (`MonitorWidgetViewModel`) acima.
///
/// **Wiring desta subtarefa (T013-T019)**: só o tipo é introduzido aqui e o
/// campo `PluginConnection::vpn_widget` é adicionado com `Default` vazio —
/// popular `status`/`last_error` a partir de um `widget/get` bem-sucedido
/// (mesmo mecanismo já usado para `monitor_widget` em
/// `update::handle_widget_outcome`) é escopo de T024, e
/// `connect_in_flight`/`disconnect_in_flight`/`last_action_error` a partir de
/// `action/invoke` (`vpn.connect`/`vpn.disconnect`) é escopo de T031 — ambas
/// tasks futuras, fora desta subtarefa.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VpnWidgetViewModel {
    /// Último `VpnStatusItem` recebido com sucesso de `widget/get` para o
    /// `widget_id` `"vpn-connection"`. `None` só antes do primeiro
    /// `widget/get` bem-sucedido (mesmo espírito de
    /// `MonitorWidgetViewModel::monitors` vazio inicialmente) — diferente de
    /// `monitors`, que é sempre uma lista (mesmo vazia), este widget é
    /// singleton (`research.md` D3: no máximo uma sessão VPN por vez), por
    /// isso `Option<VpnStatusItem>` em vez de `Vec<VpnStatusItem>`.
    pub status: Option<farol_protocol::messages::VpnStatusItem>,
    /// Erro pontual do último `widget/get` deste widget
    /// (`vpn_status_unavailable`/`exec_unavailable`, `research.md` D5),
    /// preservando `status` anterior — mesmo padrão de
    /// `MonitorWidgetViewModel::last_error`. Distinto de
    /// `PluginState::Unavailable` — um erro de leitura pontual não muda o
    /// estado geral de disponibilidade da conexão.
    pub last_error: Option<String>,
    /// `true` enquanto uma invocação de `action/invoke` de `vpn.connect`
    /// está pendente para este widget (`research.md` D7) — estado de UI
    /// local, não de protocolo; mesmo padrão de
    /// `RepositoryViewModel::fetch_in_flight`.
    pub connect_in_flight: bool,
    /// Idem, para `vpn.disconnect`.
    pub disconnect_in_flight: bool,
    /// Erro da última invocação de `vpn.connect`/`vpn.disconnect` (mensagem
    /// já traduzida pelo plugin, FR-007 de `spec.md`), limpo no próximo
    /// sucesso — mesmo padrão de `RepositoryViewModel::last_error` para
    /// `git.fetch`. Distinto de `last_error` acima, que é sobre `widget/get`,
    /// não sobre uma ação.
    pub last_action_error: Option<String>,
}

/// Estado de UI do formulário de setup de um plugin
/// (`specs/002-uptime-kuma-plugin/data-model.md` §3.2, T030, D8) —
/// consumido pela `view` quando `PluginState::Unavailable{reason:
/// NotConfigured, ..}`, em vez do widget normal daquele plugin.
///
/// **Desvio de local documentado**: `data-model.md` §3.2 descreve este tipo
/// como vivendo "fora de `PluginConnection`", associado à conexão só por um
/// identificador de plugin (`Farol`, em `main.rs`, agrega múltiplas
/// conexões desde a correção C2). Esta subtarefa (T029-T035) foi escopada
/// para tocar só `model.rs`/`update.rs`/`view.rs` (e a assinatura de
/// `plugin_worker::subscription`) — `main.rs`, onde vivem `Farol`,
/// `PluginSlot` e `Message`, está fora do escopo autorizado. Guardar
/// `SetupForm` como campo de `PluginConnection`
/// (`PluginConnection::setup_form`) é o jeito de associá-lo à conexão certa
/// sem editar `main.rs`; a associação por `plugin_name` que a nota do
/// `data-model.md` pede continua satisfeita (o campo `plugin_name` abaixo
/// é redundante com a chave de `PluginSlot.spawn_config.plugin_name`, mas
/// preservado porque `data-model.md` o especifica explicitamente) — só a
/// localização física do campo dentro do `Model` muda.
#[derive(Debug, Clone, PartialEq)]
pub struct SetupForm {
    /// Identifica a qual conexão este formulário pertence.
    pub plugin_name: String,
    /// Um par (`item` declarado no handshake, valor digitado até agora) por
    /// item de `required_config` — inicializado com string vazia por campo;
    /// `secret: true` ⟹ a `view` MUST renderizar como campo mascarado.
    pub fields: Vec<(farol_protocol::messages::RequiredConfigItem, String)>,
}

impl From<farol_protocol::WidgetItem> for RepositoryViewModel {
    /// Converte um item recém-chegado de `widget/get` sem nenhum estado de
    /// UI anterior (`fetch_in_flight: false`, `last_error: None`) — quem
    /// funde uma lista inteira de itens (`update::merge_widget_items`) é
    /// responsável por reaplicar o estado do item anterior correspondente,
    /// quando existir.
    fn from(item: farol_protocol::WidgetItem) -> Self {
        Self {
            repo: item.repo,
            fetch_action: item.fetch_action,
            fetch_in_flight: false,
            last_error: None,
        }
    }
}

impl Default for PluginConnection {
    /// Estado inicial de uma conexão recém-criada, antes de qualquer evento
    /// do worker chegar (data-model.md §3: `Starting` é o estado inicial da
    /// máquina).
    fn default() -> Self {
        Self {
            state: PluginState::Starting,
            identity: None,
            widgets: Vec::new(),
            items: Vec::new(),
            last_widget_error: None,
            monitor_widget: MonitorWidgetViewModel::default(),
            vpn_widget: VpnWidgetViewModel::default(),
            setup_form: None,
            setup_attempt: 0,
        }
    }
}

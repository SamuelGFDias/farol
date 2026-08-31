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
#[derive(Debug, Clone, PartialEq, Eq)]
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
/// Handshaking  ──(resposta, versão compatível)────► Ready
/// Ready        ──(child.wait() resolve)───────────► Unavailable{Crashed}
/// Ready        ──(timeout RPC_TIMEOUT_CONTROL)────► Unavailable{Unresponsive}
/// Ready        ──(widget/get e action/invoke ok)──► Ready (permanece)
/// Unavailable  ──(nenhuma transição nesta feature)► (terminal)
/// ```
///
/// `Unavailable` é terminal nesta feature — reinício automático de plugin é
/// Fora de Escopo do `spec.md`.
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
        }
    }
}

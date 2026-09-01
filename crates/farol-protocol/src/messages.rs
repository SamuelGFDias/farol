//! Tipos de mensagem do protocolo Farol v0.2.
//!
//! Espelha, campo a campo, os quatro JSON Schemas normativos em `protocol/schema/v0.2/`:
//! `handshake.schema.json`, `widget.schema.json`, `action.schema.json` e `error.schema.json` —
//! juntos com `protocol/SPEC.md`, eles são a fonte da verdade sobre a forma exata de cada
//! mensagem; este módulo é apenas o binding Rust dessa forma. Nomes de tipo e de campo seguem o
//! vocabulário exato dos schemas.
//!
//! Evolução de v0.1 (`specs/002-uptime-kuma-plugin/research.md` D1/D4/D8,
//! `specs/002-uptime-kuma-plugin/data-model.md` §1.1-§1.6): `CapabilityManifest.capabilities`
//! passa de `Vec<String>` para `Vec<Capability>` (objetos discriminados por `kind`, sem mais a
//! variante `"secret"`); `HandshakeHelloResult` ganha o campo `required_config`; e
//! `WidgetGetResult.items` passa a aceitar `WidgetItem` (git) **ou** `MonitorStatusItem`
//! (uptime-kuma) via `WidgetItems`, uma união discriminada pelo `kind` que o `widget_id`
//! declarou no handshake — nunca misto na mesma resposta.
//!
//! Todo `jsonrpc`/`method` de request é um valor `const` no schema (ex.: `"handshake/hello"`);
//! aqui eles são campos `String` preenchidos pelos construtores `new()` de cada tipo de request a
//! partir das constantes [`JSONRPC_VERSION`] e `METHOD_*` abaixo, para não introduzir um tipo de
//! marcador dedicado por método.

use serde::{Deserialize, Serialize};

use crate::version::ProtocolVersion;

/// Valor do campo `jsonrpc` em toda mensagem desta especificação (JSON-RPC 2.0, `protocol/SPEC.md` §2).
pub const JSONRPC_VERSION: &str = "2.0";

/// Valor do campo `method` de uma requisição `handshake/hello` (`protocol/SPEC.md` §5.1/§6.2).
pub const METHOD_HANDSHAKE_HELLO: &str = "handshake/hello";

/// Valor do campo `method` de uma requisição `widget/get` (`protocol/SPEC.md` §5.2).
pub const METHOD_WIDGET_GET: &str = "widget/get";

/// Valor do campo `method` de uma requisição `action/invoke` (`protocol/SPEC.md` §5.3).
pub const METHOD_ACTION_INVOKE: &str = "action/invoke";

/// Id de requisição JSON-RPC — escolhido pelo core, `integer` ou `string`, único dentro de uma
/// conexão enquanto não resolvido (`protocol/SPEC.md` §2.1, `RequestId` em todos os 4 schemas).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    Integer(i64),
    String(String),
}

// ---------------------------------------------------------------------------------------------
// handshake.schema.json
// ---------------------------------------------------------------------------------------------

/// Params do request `handshake/hello` — `HandshakeHelloParams` em `handshake.schema.json`,
/// nomeado `HandshakeHello` em `data-model.md` §1.6.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandshakeHello {
    /// Versão de protocolo que o core suporta.
    pub protocol_version: ProtocolVersion,
    /// Identifica o core que envia o request (ex.: `"farol-core"`). Informativo apenas — não usado
    /// pela regra de compatibilidade.
    pub core_name: String,
}

/// Envelope completo do request `handshake/hello` (`HandshakeHelloRequest` em
/// `handshake.schema.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandshakeHelloRequest {
    pub jsonrpc: String,
    pub id: RequestId,
    pub method: String,
    pub params: HandshakeHello,
}

impl HandshakeHelloRequest {
    /// Constrói o envelope com `jsonrpc`/`method` já preenchidos com os valores `const` corretos.
    pub fn new(id: RequestId, params: HandshakeHello) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            method: METHOD_HANDSHAKE_HELLO.to_string(),
            params,
        }
    }
}

/// Uma capacidade com `kind` dentro do vocabulário conhecido desta versão do protocolo
/// (`protocol/schema/v0.2/handshake.schema.json` `$defs/Capability`). **Sem** variante
/// `Secret` — removida em v0.2: credencial passa a ser declarada via `RequiredConfigItem`, não
/// via `Capability` (`research.md` D1/D8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum KnownCapability {
    Exec,
    Network {
        /// Nome de host ou IP (Princípio IV da constitution: allowlist é por host, não por
        /// URL/path).
        host: String,
        /// Porta, quando fixa/conhecida (ex.: 443). Ausente = não declarado, sem inferência de
        /// default pelo core.
        #[serde(skip_serializing_if = "Option::is_none")]
        port: Option<u16>,
    },
}

/// Uma capacidade cujo `kind` está fora do vocabulário conhecido por este binding (forward
/// compat — ver decisão em [`Capability`]). Preserva o `kind` literal e quaisquer campos
/// adicionais do objeto, sem interpretá-los.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnknownCapability {
    pub kind: String,
    /// Campos adicionais do objeto além de `kind`, preservados sem interpretação.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Uma capacidade declarada pelo plugin (`protocol/SPEC.md` §6.3, `data-model.md` §1.1). O
/// vocabulário de `kind` é aberto por desenho (`protocol/SPEC.md` §10) — um `kind` desconhecido
/// pelo core MUST ser aceito, registrado e exibido genericamente, sem que seus campos extras
/// sejam interpretados.
///
/// **Decisão de forward-compat** (nota de `research.md` D1, resolvida aqui): um enum
/// `#[serde(tag = "kind")]` puro (internamente tagueado) rejeita, na desserialização, qualquer
/// `kind` fora do vocabulário conhecido (`"exec"`/`"network"`) — diferente do JSON Schema
/// ilustrativo (`additionalProperties: true`), que aceita e ignora campos extras de um `kind`
/// desconhecido em vez de falhar. Para reconciliar isso, `Capability` é modelado como um enum
/// `#[serde(untagged)]` de duas variantes: [`KnownCapability`], tentada primeiro, e
/// [`UnknownCapability`] como fallback que preserva o `kind` literal e o restante do objeto em
/// `extra`. Trade-off aceito conscientemente: um objeto malformado de um `kind` *conhecido*
/// (ex.: `{"kind":"network"}` sem `host`) também cai em `Unknown` em vez de falhar a
/// desserialização com um erro específico — o `untagged` não distingue "kind conhecido mas
/// payload inválido" de "kind desconhecido" sem uma solução mais elaborada (`Deserialize`
/// manual com peek no campo `kind`), que não se justifica para este binding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Capability {
    Known(KnownCapability),
    Unknown(UnknownCapability),
}

/// Manifesto de capacidades declarado pelo plugin (`protocol/SPEC.md` §6.3). Sem enforcement
/// nesta versão do protocolo — o core apenas registra e exibe o que o plugin declara.
/// `capabilities` MAY ser `[]` — um plugin sem nenhuma capacidade concreta para declarar
/// honestamente ainda (ex.: `uptime-kuma` antes de `base_url` resolvido) reporta lista vazia.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapabilityManifest {
    /// Capacidades declaradas, cada uma discriminada por `kind`.
    pub capabilities: Vec<Capability>,
}

/// Declara um widget oferecido pelo plugin (`protocol/SPEC.md` §6.3). A lista de
/// `WidgetDeclaration`s devolvida no handshake é congelada pelo resto da conexão.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WidgetDeclaration {
    /// Identificador estável do widget dentro deste plugin (ex.: `"repo-status"`). Usado como
    /// `widget_id` em requests `widget/get`.
    pub id: String,
    /// Tipo declarativo do widget (ex.: `"status-grid"`, `"monitor-status-grid"`). Vocabulário do
    /// core, não do plugin — um `kind` desconhecido pelo core MUST ser ignorado silenciosamente
    /// (widget não renderizado). `"monitor-status-grid"` (novo em v0.2, `research.md` D4) reporta
    /// itens `MonitorStatusItem`, independente do vocabulário `"status-grid"`/`WidgetItem`.
    pub kind: String,
    /// Rótulo legível exibido pelo core (ex.: `"Repositórios Git"`).
    pub title: String,
    /// Intervalo de refresh sugerido em milissegundos. Ausente ⟹ o core MUST aplicar o default de
    /// 30000ms.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_refresh_interval_ms: Option<u64>,
}

/// Identifica o objeto sobre o qual uma ação opera (`protocol/SPEC.md` §5.3/§6.3). O core MUST
/// ecoar este valor de volta, literalmente, em `action/invoke` — nunca reconstruído por conta
/// própria.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionTarget {
    /// Tipo de alvo. Nesta versão do protocolo / no plugin de referência `git-local`, sempre
    /// `"repo"`.
    #[serde(rename = "type")]
    pub r#type: String,
    /// Identificador do alvo dentro do tipo (ex.: caminho absoluto de um repositório).
    pub id: String,
}

/// Declara uma ação oferecida pelo plugin sobre um alvo específico, simétrica a
/// `WidgetDeclaration` (`protocol/SPEC.md` §6.3). MAY vir no handshake e/ou em respostas
/// subsequentes de `widget/get` (§6.3.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionDeclaration {
    /// Identificador estável da ação (ex.: `"git.fetch"`) — tipicamente reutilizado por muitos
    /// alvos; `target` é o que diferencia cada instância.
    pub id: String,
    /// Rótulo legível para exibição na UI (ex.: `"Fetch"`).
    pub label: String,
    /// Objeto sobre o qual esta instância de ação opera.
    pub target: ActionTarget,
    /// Se esta ação está invocável no momento, conforme declarado pelo plugin. O core MUST NOT
    /// decidir isso por conta própria e MUST NOT permitir invocar uma ação com `enabled: false`.
    pub enabled: bool,
    /// Orçamento de timeout sugerido pelo plugin, em milissegundos, para `RPC_TIMEOUT_ACTION`
    /// quando esta ação for invocada. Ausente ⟹ o core MUST aplicar o default de 120000ms.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_hint_ms: Option<u64>,
}

/// Declara uma variável de configuração que o plugin precisa do usuário, secreta ou não
/// (`research.md` D8, `data-model.md` §1.6.1). O core, não o plugin, resolve, armazena e injeta
/// o valor — o plugin só lê a variável de ambiente já injetada no seu próprio arranque.
/// Declarada sempre, por completo, independentemente de já haver um valor armazenado.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequiredConfigItem {
    /// Identificador estável da variável, escolhido pelo plugin (ex.: `"base_url"`,
    /// `"api_key"`). Usado pelo core para derivar o nome da variável de ambiente injetada
    /// (`FAROL_PLUGIN_<PLUGIN_NAME>_<NAME>`) e para indexar `config.toml`/`secrets.toml`.
    pub name: String,
    /// `true` ⟹ o core MUST armazenar em `secrets.toml` (nunca em `config.toml`) e mascarar o
    /// campo correspondente na tela de setup.
    pub secret: bool,
    /// Rótulo legível exibido como label do campo na tela de setup.
    pub description: String,
}

/// Result de sucesso do `handshake/hello` (`protocol/SPEC.md` §6.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandshakeHelloResult {
    /// Versão de protocolo falada pelo plugin.
    pub protocol_version: ProtocolVersion,
    /// Identidade do plugin (ex.: `"git-local"`).
    pub plugin_name: String,
    pub capabilities: CapabilityManifest,
    /// Variáveis de configuração que o plugin precisa do usuário (`research.md` D8). MAY ser
    /// vazia para um plugin que não precisa de nenhuma configuração (ex.: `git-local`).
    pub required_config: Vec<RequiredConfigItem>,
    /// Widgets oferecidos por este plugin. Congelado pelo resto da conexão.
    pub widgets: Vec<WidgetDeclaration>,
    /// Ações já conhecidas no momento do handshake. MAY ser vazia quando a lista completa só é
    /// conhecível após uma consulta subsequente (`protocol/SPEC.md` §6.3.1) — o core MUST tratar
    /// esta lista como "conhecida até agora", não como definitiva.
    pub actions: Vec<ActionDeclaration>,
}

/// Resposta a um `handshake/hello` — sucesso (`result`) ou recusa do plugin (`error`, tipicamente
/// código `-32000`/`protocol_version_incompatible`, `protocol/SPEC.md` §6.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HandshakeHelloResponse {
    Success {
        jsonrpc: String,
        id: RequestId,
        result: HandshakeHelloResult,
    },
    Error {
        jsonrpc: String,
        id: RequestId,
        error: ErrorObject,
    },
}

// ---------------------------------------------------------------------------------------------
// widget.schema.json
// ---------------------------------------------------------------------------------------------

/// Estado do remoto de um repositório — união discriminada por `kind`
/// (`protocol/SPEC.md`/`widget.schema.json`). `NoRemote` é uma serialização distinta de um
/// repositório rastreado reportando 0 ahead / 0 behind: o core nunca precisa inferir "sem remoto"
/// a partir de um campo ausente.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RemoteStatus {
    Tracked { ahead: u64, behind: u64 },
    NoRemote,
}

/// Um repositório reportado por `widget/get` (`protocol/SPEC.md` §5.2, `data-model.md` §1.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitRepository {
    /// Identificador estável — caminho absoluto do repositório.
    pub id: String,
    /// Nome curto para exibição (ex.: nome do diretório).
    pub name: String,
    /// Caminho absoluto no filesystem.
    pub path: String,
    /// `true` = working tree com mudanças pendentes; `false` = limpa.
    pub dirty: bool,
    /// MUST ser `kind: "no_remote"` se e somente se a `ActionDeclaration` de fetch correspondente
    /// tiver `enabled: false`.
    pub remote_status: RemoteStatus,
}

/// Pareia um `GitRepository` com sua `ActionDeclaration` de fetch correspondente — é assim que a
/// lista de ações, possivelmente vazia no handshake, chega ao core na prática
/// (`protocol/SPEC.md` §6.3.1, `widget.schema.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WidgetItem {
    pub repo: GitRepository,
    /// MUST ter `enabled: false` sempre que `repo.remote_status.kind == "no_remote"`.
    pub fetch_action: ActionDeclaration,
}

/// Estado de um monitor, mapeado do valor bruto `monitor_status` do `/metrics` do Uptime Kuma
/// (`research.md` D4, FR-012): `1→up`, `0→down`, `2→pending`, `3→maintenance`. Um valor bruto
/// fora de `{0,1,2,3}` não produz este item — invalida a leitura inteira (`metrics_parse_error`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MonitorStatus {
    Up,
    Down,
    Pending,
    Maintenance,
}

/// Um monitor reportado por `widget/get` para um widget declarado com
/// `kind: "monitor-status-grid"` (novo em v0.2, `research.md` D4, `data-model.md` §1.3).
/// Diferente de [`WidgetItem`], nunca carrega nenhuma `ActionDeclaration` associada — este
/// plugin nunca declara ações (FR-004); o campo simplesmente não existe neste tipo, não é um
/// campo opcional vazio.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorStatusItem {
    /// Nome de exibição do monitor, do label `monitor_name` do `/metrics` — possivelmente
    /// sanitizado em relação ao nome original do Uptime Kuma.
    pub name: String,
    pub status: MonitorStatus,
    /// De `monitor_response_time` (ms), quando aplicável ao status do monitor; `None` serializa
    /// como `null` explícito (sem `skip_serializing_if`) — campo obrigatório e nullable, nunca
    /// ausente, mesmo espírito de `RemoteStatus::NoRemote`.
    pub response_time_ms: Option<u32>,
}

/// União discriminada pelo `kind` do widget que originou a resposta de `widget/get` — `Git`
/// (`Vec<WidgetItem>`) para `kind: "status-grid"`, ou `Monitor` (`Vec<MonitorStatusItem>`) para
/// `kind: "monitor-status-grid"`. Nunca mista: cada resposta contém só um dos dois vocabulários
/// (`data-model.md` §1.4, correção C3; `protocol/schema/v0.2/widget.schema.json`
/// `WidgetGetResult.items`, um `oneOf` de dois arrays).
///
/// `#[serde(untagged)]` faz `items` serializar, no wire, como o array simples já fixado pelo
/// schema JSON — sem tag/envelope extra. Decisão de desenho (item de C3 marcado como "decisão de
/// implementação"): um enum sobre o `Vec<T>` inteiro, não um enum por elemento nem dois campos
/// `Option<Vec<T>>` mutuamente exclusivos — porque o discriminante real é o `widget_id`/`kind`
/// do *pedido* inteiro (conhecido pelo core antes mesmo de receber a resposta, `data-model.md`
/// §1.4), nunca um dado por item dentro do array. Ambiguidade aceita conscientemente: como as
/// duas variantes serializam como `Vec<T>` simples, um array vazio `[]` desserializa sempre como
/// a primeira variante tentada (`Git`, pela ordem de declaração) — inofensivo na prática porque
/// o core nunca infere a forma pelo conteúdo de `items` isoladamente; ele já sabe, pelo `kind`
/// que o `widget_id` declarou no handshake, qual variante esperar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WidgetItems {
    Git(Vec<WidgetItem>),
    Monitor(Vec<MonitorStatusItem>),
}

/// Params do request `widget/get`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WidgetGetParams {
    /// MUST corresponder ao `id` de uma `WidgetDeclaration` devolvida pelo `handshake/hello` deste
    /// plugin.
    pub widget_id: String,
}

/// Envelope completo do request `widget/get`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WidgetGetRequest {
    pub jsonrpc: String,
    pub id: RequestId,
    pub method: String,
    pub params: WidgetGetParams,
}

impl WidgetGetRequest {
    pub fn new(id: RequestId, params: WidgetGetParams) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            method: METHOD_WIDGET_GET.to_string(),
            params,
        }
    }
}

/// Result de sucesso do `widget/get`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WidgetGetResult {
    /// Ecoa o `widget_id` do request.
    pub widget_id: String,
    /// `Vec<WidgetItem>` ou `Vec<MonitorStatusItem>`, conforme o `kind` que este `widget_id`
    /// declarou no handshake (correção C3, `data-model.md` §1.4). MAY ser vazia para qualquer
    /// `kind` — um `scan_root` configurado sem repositórios embaixo, ou uma instância Uptime Kuma
    /// sem monitores cadastrados, são ambos estados válidos, não erros.
    pub items: WidgetItems,
}

/// Resposta a um `widget/get` — sucesso ou erro pontual (ex.: `-32004`/`scan_root_unreadable`).
/// Um erro aqui não muda, por si só, a disponibilidade da conexão com o plugin
/// (`protocol/SPEC.md` §5.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WidgetGetResponse {
    Success {
        jsonrpc: String,
        id: RequestId,
        result: WidgetGetResult,
    },
    Error {
        jsonrpc: String,
        id: RequestId,
        error: ErrorObject,
    },
}

// ---------------------------------------------------------------------------------------------
// action.schema.json
// ---------------------------------------------------------------------------------------------

/// Params do request `action/invoke`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionInvokeParams {
    /// MUST corresponder ao `id` de uma `ActionDeclaration` previamente declarada pelo plugin para
    /// `target`, com `enabled: true`.
    pub action_id: String,
    /// MUST ser ecoado, literalmente, do `target` da `ActionDeclaration` que motivou esta
    /// invocação — o core MUST NOT reconstruir um `target` por conta própria.
    pub target: ActionTarget,
}

/// Envelope completo do request `action/invoke`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionInvokeRequest {
    pub jsonrpc: String,
    pub id: RequestId,
    pub method: String,
    pub params: ActionInvokeParams,
}

impl ActionInvokeRequest {
    pub fn new(id: RequestId, params: ActionInvokeParams) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            method: METHOD_ACTION_INVOKE.to_string(),
            params,
        }
    }
}

/// Result de sucesso de `action/invoke`. Para `git.fetch`, é o estado pós-fetch do repositório —
/// o core substitui os dados do repositório diretamente por este valor, sem um `widget/get`
/// adicional (`protocol/SPEC.md` FR-018 / `data-model.md` §4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionInvokeResult {
    pub repo: GitRepository,
}

/// Resposta a um `action/invoke` — sucesso ou erro pontual (`-32001`/`fetch_failed`,
/// `-32002`/`action_timeout`, `-32003`/`exec_unavailable`). Nunca, por si só, encerra o processo
/// do plugin nem marca a conexão como indisponível (`protocol/SPEC.md` §8.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ActionInvokeResponse {
    Success {
        jsonrpc: String,
        id: RequestId,
        result: ActionInvokeResult,
    },
    Error {
        jsonrpc: String,
        id: RequestId,
        error: ErrorObject,
    },
}

// ---------------------------------------------------------------------------------------------
// error.schema.json
// ---------------------------------------------------------------------------------------------

/// Objeto de erro JSON-RPC padrão, comum a qualquer resposta de erro de qualquer um dos três
/// métodos (`protocol/SPEC.md` §8, `error.schema.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorObject {
    /// Código JSON-RPC padrão reservado (`-32700`..`-32600`..`-32603`) ou código de domínio Farol
    /// (`-32000` a `-32099`).
    pub code: i64,
    /// Mensagem legível para humano/log — nunca destinada a ser parseada por código;
    /// `code`/`data.reason` MUST distinguir tipos de erro, nunca o conteúdo de `message`.
    pub message: String,
    /// Detalhe estruturado adicional. Quando presente para um código de domínio Farol,
    /// `data.reason` deve identificar a causa específica (§8.2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<ErrorData>,
}

/// Corpo do campo `data` de um `ErrorObject`. `reason` é deixado como string aberta (não um enum
/// fechado) por decisão explícita de forward-compatibility (`protocol/SPEC.md` §8,
/// `error.schema.json`): um plugin futuro MAY reservar novos valores de `reason` dentro da mesma
/// faixa de `code`, sem que isso quebre um binding mais antigo. Campos adicionais além de `reason`
/// (ex.: `detail`) variam por `reason` e são preservados via `extra`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ErrorData {
    /// Razão de domínio Farol (ex.: `"fetch_failed"`, `"protocol_version_incompatible"`).
    /// Requerida pela especificação sempre que `code` for um código de domínio Farol, mas mantida
    /// `Option` aqui porque o schema não a marca `required` no nível de validação de JSON Schema —
    /// apenas na prosa normativa.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Campos adicionais de `data` além de `reason` (ex.: `detail`, `target`) — abertos por
    /// desenho (`additionalProperties: true` em `error.schema.json`).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_hello_request_round_trips() {
        let req = HandshakeHelloRequest::new(
            RequestId::Integer(1),
            HandshakeHello {
                protocol_version: ProtocolVersion::new(0, 1),
                core_name: "farol-core".to_string(),
            },
        );

        let json = serde_json::to_string(&req).unwrap();
        let back: HandshakeHelloRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back, req);
        assert_eq!(req.jsonrpc, JSONRPC_VERSION);
        assert_eq!(req.method, METHOD_HANDSHAKE_HELLO);
    }

    /// Adaptado do exemplo de `git-local` em `protocol/SPEC.md` §4.1 para a forma de wire v0.2
    /// (`capabilities` como objetos, `required_config` novo) — o §4.1 do próprio `SPEC.md` ainda
    /// traz a forma v0.1 literal naquele parágrafo (ilustra só o framing NDJSON, não o conteúdo
    /// do handshake); o exemplo atualizado para v0.2 vive em §6.3, reproduzido aqui só para o caso
    /// `git-local` (sem `required_config`).
    #[test]
    fn handshake_hello_response_success_matches_spec_example() {
        let raw = r#"{"jsonrpc":"2.0","id":1,"result":{"protocol_version":"0.2","plugin_name":"git-local","capabilities":{"capabilities":[{"kind":"exec"}]},"required_config":[],"widgets":[{"id":"repo-status","kind":"status-grid","title":"Repositórios Git"}],"actions":[]}}"#;

        let parsed: HandshakeHelloResponse = serde_json::from_str(raw).unwrap();
        match &parsed {
            HandshakeHelloResponse::Success { id, result, .. } => {
                assert_eq!(*id, RequestId::Integer(1));
                assert_eq!(result.protocol_version, ProtocolVersion::new(0, 2));
                assert_eq!(result.plugin_name, "git-local");
                assert_eq!(
                    result.capabilities.capabilities,
                    vec![Capability::Known(KnownCapability::Exec)]
                );
                assert!(result.required_config.is_empty());
                assert_eq!(result.widgets.len(), 1);
                assert_eq!(result.widgets[0].id, "repo-status");
                assert_eq!(result.widgets[0].kind, "status-grid");
                assert!(result.widgets[0].suggested_refresh_interval_ms.is_none());
                assert!(result.actions.is_empty());
            }
            HandshakeHelloResponse::Error { .. } => panic!("esperava Success"),
        }

        // Round-trip: re-serializar deve reproduzir exatamente a mesma linha compacta, já que os
        // campos opcionais ausentes não devem reaparecer na saída.
        let re_encoded = serde_json::to_string(&parsed).unwrap();
        assert_eq!(re_encoded, raw);
    }

    /// `kind` fora do vocabulário conhecido (`Capability::Unknown`) é aceito e preserva o `kind`
    /// literal e os campos extras, sem interpretá-los (`research.md` D1, forward-compat).
    #[test]
    fn capability_unknown_kind_round_trips_preserving_extra_fields() {
        let raw = r#"{"kind":"gpu","cores":8}"#;
        let parsed: Capability = serde_json::from_str(raw).unwrap();
        match &parsed {
            Capability::Unknown(unknown) => {
                assert_eq!(unknown.kind, "gpu");
                assert_eq!(unknown.extra.get("cores").unwrap(), 8);
            }
            Capability::Known(_) => panic!("esperava Unknown"),
        }
        let re_encoded = serde_json::to_string(&parsed).unwrap();
        assert_eq!(re_encoded, raw);
    }

    #[test]
    fn handshake_hello_response_error_round_trips() {
        let raw = r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"versao incompativel","data":{"reason":"protocol_version_incompatible"}}}"#;
        let parsed: HandshakeHelloResponse = serde_json::from_str(raw).unwrap();
        match &parsed {
            HandshakeHelloResponse::Error { error, .. } => {
                assert_eq!(error.code, -32000);
                assert_eq!(
                    error.data.as_ref().unwrap().reason.as_deref(),
                    Some("protocol_version_incompatible")
                );
            }
            HandshakeHelloResponse::Success { .. } => panic!("esperava Error"),
        }
        let re_encoded = serde_json::to_string(&parsed).unwrap();
        assert_eq!(re_encoded, raw);
    }

    #[test]
    fn action_target_serializes_type_field_without_raw_prefix() {
        let target = ActionTarget {
            r#type: "repo".to_string(),
            id: "/home/dev/projetos/farol".to_string(),
        };
        let json = serde_json::to_string(&target).unwrap();
        assert_eq!(json, r#"{"type":"repo","id":"/home/dev/projetos/farol"}"#);
    }

    #[test]
    fn remote_status_no_remote_is_distinct_from_zero_ahead_behind() {
        let no_remote = serde_json::to_string(&RemoteStatus::NoRemote).unwrap();
        let zero_tracked = serde_json::to_string(&RemoteStatus::Tracked {
            ahead: 0,
            behind: 0,
        })
        .unwrap();
        assert_ne!(no_remote, zero_tracked);
        assert_eq!(no_remote, r#"{"kind":"no_remote"}"#);
        assert_eq!(zero_tracked, r#"{"kind":"tracked","ahead":0,"behind":0}"#);
    }

    #[test]
    fn widget_get_result_round_trips_with_items() {
        let result = WidgetGetResult {
            widget_id: "repo-status".to_string(),
            items: WidgetItems::Git(vec![WidgetItem {
                repo: GitRepository {
                    id: "/home/dev/projetos/farol".to_string(),
                    name: "farol".to_string(),
                    path: "/home/dev/projetos/farol".to_string(),
                    dirty: true,
                    remote_status: RemoteStatus::Tracked {
                        ahead: 1,
                        behind: 0,
                    },
                },
                fetch_action: ActionDeclaration {
                    id: "git.fetch".to_string(),
                    label: "Fetch".to_string(),
                    target: ActionTarget {
                        r#type: "repo".to_string(),
                        id: "/home/dev/projetos/farol".to_string(),
                    },
                    enabled: true,
                    timeout_hint_ms: None,
                },
            }]),
        };

        let json = serde_json::to_string(&result).unwrap();
        let back: WidgetGetResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back, result);
    }

    /// `MonitorStatusItem` — `response_time_ms: None` serializa como `null` explícito (campo
    /// obrigatório e nullable, nunca ausente), e `WidgetGetResult.items` desta vez carrega a
    /// variante `Monitor` (correção C3, `data-model.md` §1.4).
    #[test]
    fn widget_get_result_round_trips_with_monitor_items() {
        let result = WidgetGetResult {
            widget_id: "uptime-kuma-monitors".to_string(),
            items: WidgetItems::Monitor(vec![
                MonitorStatusItem {
                    name: "api_example_com".to_string(),
                    status: MonitorStatus::Up,
                    response_time_ms: Some(42),
                },
                MonitorStatusItem {
                    name: "internal_service".to_string(),
                    status: MonitorStatus::Down,
                    response_time_ms: None,
                },
            ]),
        };

        let json = serde_json::to_string(&result).unwrap();
        assert_eq!(
            json,
            r#"{"widget_id":"uptime-kuma-monitors","items":[{"name":"api_example_com","status":"up","response_time_ms":42},{"name":"internal_service","status":"down","response_time_ms":null}]}"#
        );
        let back: WidgetGetResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back, result);
    }
}

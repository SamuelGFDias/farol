//! Farol Protocol — language-agnostic specification binding for Rust.
//!
//! Este crate é um *binding* Rust da especificação normativa em `protocol/SPEC.md` e dos JSON
//! Schemas em `protocol/schema/v0.1/*.schema.json`. Ele nunca é, por si só, a fonte da verdade do
//! protocolo — toda mudança de protocolo é feita primeiro na especificação e nos schemas; este
//! binding é atualizado depois, para acompanhar (ver `protocol/SPEC.md`, seção introdutória).

/// Codec NDJSON do transporte (`protocol/SPEC.md` §4).
pub mod framing;
/// Tipos de mensagem do protocolo v0.1 (`protocol/SPEC.md` §5–§8, `protocol/schema/v0.1/*.json`).
pub mod messages;
/// `ProtocolVersion` e a regra de compatibilidade de versão (`protocol/SPEC.md` §6.4).
pub mod version;

pub use framing::{decode, encode, FramingError};
pub use messages::{
    ActionDeclaration, ActionInvokeParams, ActionInvokeRequest, ActionInvokeResponse,
    ActionInvokeResult, ActionTarget, CapabilityManifest, ErrorData, ErrorObject, GitRepository,
    HandshakeHello, HandshakeHelloRequest, HandshakeHelloResponse, HandshakeHelloResult,
    RemoteStatus, RequestId, WidgetDeclaration, WidgetGetParams, WidgetGetRequest,
    WidgetGetResponse, WidgetGetResult, WidgetItem, JSONRPC_VERSION, METHOD_ACTION_INVOKE,
    METHOD_HANDSHAKE_HELLO, METHOD_WIDGET_GET,
};
pub use version::{ProtocolVersion, ProtocolVersionParseError};

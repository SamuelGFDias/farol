//! Versionamento do protocolo Farol.
//!
//! Ver `protocol/SPEC.md` §6.4 (Versionamento) e
//! `specs/001-walking-skeleton-git-plugin/research.md` D7 para a decisão e o algoritmo de
//! compatibilidade normativos. Este módulo é um binding Rust dessas regras — não a fonte da
//! verdade sobre elas.

use std::fmt;
use std::str::FromStr;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// Versão do protocolo Farol, campo `protocol_version` do handshake — string no formato
/// `"MAJOR.MINOR"` (ex.: `"0.1"`), conforme `protocol/SPEC.md` §6.4.
///
/// Não existe componente `PATCH` no wire: correção de bug de implementação não é mudança de
/// contrato. Um binding de linguagem específico (este crate, por exemplo) MAY ter seu próprio
/// esquema de versionamento de publicação — isso é versionamento do binding, não do protocolo, e
/// as duas coisas não precisam coincidir.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProtocolVersion {
    /// Incrementa em qualquer mudança incompatível de wire (remoção de campo obrigatório, mudança
    /// de semântica de um campo existente, remoção de um método).
    pub major: u32,
    /// Incrementa em uma adição compatível (novo campo opcional, novo método opcional que um
    /// consumidor mais antigo pode ignorar).
    pub minor: u32,
}

/// Erro de parsing de uma string `"MAJOR.MINOR"` para [`ProtocolVersion`].
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtocolVersionParseError {
    /// A string não tem o formato `"MAJOR.MINOR"` (falta o ponto, tem componentes demais, ou
    /// algum lado está vazio).
    #[error("formato de versão de protocolo inválido: {0:?} (esperado \"MAJOR.MINOR\")")]
    InvalidFormat(String),

    /// Um dos dois componentes não é um inteiro não-negativo válido.
    #[error("componente numérico inválido em versão de protocolo {0:?}: {1}")]
    InvalidNumber(String, #[source] std::num::ParseIntError),
}

impl ProtocolVersion {
    /// Constrói uma `ProtocolVersion` diretamente a partir dos componentes numéricos.
    pub fn new(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }

    /// Avalia se `self` (a versão falada pelo *plugin*) é compatível com `core` (a versão que o
    /// *core* suporta), conforme o algoritmo normativo de `protocol/SPEC.md` §6.4 / D7:
    ///
    /// ```text
    /// se plugin.MAJOR == 0 (série pré-1.0):
    ///     compatível ⟺ plugin.protocol_version == core.protocol_version   # igualdade exata
    /// senão (plugin.MAJOR >= 1):
    ///     compatível ⟺ plugin.MAJOR == core.MAJOR  E  core.MINOR >= plugin.MINOR
    /// ```
    ///
    /// O caso `MAJOR == 0` é um sub-caso mais estrito do mesmo algoritmo, não uma regra separada:
    /// a série `0.x` não carrega garantia de compatibilidade nem entre MINORs (convenção semver
    /// aplicada literalmente), então exige-se igualdade exata de versão enquanto o protocolo
    /// permanecer pré-1.0.
    pub fn is_compatible_with(&self, core: &ProtocolVersion) -> bool {
        if self.major == 0 {
            self == core
        } else {
            self.major == core.major && core.minor >= self.minor
        }
    }
}

impl fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

impl FromStr for ProtocolVersion {
    type Err = ProtocolVersionParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut segments = s.split('.');
        let major_str = segments.next().filter(|p| !p.is_empty());
        let minor_str = segments.next().filter(|p| !p.is_empty());
        let no_extra_segment = segments.next().is_none();

        let (major_str, minor_str) = match (major_str, minor_str, no_extra_segment) {
            (Some(major), Some(minor), true) => (major, minor),
            _ => return Err(ProtocolVersionParseError::InvalidFormat(s.to_string())),
        };

        let major = major_str
            .parse::<u32>()
            .map_err(|e| ProtocolVersionParseError::InvalidNumber(s.to_string(), e))?;
        let minor = minor_str
            .parse::<u32>()
            .map_err(|e| ProtocolVersionParseError::InvalidNumber(s.to_string(), e))?;

        Ok(ProtocolVersion { major, minor })
    }
}

/// Serializa como a string `"MAJOR.MINOR"` — a forma exata usada pelo campo `protocol_version` no
/// wire (`handshake.schema.json`, padrão `^[0-9]+\.[0-9]+$`).
impl Serialize for ProtocolVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ProtocolVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        raw.parse::<ProtocolVersion>().map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_major_minor_string() {
        let v: ProtocolVersion = "0.1".parse().unwrap();
        assert_eq!(v, ProtocolVersion::new(0, 1));
        assert_eq!(v.to_string(), "0.1");
    }

    #[test]
    fn rejects_malformed_strings() {
        assert!("0".parse::<ProtocolVersion>().is_err());
        assert!("0.1.2".parse::<ProtocolVersion>().is_err());
        assert!("a.b".parse::<ProtocolVersion>().is_err());
        assert!(".1".parse::<ProtocolVersion>().is_err());
        assert!("0.".parse::<ProtocolVersion>().is_err());
        assert!("".parse::<ProtocolVersion>().is_err());
    }

    #[test]
    fn serializes_and_deserializes_as_plain_string() {
        let v = ProtocolVersion::new(0, 1);
        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(json, "\"0.1\"");
        let back: ProtocolVersion = serde_json::from_str(&json).unwrap();
        assert_eq!(back, v);
    }

    // --- D7: regime MAJOR == 0 (série pré-1.0) — exige igualdade exata ---

    #[test]
    fn major_zero_requires_exact_equality() {
        let core = ProtocolVersion::new(0, 1);
        assert!(ProtocolVersion::new(0, 1).is_compatible_with(&core));
        // MINOR diferente é incompatível mesmo sendo "menor" que o core.
        assert!(!ProtocolVersion::new(0, 0).is_compatible_with(&core));
        // MINOR maior também é incompatível.
        assert!(!ProtocolVersion::new(0, 2).is_compatible_with(&core));
    }

    // --- D7: regime MAJOR >= 1 — MAJOR igual e core.MINOR >= plugin.MINOR ---

    #[test]
    fn major_at_least_one_uses_general_rule() {
        let core = ProtocolVersion::new(1, 3);

        // plugin fala menos do que o core sabe: compatível.
        assert!(ProtocolVersion::new(1, 0).is_compatible_with(&core));
        assert!(ProtocolVersion::new(1, 3).is_compatible_with(&core));

        // plugin fala mais do que o core conhece: incompatível.
        assert!(!ProtocolVersion::new(1, 4).is_compatible_with(&core));

        // MAJOR diferente é sempre incompatível, independente do MINOR.
        assert!(!ProtocolVersion::new(2, 0).is_compatible_with(&core));
        assert!(!ProtocolVersion::new(0, 1).is_compatible_with(&core));
    }
}

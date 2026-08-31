//! Codec NDJSON (newline-delimited JSON) do transporte do protocolo Farol.
//!
//! Ver `protocol/SPEC.md` §4 (Framing) e
//! `specs/001-walking-skeleton-git-plugin/research.md` D2 para a decisão normativa: cada mensagem
//! JSON-RPC MUST ser serializada como uma única linha de JSON compacto (sem espaços, quebras de
//! linha ou indentação supérfluos dentro da mensagem), terminada por exatamente um `\n` (LF),
//! codificada em UTF-8. Não há cabeçalho de tamanho (`Content-Length`) nem prefixo binário — o
//! `\n` é o único delimitador de mensagem.
//!
//! As funções aqui são genéricas sobre `T: Serialize`/`T: DeserializeOwned` para servir a
//! qualquer tipo de mensagem definido em `crate::messages`.

use serde::de::DeserializeOwned;
use serde::Serialize;
use thiserror::Error;

/// Erros do codec de framing NDJSON.
#[derive(Debug, Error)]
pub enum FramingError {
    /// Falha ao serializar uma mensagem para JSON compacto de linha única.
    #[error("falha ao serializar mensagem NDJSON: {0}")]
    Encode(#[source] serde_json::Error),

    /// Falha ao desserializar uma linha NDJSON como JSON.
    #[error("falha ao desserializar linha NDJSON: {0}")]
    Decode(#[source] serde_json::Error),
}

/// Codifica `message` como uma linha NDJSON pronta para escrita no transporte: JSON compacto
/// (sem pretty-print) terminado por `\n`.
///
/// Conforme `protocol/SPEC.md` §4: um serializador "bonito" (pretty-printed, com quebras de linha
/// internas) é uma violação de framing — ele introduziria bytes `\n` crus dentro do corpo de uma
/// única mensagem. `serde_json::to_string` já produz JSON compacto por padrão; esta função apenas
/// garante o terminador de linha exigido pelo protocolo.
pub fn encode<T: Serialize>(message: &T) -> Result<String, FramingError> {
    let mut line = serde_json::to_string(message).map_err(FramingError::Encode)?;
    line.push('\n');
    Ok(line)
}

/// Desserializa uma linha NDJSON (com ou sem o `\n` final — ambos os formatos são aceitos) para
/// `T`.
///
/// O chamador é responsável por ler uma linha completa do stream (até e incluindo o `\n`) antes
/// de invocar esta função — decodificar uma linha ainda incompleta produziria um erro de parse
/// espúrio (§4).
///
/// Retorna `Ok(None)` para uma linha vazia (sem nenhum byte de conteúdo não-whitespace antes do
/// `\n`) em vez de erro: `protocol/SPEC.md` §4 exige que uma linha vazia MUST ser ignorada
/// silenciosamente por quem lê — nem tratada como mensagem inválida, nem propagada como erro. Uma
/// linha com conteúdo que não é JSON válido continua sendo `Err`.
pub fn decode<T: DeserializeOwned>(line: &str) -> Result<Option<T>, FramingError> {
    let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
    if trimmed.trim().is_empty() {
        return Ok(None);
    }
    serde_json::from_str(trimmed)
        .map(Some)
        .map_err(FramingError::Decode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Sample {
        id: u32,
        name: String,
    }

    #[test]
    fn encode_produces_compact_single_line_terminated_by_lf() {
        let msg = Sample {
            id: 1,
            name: "git-local".to_string(),
        };
        let line = encode(&msg).unwrap();

        assert!(line.ends_with('\n'));
        assert_eq!(line.matches('\n').count(), 1, "exatamente um LF, no final");
        assert!(!line.contains("  "), "sem indentação de pretty-print");
        assert_eq!(line, "{\"id\":1,\"name\":\"git-local\"}\n");
    }

    #[test]
    fn round_trips_encode_then_decode() {
        let original = Sample {
            id: 42,
            name: "repo-status".to_string(),
        };

        let line = encode(&original).unwrap();
        let decoded: Sample = decode::<Sample>(&line).unwrap().expect("linha não vazia");

        assert_eq!(decoded, original);
    }

    #[test]
    fn decode_accepts_line_with_or_without_trailing_newline() {
        let with_lf = "{\"id\":7,\"name\":\"a\"}\n";
        let without_lf = "{\"id\":7,\"name\":\"a\"}";

        let a: Sample = decode(with_lf).unwrap().unwrap();
        let b: Sample = decode(without_lf).unwrap().unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn decode_ignores_blank_lines_without_erroring() {
        let result: Result<Option<Sample>, FramingError> = decode("\n");
        assert_eq!(result.unwrap(), None);

        let result: Result<Option<Sample>, FramingError> = decode("");
        assert_eq!(result.unwrap(), None);
    }

    #[test]
    fn decode_reports_error_for_invalid_json() {
        let result: Result<Option<Sample>, FramingError> = decode("not json at all\n");
        assert!(matches!(result, Err(FramingError::Decode(_))));
    }
}

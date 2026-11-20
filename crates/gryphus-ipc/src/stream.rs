//! Formato binario de la salida de un agente por `tauri::ipc::Channel`.
//!
//! Cada mensaje es `[tag][payload]`, sin JSON (la salida PTY ya son bytes):
//!
//! | tag | significado | payload |
//! |---|---|---|
//! | 0 | Scrollback | bytes |
//! | 1 | Data | bytes |
//! | 2 | Exited con código | `i32` little-endian (`i32::MIN` = sin código) |
//! | 3 | Failed | motivo en UTF-8 |

const SCROLLBACK: u8 = 0;
const DATA: u8 = 1;
const EXITED: u8 = 2;
const FAILED: u8 = 3;
const NO_CODE: i32 = i32::MIN;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamMsg {
    Scrollback(Vec<u8>),
    Data(Vec<u8>),
    Exited(Option<i32>),
    Failed(String),
}

pub fn encode(msg: &StreamMsg) -> Vec<u8> {
    let (tag, payload): (u8, Vec<u8>) = match msg {
        StreamMsg::Scrollback(b) => (SCROLLBACK, b.clone()),
        StreamMsg::Data(b) => (DATA, b.clone()),
        StreamMsg::Exited(code) => (EXITED, code.unwrap_or(NO_CODE).to_le_bytes().to_vec()),
        StreamMsg::Failed(reason) => (FAILED, reason.as_bytes().to_vec()),
    };
    let mut out = Vec::with_capacity(payload.len() + 1);
    out.push(tag);
    out.extend_from_slice(&payload);
    out
}

/// Variante sin copiar para bytes de salida (el caso caliente).
pub fn encode_bytes(tag_data: bool, bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 1);
    out.push(if tag_data { DATA } else { SCROLLBACK });
    out.extend_from_slice(bytes);
    out
}

pub fn decode(bytes: &[u8]) -> Option<StreamMsg> {
    let (&tag, payload) = bytes.split_first()?;
    match tag {
        SCROLLBACK => Some(StreamMsg::Scrollback(payload.to_vec())),
        DATA => Some(StreamMsg::Data(payload.to_vec())),
        EXITED => {
            let code = i32::from_le_bytes(payload.try_into().ok()?);
            Some(StreamMsg::Exited((code != NO_CODE).then_some(code)))
        }
        FAILED => Some(StreamMsg::Failed(
            String::from_utf8_lossy(payload).into_owned(),
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_message() {
        for msg in [
            StreamMsg::Scrollback(b"hola".to_vec()),
            StreamMsg::Data(vec![0, 255, 27]),
            StreamMsg::Data(Vec::new()),
            StreamMsg::Exited(Some(3)),
            StreamMsg::Exited(Some(-1)),
            StreamMsg::Exited(None),
            StreamMsg::Failed("no encontrado ñ".into()),
        ] {
            assert_eq!(decode(&encode(&msg)), Some(msg));
        }
    }

    #[test]
    fn encode_bytes_matches_encode() {
        assert_eq!(
            encode_bytes(true, b"x"),
            encode(&StreamMsg::Data(b"x".to_vec()))
        );
        assert_eq!(
            encode_bytes(false, b"x"),
            encode(&StreamMsg::Scrollback(b"x".to_vec()))
        );
    }

    #[test]
    fn rejects_malformed_input() {
        assert_eq!(decode(&[]), None);
        assert_eq!(decode(&[9, 1]), None);
        assert_eq!(decode(&[EXITED, 1, 2]), None);
    }
}

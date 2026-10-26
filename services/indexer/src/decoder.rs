//! Deterministic decoding of Anchor event payloads emitted by Tidebook.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use thiserror::Error;

use crate::events::RawEventLog;

pub const EVENT_DISCRIMINATOR_BYTES: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedEventEnvelope {
    pub log_index: usize,
    pub payload_base64: String,
    pub discriminator: [u8; EVENT_DISCRIMINATOR_BYTES],
    pub body: Vec<u8>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EventDecodeError {
    #[error("event payload is not valid Base64: {0}")]
    InvalidBase64(String),

    #[error(
        "event payload contains {decoded_len} decoded bytes; \
         at least {EVENT_DISCRIMINATOR_BYTES} are required"
    )]
    MissingDiscriminator { decoded_len: usize },
}

pub fn decode_envelope(raw_event: &RawEventLog) -> Result<DecodedEventEnvelope, EventDecodeError> {
    let decoded = STANDARD
        .decode(&raw_event.payload_base64)
        .map_err(|error| EventDecodeError::InvalidBase64(error.to_string()))?;

    let (discriminator_bytes, body) = decoded.split_at_checked(EVENT_DISCRIMINATOR_BYTES).ok_or(
        EventDecodeError::MissingDiscriminator {
            decoded_len: decoded.len(),
        },
    )?;

    let discriminator = discriminator_bytes
        .try_into()
        .expect("the preceding length check guarantees eight bytes");

    Ok(DecodedEventEnvelope {
        log_index: raw_event.log_index,
        payload_base64: raw_event.payload_base64.clone(),
        discriminator,
        body: body.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_event(decoded: &[u8], log_index: usize) -> RawEventLog {
        RawEventLog {
            log_index,
            payload_base64: STANDARD.encode(decoded),
        }
    }

    #[test]
    fn decodes_discriminator_and_body_while_preserving_source_data() {
        let decoded = [
            1, 2, 3, 4, 5, 6, 7, 8, // discriminator
            9, 10, 11, // event body
        ];
        let raw = raw_event(&decoded, 17);

        let envelope = decode_envelope(&raw).expect("valid payload should decode");

        assert_eq!(envelope.log_index, 17);
        assert_eq!(envelope.payload_base64, raw.payload_base64);
        assert_eq!(envelope.discriminator, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(envelope.body, vec![9, 10, 11]);
    }

    #[test]
    fn accepts_exactly_eight_bytes_with_an_empty_body() {
        let raw = raw_event(&[1, 2, 3, 4, 5, 6, 7, 8], 3);

        let envelope = decode_envelope(&raw).expect("discriminator-only payload is valid");

        assert_eq!(envelope.discriminator, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert!(envelope.body.is_empty());
    }

    #[test]
    fn rejects_invalid_base64() {
        let raw = RawEventLog {
            log_index: 4,
            payload_base64: "not-valid-base64%%%".to_owned(),
        };

        assert!(matches!(
            decode_envelope(&raw),
            Err(EventDecodeError::InvalidBase64(_))
        ));
    }

    #[test]
    fn rejects_payload_shorter_than_discriminator() {
        let raw = raw_event(&[1, 2, 3, 4, 5, 6, 7], 5);

        assert_eq!(
            decode_envelope(&raw),
            Err(EventDecodeError::MissingDiscriminator { decoded_len: 7 })
        );
    }
}

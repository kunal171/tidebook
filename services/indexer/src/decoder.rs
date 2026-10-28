//! Deterministic decoding of Anchor event payloads emitted by Tidebook.
mod types;

use crate::events::RawEventLog;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use borsh::BorshDeserialize;
use thiserror::Error;
pub use types::{MarketClosedEvent, MarketInitializedEvent, MarketStatusChangedEvent};

pub const EVENT_DISCRIMINATOR_BYTES: usize = 8;
const MARKET_INITIALIZED_DISCRIMINATOR: [u8; 8] = [70, 173, 96, 202, 100, 143, 45, 25];
const MARKET_STATUS_CHANGED_DISCRIMINATOR: [u8; 8] = [189, 57, 199, 130, 179, 34, 39, 53];

const MARKET_CLOSED_DISCRIMINATOR: [u8; 8] = [64, 193, 117, 138, 19, 11, 84, 44];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedEventEnvelope {
    pub log_index: usize,
    pub payload_base64: String,
    pub discriminator: [u8; EVENT_DISCRIMINATOR_BYTES],
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedEventLog {
    pub log_index: usize,
    pub payload_base64: String,
    pub event: TidebookEvent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TidebookEvent {
    Market(MarketEvent),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarketEvent {
    Initialized(MarketInitializedEvent),
    StatusChanged(MarketStatusChangedEvent),
    Closed(MarketClosedEvent),
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

    #[error("unknown Tidebook event discriminator: {0:?}")]
    UnknownDiscriminator([u8; EVENT_DISCRIMINATOR_BYTES]),

    #[error("invalid {event_name} body: {message}")]
    InvalidEventBody {
        event_name: &'static str,
        message: String,
    },
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

fn decode_body<T: BorshDeserialize>(
    body: &[u8],
    event_name: &'static str,
) -> Result<T, EventDecodeError> {
    borsh::from_slice(body).map_err(|error| EventDecodeError::InvalidEventBody {
        event_name,
        message: error.to_string(),
    })
}

pub fn decode_event(raw_event: &RawEventLog) -> Result<DecodedEventLog, EventDecodeError> {
    let envelope = decode_envelope(raw_event)?;

    let event = match envelope.discriminator {
        MARKET_INITIALIZED_DISCRIMINATOR => TidebookEvent::Market(MarketEvent::Initialized(
            decode_body(&envelope.body, "MarketInitializedEvent")?,
        )),

        MARKET_STATUS_CHANGED_DISCRIMINATOR => TidebookEvent::Market(MarketEvent::StatusChanged(
            decode_body(&envelope.body, "MarketStatusChangedEvent")?,
        )),

        MARKET_CLOSED_DISCRIMINATOR => TidebookEvent::Market(MarketEvent::Closed(decode_body(
            &envelope.body,
            "MarketClosedEvent",
        )?)),

        discriminator => {
            return Err(EventDecodeError::UnknownDiscriminator(discriminator));
        }
    };

    Ok(DecodedEventLog {
        log_index: envelope.log_index,
        payload_base64: envelope.payload_base64,
        event,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_pubkey::Pubkey;

    fn market_initialized_fixture() -> (Vec<u8>, MarketInitializedEvent) {
        let market = Pubkey::new_from_array([1; 32]);
        let market_fees = Pubkey::new_from_array([2; 32]);
        let authority = Pubkey::new_from_array([3; 32]);
        let base_mint = Pubkey::new_from_array([4; 32]);
        let quote_mint = Pubkey::new_from_array([5; 32]);
        let base_vault = Pubkey::new_from_array([6; 32]);
        let quote_vault = Pubkey::new_from_array([7; 32]);
        let price_tick_size: u64 = 10_000;
        let quantity_lot_size: u64 = 100_000;

        let mut body = Vec::with_capacity(240);

        for pubkey in [
            &market,
            &market_fees,
            &authority,
            &base_mint,
            &quote_mint,
            &base_vault,
            &quote_vault,
        ] {
            body.extend_from_slice(pubkey.as_ref());
        }

        body.extend_from_slice(&price_tick_size.to_le_bytes());
        body.extend_from_slice(&quantity_lot_size.to_le_bytes());

        let expected = MarketInitializedEvent {
            market,
            market_fees,
            authority,
            base_mint,
            quote_mint,
            base_vault,
            quote_vault,
            price_tick_size,
            quantity_lot_size,
        };

        (body, expected)
    }

    fn encoded_market_initialized_event(body: &[u8]) -> Vec<u8> {
        let mut payload = Vec::with_capacity(EVENT_DISCRIMINATOR_BYTES + body.len());

        payload.extend_from_slice(&MARKET_INITIALIZED_DISCRIMINATOR);
        payload.extend_from_slice(body);

        payload
    }

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

    #[test]
    fn decodes_market_initialized_event() {
        let (body, expected) = market_initialized_fixture();
        let payload = encoded_market_initialized_event(&body);
        let raw = raw_event(&payload, 23);

        let decoded = decode_event(&raw).expect("valid MarketInitializedEvent should decode");

        assert_eq!(decoded.log_index, 23);
        assert_eq!(decoded.payload_base64, raw.payload_base64);
        assert_eq!(
            decoded.event,
            TidebookEvent::Market(MarketEvent::Initialized(expected))
        );
    }

    #[test]
    fn rejects_unknown_event_discriminator() {
        let discriminator = [255; EVENT_DISCRIMINATOR_BYTES];
        let raw = raw_event(&discriminator, 8);

        assert_eq!(
            decode_event(&raw),
            Err(EventDecodeError::UnknownDiscriminator(discriminator))
        );
    }

    #[test]
    fn rejects_truncated_market_initialized_body() {
        let (mut body, _) = market_initialized_fixture();
        body.pop();

        let payload = encoded_market_initialized_event(&body);
        let raw = raw_event(&payload, 9);

        assert!(matches!(
            decode_event(&raw),
            Err(EventDecodeError::InvalidEventBody {
                event_name: "MarketInitializedEvent",
                ..
            })
        ));
    }

    #[test]
    fn rejects_trailing_market_initialized_bytes() {
        let (mut body, _) = market_initialized_fixture();
        body.push(0);

        let payload = encoded_market_initialized_event(&body);
        let raw = raw_event(&payload, 10);

        assert!(matches!(
            decode_event(&raw),
            Err(EventDecodeError::InvalidEventBody {
                event_name: "MarketInitializedEvent",
                ..
            })
        ));
    }
}

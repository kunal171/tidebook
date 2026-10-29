//! Deterministic decoding of Anchor event payloads emitted by Tidebook.

mod types;

use crate::events::RawEventLog;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use borsh::BorshDeserialize;
use thiserror::Error;
pub use types::{
    MarketClosedEvent, MarketInitializedEvent, MarketStatusChangedEvent, OrderCanceledEvent,
    OrderPlacedEvent,
};

/// Anchor prefixes every serialized event body with an eight-byte
/// discriminator derived from `sha256("event:<RustTypeName>")`.
pub const EVENT_DISCRIMINATOR_BYTES: usize = 8;

// These constants are copied from the generated Anchor IDL. Keeping routing
// explicit makes an on-chain event rename or schema addition fail closed as an
// unknown discriminator instead of being misclassified as an existing event.
const MARKET_INITIALIZED_DISCRIMINATOR: [u8; 8] = [70, 173, 96, 202, 100, 143, 45, 25];
const MARKET_STATUS_CHANGED_DISCRIMINATOR: [u8; 8] = [189, 57, 199, 130, 179, 34, 39, 53];
const MARKET_CLOSED_DISCRIMINATOR: [u8; 8] = [64, 193, 117, 138, 19, 11, 84, 44];
const ORDER_PLACED_DISCRIMINATOR: [u8; 8] = [245, 198, 202, 247, 110, 231, 254, 156];

const ORDER_CANCELED_DISCRIMINATOR: [u8; 8] = [10, 31, 133, 147, 181, 32, 34, 30];

/// Valid Base64 split into its Anchor discriminator and still-untyped body.
///
/// The raw Base64 and original log position remain attached so later storage
/// can retain auditable source data alongside the typed projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedEventEnvelope {
    pub log_index: usize,
    pub payload_base64: String,
    pub discriminator: [u8; EVENT_DISCRIMINATOR_BYTES],
    pub body: Vec<u8>,
}

/// A successfully identified and Borsh-decoded Tidebook event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedEventLog {
    pub log_index: usize,
    pub payload_base64: String,
    pub event: TidebookEvent,
}

/// Top-level event families mirror protocol domains rather than flattening all
/// event names into one growing enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TidebookEvent {
    Market(MarketEvent),
    Order(OrderEvent),
}

/// Observable market lifecycle transitions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarketEvent {
    Initialized(MarketInitializedEvent),
    StatusChanged(MarketStatusChangedEvent),
    Closed(MarketClosedEvent),
}

/// Observable order lifecycle transitions outside trade execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrderEvent {
    Placed(OrderPlacedEvent),
    Canceled(OrderCanceledEvent),
}

/// Failures are separated by decoding stage so operators can distinguish
/// corrupt transport data, an unsupported schema, and an invalid known event.
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

/// Decodes Base64 and separates the discriminator without interpreting the
/// event body. Exactly eight decoded bytes is valid and represents an empty
/// body; the typed decoder decides whether that body is valid for its event.
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

/// Strictly decodes one known event body.
///
/// `borsh::from_slice` requires the entire slice to be consumed. This rejects
/// both truncated payloads and payloads with trailing fields from a schema the
/// current indexer does not yet understand.
fn decode_body<T: BorshDeserialize>(
    body: &[u8],
    event_name: &'static str,
) -> Result<T, EventDecodeError> {
    borsh::from_slice(body).map_err(|error| EventDecodeError::InvalidEventBody {
        event_name,
        message: error.to_string(),
    })
}

/// Routes one extracted program-data log to its exact Tidebook event type.
/// Unknown discriminators are errors so a future checkpoint cannot advance
/// past an event that was never represented in the indexed state.
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
        ORDER_PLACED_DISCRIMINATOR => TidebookEvent::Order(OrderEvent::Placed(decode_body(
            &envelope.body,
            "OrderPlacedEvent",
        )?)),

        ORDER_CANCELED_DISCRIMINATOR => TidebookEvent::Order(OrderEvent::Canceled(decode_body(
            &envelope.body,
            "OrderCanceledEvent",
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
    use crate::decoder::types::{MarketStatus, OrderSide};
    use solana_pubkey::Pubkey;

    // Fixtures are assembled byte by byte instead of serializing the same
    // off-chain structs we decode. This independently checks field order,
    // integer endianness, enum tags, and expected body length.
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

    fn market_closed_fixture() -> (Vec<u8>, MarketClosedEvent) {
        let market = Pubkey::new_from_array([21; 32]);
        let market_fees = Pubkey::new_from_array([22; 32]);
        let authority = Pubkey::new_from_array([23; 32]);
        let base_mint = Pubkey::new_from_array([24; 32]);
        let quote_mint = Pubkey::new_from_array([25; 32]);
        let base_vault = Pubkey::new_from_array([26; 32]);
        let quote_vault = Pubkey::new_from_array([27; 32]);

        let mut body = Vec::with_capacity(224);

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

        let expected = MarketClosedEvent {
            market,
            market_fees,
            authority,
            base_mint,
            quote_mint,
            base_vault,
            quote_vault,
        };

        (body, expected)
    }

    fn market_status_changed_body(status_variant: u8) -> (Vec<u8>, Pubkey, Pubkey) {
        let market = Pubkey::new_from_array([11; 32]);
        let authority = Pubkey::new_from_array([12; 32]);

        let mut body = Vec::with_capacity(65);
        body.extend_from_slice(market.as_ref());
        body.extend_from_slice(authority.as_ref());
        body.push(status_variant);

        (body, market, authority)
    }

    fn encoded_event(discriminator: [u8; EVENT_DISCRIMINATOR_BYTES], body: &[u8]) -> Vec<u8> {
        let mut payload = Vec::with_capacity(EVENT_DISCRIMINATOR_BYTES + body.len());

        payload.extend_from_slice(&discriminator);
        payload.extend_from_slice(body);

        payload
    }

    fn raw_event(decoded: &[u8], log_index: usize) -> RawEventLog {
        RawEventLog {
            log_index,
            payload_base64: STANDARD.encode(decoded),
        }
    }

    fn order_placed_fixture(side_variant: u8, side: OrderSide) -> (Vec<u8>, OrderPlacedEvent) {
        let market = Pubkey::new_from_array([31; 32]);
        let order = Pubkey::new_from_array([32; 32]);
        let owner = Pubkey::new_from_array([33; 32]);
        let price_level = Pubkey::new_from_array([34; 32]);

        let order_id: u64 = 42;
        let price: u64 = 250_000;
        let quantity: u64 = 10_000;
        let locked_collateral: u64 = 2_500_000;

        // 4 Pubkeys + 4 u64 values + one enum byte.
        let mut body = Vec::with_capacity(161);
        body.extend_from_slice(market.as_ref());
        body.extend_from_slice(order.as_ref());
        body.extend_from_slice(&order_id.to_le_bytes());
        body.extend_from_slice(owner.as_ref());
        body.push(side_variant);
        body.extend_from_slice(&price.to_le_bytes());
        body.extend_from_slice(&quantity.to_le_bytes());
        body.extend_from_slice(&locked_collateral.to_le_bytes());
        body.extend_from_slice(price_level.as_ref());

        let expected = OrderPlacedEvent {
            market,
            order,
            order_id,
            owner,
            side,
            price,
            quantity,
            locked_collateral,
            price_level,
        };

        (body, expected)
    }

    fn order_canceled_fixture(price_level_closed: bool) -> (Vec<u8>, OrderCanceledEvent) {
        let market = Pubkey::new_from_array([41; 32]);
        let order = Pubkey::new_from_array([42; 32]);
        let owner = Pubkey::new_from_array([43; 32]);
        let price_level = Pubkey::new_from_array([44; 32]);

        let order_id: u64 = 81;
        let side = OrderSide::Ask;
        let price: u64 = 300_000;
        let canceled_quantity: u64 = 4_000;
        let released_collateral: u64 = 4_000;

        // Placed-event layout plus the final Borsh Boolean.
        let mut body = Vec::with_capacity(162);
        body.extend_from_slice(market.as_ref());
        body.extend_from_slice(order.as_ref());
        body.extend_from_slice(&order_id.to_le_bytes());
        body.extend_from_slice(owner.as_ref());
        body.push(1); // OrderSide::Ask
        body.extend_from_slice(&price.to_le_bytes());
        body.extend_from_slice(&canceled_quantity.to_le_bytes());
        body.extend_from_slice(&released_collateral.to_le_bytes());
        body.extend_from_slice(price_level.as_ref());
        body.push(u8::from(price_level_closed));

        let expected = OrderCanceledEvent {
            market,
            order,
            order_id,
            owner,
            side,
            price,
            canceled_quantity,
            released_collateral,
            price_level,
            price_level_closed,
        };

        (body, expected)
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
        let payload = encoded_event(MARKET_INITIALIZED_DISCRIMINATOR, &body);
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

        let payload = encoded_event(MARKET_INITIALIZED_DISCRIMINATOR, &body);
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

        let payload = encoded_event(MARKET_INITIALIZED_DISCRIMINATOR, &body);
        let raw = raw_event(&payload, 10);

        assert!(matches!(
            decode_event(&raw),
            Err(EventDecodeError::InvalidEventBody {
                event_name: "MarketInitializedEvent",
                ..
            })
        ));
    }

    #[test]
    fn decodes_both_market_status_variants() {
        for (variant, expected_status) in [(0, MarketStatus::Active), (1, MarketStatus::Paused)] {
            let (body, market, authority) = market_status_changed_body(variant);

            let payload = encoded_event(MARKET_STATUS_CHANGED_DISCRIMINATOR, &body);
            let raw = raw_event(&payload, usize::from(variant));

            let decoded = decode_event(&raw).expect("valid MarketStatusChangedEvent should decode");

            assert_eq!(
                decoded.event,
                TidebookEvent::Market(MarketEvent::StatusChanged(MarketStatusChangedEvent {
                    market,
                    authority,
                    status: expected_status,
                },))
            );
        }
    }

    #[test]
    fn rejects_invalid_market_status_variant() {
        let (body, _, _) = market_status_changed_body(2);

        let payload = encoded_event(MARKET_STATUS_CHANGED_DISCRIMINATOR, &body);
        let raw = raw_event(&payload, 30);

        assert!(matches!(
            decode_event(&raw),
            Err(EventDecodeError::InvalidEventBody {
                event_name: "MarketStatusChangedEvent",
                ..
            })
        ));
    }

    #[test]
    fn decodes_market_closed_event() {
        let (body, expected) = market_closed_fixture();

        let payload = encoded_event(MARKET_CLOSED_DISCRIMINATOR, &body);
        let raw = raw_event(&payload, 31);

        let decoded = decode_event(&raw).expect("valid MarketClosedEvent should decode");

        assert_eq!(decoded.log_index, 31);
        assert_eq!(
            decoded.event,
            TidebookEvent::Market(MarketEvent::Closed(expected))
        );
    }

    #[test]
    fn decodes_both_order_placed_sides() {
        for (variant, side) in [(0, OrderSide::Bid), (1, OrderSide::Ask)] {
            let (body, expected) = order_placed_fixture(variant, side);

            let payload = encoded_event(ORDER_PLACED_DISCRIMINATOR, &body);
            let raw = raw_event(&payload, 40 + usize::from(variant));

            let decoded = decode_event(&raw).expect("valid OrderPlacedEvent should decode");

            assert_eq!(
                decoded.event,
                TidebookEvent::Order(OrderEvent::Placed(expected))
            );
        }
    }

    #[test]
    fn rejects_invalid_order_side() {
        let (mut body, _) = order_placed_fixture(0, OrderSide::Bid);

        body[104] = 2;

        let payload = encoded_event(ORDER_PLACED_DISCRIMINATOR, &body);
        let raw = raw_event(&payload, 42);

        assert!(matches!(
            decode_event(&raw),
            Err(EventDecodeError::InvalidEventBody {
                event_name: "OrderPlacedEvent",
                ..
            })
        ));
    }

    #[test]
    fn decodes_both_order_canceled_closure_states() {
        for price_level_closed in [false, true] {
            let (body, expected) = order_canceled_fixture(price_level_closed);

            let payload = encoded_event(ORDER_CANCELED_DISCRIMINATOR, &body);
            let raw = raw_event(&payload, 50 + usize::from(price_level_closed));

            let decoded = decode_event(&raw).expect("valid OrderCanceledEvent should decode");

            assert_eq!(
                decoded.event,
                TidebookEvent::Order(OrderEvent::Canceled(expected))
            );
        }
    }

    #[test]
    fn rejects_invalid_price_level_closed_boolean() {
        let (mut body, _) = order_canceled_fixture(false);

        let final_byte = body.last_mut().expect("fixture contains the Boolean field");

        *final_byte = 2;

        let payload = encoded_event(ORDER_CANCELED_DISCRIMINATOR, &body);
        let raw = raw_event(&payload, 52);

        assert!(matches!(
            decode_event(&raw),
            Err(EventDecodeError::InvalidEventBody {
                event_name: "OrderCanceledEvent",
                ..
            })
        ));
    }
}

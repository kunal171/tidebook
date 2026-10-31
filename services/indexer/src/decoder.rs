//! Deterministic decoding of Anchor event payloads emitted by Tidebook.

mod types;

use crate::events::RawEventLog;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use borsh::BorshDeserialize;
use serde::Serialize;
use thiserror::Error;
pub use types::{
    AdminAddedEvent, AdminRemovedEvent, AdminStatusChangedEvent, DepositEvent, FillEvent,
    MarketClosedEvent, MarketInitializedEvent, MarketStatusChangedEvent, OrderCanceledEvent,
    OrderPlacedEvent, ProtocolFeesWithdrawnEvent, ProtocolInitializedEvent, TakerFeeUpdatedEvent,
    TestFaucetClaimedEvent, TestFaucetClosedEvent, TestFaucetInitializedEvent,
    TraderBalanceInitializedEvent, WithdrawalEvent,
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
const FILL_EVENT_DISCRIMINATOR: [u8; 8] = [13, 89, 41, 228, 105, 178, 45, 112];
const TRADER_BALANCE_INITIALIZED_DISCRIMINATOR: [u8; 8] = [37, 85, 211, 33, 106, 44, 35, 255];
const DEPOSIT_EVENT_DISCRIMINATOR: [u8; 8] = [120, 248, 61, 83, 31, 142, 107, 144];
const WITHDRAWAL_EVENT_DISCRIMINATOR: [u8; 8] = [161, 53, 185, 18, 98, 254, 54, 165];
const PROTOCOL_INITIALIZED_DISCRIMINATOR: [u8; 8] = [225, 159, 165, 209, 78, 191, 125, 73];
const ADMIN_ADDED_DISCRIMINATOR: [u8; 8] = [68, 183, 187, 200, 190, 214, 20, 77];
const ADMIN_STATUS_CHANGED_DISCRIMINATOR: [u8; 8] = [103, 168, 116, 72, 83, 249, 142, 91];
const ADMIN_REMOVED_DISCRIMINATOR: [u8; 8] = [226, 5, 12, 53, 69, 56, 172, 132];
const TAKER_FEE_UPDATED_DISCRIMINATOR: [u8; 8] = [99, 217, 115, 77, 244, 244, 75, 248];
const PROTOCOL_FEES_WITHDRAWN_DISCRIMINATOR: [u8; 8] = [142, 109, 101, 78, 169, 208, 188, 216];
const TEST_FAUCET_INITIALIZED_DISCRIMINATOR: [u8; 8] = [247, 38, 39, 17, 66, 174, 194, 81];
const TEST_FAUCET_CLAIMED_DISCRIMINATOR: [u8; 8] = [149, 47, 203, 239, 92, 155, 247, 96];
const TEST_FAUCET_CLOSED_DISCRIMINATOR: [u8; 8] = [169, 116, 71, 236, 191, 44, 241, 40];

/// Valid Base64 split into its Anchor discriminator and still-untyped body.
///
/// The raw Base64 and original log position remain attached so later storage
/// can retain auditable source data alongside the typed projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DecodedEventEnvelope {
    pub log_index: usize,
    pub payload_base64: String,
    pub discriminator: [u8; EVENT_DISCRIMINATOR_BYTES],
    pub body: Vec<u8>,
}

/// A successfully identified and Borsh-decoded Tidebook event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DecodedEventLog {
    pub log_index: usize,
    pub payload_base64: String,
    pub event: TidebookEvent,
}

/// Top-level event families mirror protocol domains rather than flattening all
/// event names into one growing enum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum TidebookEvent {
    Governance(GovernanceEvent),
    Market(MarketEvent),
    Order(OrderEvent),
    Fill(FillEvent),
    Balance(BalanceEvent),
    Fee(FeeEvent),
    Faucet(FaucetEvent),
}

/// Observable protocol and administrator governance transitions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum GovernanceEvent {
    ProtocolInitialized(ProtocolInitializedEvent),
    AdminAdded(AdminAddedEvent),
    AdminStatusChanged(AdminStatusChangedEvent),
    AdminRemoved(AdminRemovedEvent),
    TakerFeeUpdated(TakerFeeUpdatedEvent),
}

/// Observable market lifecycle transitions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum MarketEvent {
    Initialized(MarketInitializedEvent),
    StatusChanged(MarketStatusChangedEvent),
    Closed(MarketClosedEvent),
}

/// Observable order lifecycle transitions outside trade execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum OrderEvent {
    Placed(OrderPlacedEvent),
    Canceled(OrderCanceledEvent),
}

/// Observable changes to a trader's internal market ledger and token custody.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum BalanceEvent {
    Initialized(TraderBalanceInitializedEvent),
    Deposited(DepositEvent),
    Withdrawn(WithdrawalEvent),
}

/// Observable protocol-treasury transitions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum FeeEvent {
    ProtocolFeesWithdrawn(ProtocolFeesWithdrawnEvent),
}

/// Observable lifecycle and claim operations for devnet test faucets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum FaucetEvent {
    Initialized(TestFaucetInitializedEvent),
    Claimed(TestFaucetClaimedEvent),
    Closed(TestFaucetClosedEvent),
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

        FILL_EVENT_DISCRIMINATOR => TidebookEvent::Fill(decode_body(&envelope.body, "FillEvent")?),

        TRADER_BALANCE_INITIALIZED_DISCRIMINATOR => {
            TidebookEvent::Balance(BalanceEvent::Initialized(decode_body(
                &envelope.body,
                "TraderBalanceInitializedEvent",
            )?))
        }

        DEPOSIT_EVENT_DISCRIMINATOR => TidebookEvent::Balance(BalanceEvent::Deposited(
            decode_body(&envelope.body, "DepositEvent")?,
        )),

        WITHDRAWAL_EVENT_DISCRIMINATOR => TidebookEvent::Balance(BalanceEvent::Withdrawn(
            decode_body(&envelope.body, "WithdrawalEvent")?,
        )),

        PROTOCOL_INITIALIZED_DISCRIMINATOR => {
            TidebookEvent::Governance(GovernanceEvent::ProtocolInitialized(decode_body(
                &envelope.body,
                "ProtocolInitializedEvent",
            )?))
        }

        ADMIN_ADDED_DISCRIMINATOR => TidebookEvent::Governance(GovernanceEvent::AdminAdded(
            decode_body(&envelope.body, "AdminAddedEvent")?,
        )),

        ADMIN_STATUS_CHANGED_DISCRIMINATOR => {
            TidebookEvent::Governance(GovernanceEvent::AdminStatusChanged(decode_body(
                &envelope.body,
                "AdminStatusChangedEvent",
            )?))
        }

        ADMIN_REMOVED_DISCRIMINATOR => TidebookEvent::Governance(GovernanceEvent::AdminRemoved(
            decode_body(&envelope.body, "AdminRemovedEvent")?,
        )),

        TAKER_FEE_UPDATED_DISCRIMINATOR => TidebookEvent::Governance(
            GovernanceEvent::TakerFeeUpdated(decode_body(&envelope.body, "TakerFeeUpdatedEvent")?),
        ),

        PROTOCOL_FEES_WITHDRAWN_DISCRIMINATOR => {
            TidebookEvent::Fee(FeeEvent::ProtocolFeesWithdrawn(decode_body(
                &envelope.body,
                "ProtocolFeesWithdrawnEvent",
            )?))
        }

        TEST_FAUCET_INITIALIZED_DISCRIMINATOR => TidebookEvent::Faucet(FaucetEvent::Initialized(
            decode_body(&envelope.body, "TestFaucetInitializedEvent")?,
        )),

        TEST_FAUCET_CLAIMED_DISCRIMINATOR => TidebookEvent::Faucet(FaucetEvent::Claimed(
            decode_body(&envelope.body, "TestFaucetClaimedEvent")?,
        )),

        TEST_FAUCET_CLOSED_DISCRIMINATOR => TidebookEvent::Faucet(FaucetEvent::Closed(
            decode_body(&envelope.body, "TestFaucetClosedEvent")?,
        )),

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
    use crate::decoder::types::{AdminStatus, MarketStatus, OrderSide};
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

    fn fill_fixture(side_variant: u8, maker_side: OrderSide) -> (Vec<u8>, FillEvent) {
        let market = Pubkey::new_from_array([51; 32]);
        let maker_order = Pubkey::new_from_array([52; 32]);
        let maker = Pubkey::new_from_array([53; 32]);
        let taker = Pubkey::new_from_array([54; 32]);
        let maker_order_id: u64 = 91;
        let execution_price: u64 = 250_000;
        let base_quantity: u64 = 4_000;
        let quote_quantity: u64 = 1_000_000;
        let taker_fee_quote: u64 = 2_500;
        let maker_remaining_quantity: u64 = 6_000;
        let taker_remaining_quantity: u64 = 1_000;

        // 4 Pubkeys + 7 u64 values + one OrderSide enum byte.
        let mut body = Vec::with_capacity(185);
        body.extend_from_slice(market.as_ref());
        body.extend_from_slice(maker_order.as_ref());
        body.extend_from_slice(&maker_order_id.to_le_bytes());
        body.extend_from_slice(maker.as_ref());
        body.extend_from_slice(taker.as_ref());
        body.push(side_variant);
        body.extend_from_slice(&execution_price.to_le_bytes());
        body.extend_from_slice(&base_quantity.to_le_bytes());
        body.extend_from_slice(&quote_quantity.to_le_bytes());
        body.extend_from_slice(&taker_fee_quote.to_le_bytes());
        body.extend_from_slice(&maker_remaining_quantity.to_le_bytes());
        body.extend_from_slice(&taker_remaining_quantity.to_le_bytes());

        let expected = FillEvent {
            market,
            maker_order,
            maker_order_id,
            maker,
            taker,
            maker_side,
            execution_price,
            base_quantity,
            quote_quantity,
            taker_fee_quote,
            maker_remaining_quantity,
            taker_remaining_quantity,
        };

        (body, expected)
    }

    fn trader_balance_initialized_fixture() -> (Vec<u8>, TraderBalanceInitializedEvent) {
        let market = Pubkey::new_from_array([61; 32]);
        let owner = Pubkey::new_from_array([62; 32]);
        let trader_balance = Pubkey::new_from_array([63; 32]);

        let mut body = Vec::with_capacity(96);
        body.extend_from_slice(market.as_ref());
        body.extend_from_slice(owner.as_ref());
        body.extend_from_slice(trader_balance.as_ref());

        let expected = TraderBalanceInitializedEvent {
            market,
            owner,
            trader_balance,
        };

        (body, expected)
    }

    fn deposit_fixture() -> (Vec<u8>, DepositEvent) {
        let market = Pubkey::new_from_array([71; 32]);
        let owner = Pubkey::new_from_array([72; 32]);
        let trader_balance = Pubkey::new_from_array([73; 32]);
        let mint = Pubkey::new_from_array([74; 32]);
        let market_vault = Pubkey::new_from_array([75; 32]);
        let amount: u64 = 25_000;
        let free_balance: u64 = 80_000;

        let mut body = Vec::with_capacity(176);
        for pubkey in [&market, &owner, &trader_balance, &mint, &market_vault] {
            body.extend_from_slice(pubkey.as_ref());
        }
        body.extend_from_slice(&amount.to_le_bytes());
        body.extend_from_slice(&free_balance.to_le_bytes());

        let expected = DepositEvent {
            market,
            owner,
            trader_balance,
            mint,
            market_vault,
            amount,
            free_balance,
        };

        (body, expected)
    }

    fn withdrawal_fixture() -> (Vec<u8>, WithdrawalEvent) {
        let market = Pubkey::new_from_array([81; 32]);
        let owner = Pubkey::new_from_array([82; 32]);
        let trader_balance = Pubkey::new_from_array([83; 32]);
        let mint = Pubkey::new_from_array([84; 32]);
        let market_vault = Pubkey::new_from_array([85; 32]);
        let amount: u64 = 15_000;
        let free_balance: u64 = 40_000;

        let mut body = Vec::with_capacity(176);
        for pubkey in [&market, &owner, &trader_balance, &mint, &market_vault] {
            body.extend_from_slice(pubkey.as_ref());
        }
        body.extend_from_slice(&amount.to_le_bytes());
        body.extend_from_slice(&free_balance.to_le_bytes());

        let expected = WithdrawalEvent {
            market,
            owner,
            trader_balance,
            mint,
            market_vault,
            amount,
            free_balance,
        };

        (body, expected)
    }

    fn protocol_initialized_fixture() -> (Vec<u8>, ProtocolInitializedEvent) {
        let protocol_config = Pubkey::new_from_array([91; 32]);
        let super_admin = Pubkey::new_from_array([92; 32]);
        let admin_record = Pubkey::new_from_array([93; 32]);

        let mut body = Vec::with_capacity(96);
        for pubkey in [&protocol_config, &super_admin, &admin_record] {
            body.extend_from_slice(pubkey.as_ref());
        }

        (
            body,
            ProtocolInitializedEvent {
                protocol_config,
                super_admin,
                admin_record,
            },
        )
    }

    fn admin_added_fixture() -> (Vec<u8>, AdminAddedEvent) {
        let admin_record = Pubkey::new_from_array([94; 32]);
        let authority = Pubkey::new_from_array([95; 32]);
        let added_by = Pubkey::new_from_array([96; 32]);

        let mut body = Vec::with_capacity(96);
        for pubkey in [&admin_record, &authority, &added_by] {
            body.extend_from_slice(pubkey.as_ref());
        }

        (
            body,
            AdminAddedEvent {
                admin_record,
                authority,
                added_by,
            },
        )
    }

    fn admin_status_changed_fixture(
        status_variant: u8,
        status: AdminStatus,
    ) -> (Vec<u8>, AdminStatusChangedEvent) {
        let admin_record = Pubkey::new_from_array([97; 32]);
        let authority = Pubkey::new_from_array([98; 32]);
        let changed_by = Pubkey::new_from_array([99; 32]);

        let mut body = Vec::with_capacity(97);
        for pubkey in [&admin_record, &authority, &changed_by] {
            body.extend_from_slice(pubkey.as_ref());
        }
        body.push(status_variant);

        (
            body,
            AdminStatusChangedEvent {
                admin_record,
                authority,
                changed_by,
                status,
            },
        )
    }

    fn admin_removed_fixture() -> (Vec<u8>, AdminRemovedEvent) {
        let admin_record = Pubkey::new_from_array([100; 32]);
        let authority = Pubkey::new_from_array([101; 32]);
        let removed_by = Pubkey::new_from_array([102; 32]);

        let mut body = Vec::with_capacity(96);
        for pubkey in [&admin_record, &authority, &removed_by] {
            body.extend_from_slice(pubkey.as_ref());
        }

        (
            body,
            AdminRemovedEvent {
                admin_record,
                authority,
                removed_by,
            },
        )
    }

    fn taker_fee_updated_fixture() -> (Vec<u8>, TakerFeeUpdatedEvent) {
        let protocol_config = Pubkey::new_from_array([103; 32]);
        let super_admin = Pubkey::new_from_array([104; 32]);
        let previous_taker_fee_bps: u16 = 15;
        let new_taker_fee_bps: u16 = 25;

        let mut body = Vec::with_capacity(68);
        body.extend_from_slice(protocol_config.as_ref());
        body.extend_from_slice(super_admin.as_ref());
        body.extend_from_slice(&previous_taker_fee_bps.to_le_bytes());
        body.extend_from_slice(&new_taker_fee_bps.to_le_bytes());

        (
            body,
            TakerFeeUpdatedEvent {
                protocol_config,
                super_admin,
                previous_taker_fee_bps,
                new_taker_fee_bps,
            },
        )
    }

    fn protocol_fees_withdrawn_fixture() -> (Vec<u8>, ProtocolFeesWithdrawnEvent) {
        let market = Pubkey::new_from_array([105; 32]);
        let market_fees = Pubkey::new_from_array([106; 32]);
        let super_admin = Pubkey::new_from_array([107; 32]);
        let quote_mint = Pubkey::new_from_array([108; 32]);
        let quote_vault = Pubkey::new_from_array([109; 32]);
        let destination_quote_account = Pubkey::new_from_array([110; 32]);
        let amount: u64 = 50_000;
        let remaining_accrued_quote_fees: u64 = 125_000;

        let mut body = Vec::with_capacity(208);
        for pubkey in [
            &market,
            &market_fees,
            &super_admin,
            &quote_mint,
            &quote_vault,
            &destination_quote_account,
        ] {
            body.extend_from_slice(pubkey.as_ref());
        }
        body.extend_from_slice(&amount.to_le_bytes());
        body.extend_from_slice(&remaining_accrued_quote_fees.to_le_bytes());

        (
            body,
            ProtocolFeesWithdrawnEvent {
                market,
                market_fees,
                super_admin,
                quote_mint,
                quote_vault,
                destination_quote_account,
                amount,
                remaining_accrued_quote_fees,
            },
        )
    }

    fn test_faucet_initialized_fixture() -> (Vec<u8>, TestFaucetInitializedEvent) {
        let market = Pubkey::new_from_array([111; 32]);
        let test_faucet = Pubkey::new_from_array([112; 32]);
        let faucet_authority = Pubkey::new_from_array([113; 32]);
        let base_mint = Pubkey::new_from_array([114; 32]);
        let quote_mint = Pubkey::new_from_array([115; 32]);
        let base_claim_amount: u64 = 10_000;
        let quote_claim_amount: u64 = 20_000;

        let mut body = Vec::with_capacity(176);
        for pubkey in [
            &market,
            &test_faucet,
            &faucet_authority,
            &base_mint,
            &quote_mint,
        ] {
            body.extend_from_slice(pubkey.as_ref());
        }
        body.extend_from_slice(&base_claim_amount.to_le_bytes());
        body.extend_from_slice(&quote_claim_amount.to_le_bytes());

        (
            body,
            TestFaucetInitializedEvent {
                market,
                test_faucet,
                faucet_authority,
                base_mint,
                quote_mint,
                base_claim_amount,
                quote_claim_amount,
            },
        )
    }

    fn test_faucet_claimed_fixture() -> (Vec<u8>, TestFaucetClaimedEvent) {
        let market = Pubkey::new_from_array([116; 32]);
        let test_faucet = Pubkey::new_from_array([117; 32]);
        let claimant = Pubkey::new_from_array([118; 32]);
        let base_amount: u64 = 10_000;
        let quote_amount: u64 = 20_000;

        let mut body = Vec::with_capacity(112);
        for pubkey in [&market, &test_faucet, &claimant] {
            body.extend_from_slice(pubkey.as_ref());
        }
        body.extend_from_slice(&base_amount.to_le_bytes());
        body.extend_from_slice(&quote_amount.to_le_bytes());

        (
            body,
            TestFaucetClaimedEvent {
                market,
                test_faucet,
                claimant,
                base_amount,
                quote_amount,
            },
        )
    }

    fn test_faucet_closed_fixture() -> (Vec<u8>, TestFaucetClosedEvent) {
        let market = Pubkey::new_from_array([119; 32]);
        let test_faucet = Pubkey::new_from_array([120; 32]);
        let authority = Pubkey::new_from_array([121; 32]);
        let base_mint = Pubkey::new_from_array([122; 32]);
        let quote_mint = Pubkey::new_from_array([123; 32]);

        let mut body = Vec::with_capacity(160);
        for pubkey in [&market, &test_faucet, &authority, &base_mint, &quote_mint] {
            body.extend_from_slice(pubkey.as_ref());
        }

        (
            body,
            TestFaucetClosedEvent {
                market,
                test_faucet,
                authority,
                base_mint,
                quote_mint,
            },
        )
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

    #[test]
    fn decodes_fill_event_for_both_maker_sides() {
        for (variant, maker_side) in [(0, OrderSide::Bid), (1, OrderSide::Ask)] {
            let (body, expected) = fill_fixture(variant, maker_side);
            let payload = encoded_event(FILL_EVENT_DISCRIMINATOR, &body);
            let raw = raw_event(&payload, 60 + usize::from(variant));

            let decoded = decode_event(&raw).expect("valid FillEvent should decode");

            assert_eq!(decoded.event, TidebookEvent::Fill(expected));
        }
    }

    #[test]
    fn rejects_invalid_fill_maker_side() {
        let (body, _) = fill_fixture(2, OrderSide::Bid);
        let payload = encoded_event(FILL_EVENT_DISCRIMINATOR, &body);
        let raw = raw_event(&payload, 62);

        assert!(matches!(
            decode_event(&raw),
            Err(EventDecodeError::InvalidEventBody {
                event_name: "FillEvent",
                ..
            })
        ));
    }

    #[test]
    fn rejects_truncated_fill_body() {
        let (mut body, _) = fill_fixture(0, OrderSide::Bid);
        body.pop();

        let payload = encoded_event(FILL_EVENT_DISCRIMINATOR, &body);
        let raw = raw_event(&payload, 63);

        assert!(matches!(
            decode_event(&raw),
            Err(EventDecodeError::InvalidEventBody {
                event_name: "FillEvent",
                ..
            })
        ));
    }

    #[test]
    fn decodes_trader_balance_initialized_event() {
        let (body, expected) = trader_balance_initialized_fixture();
        let payload = encoded_event(TRADER_BALANCE_INITIALIZED_DISCRIMINATOR, &body);
        let raw = raw_event(&payload, 70);

        let decoded =
            decode_event(&raw).expect("valid TraderBalanceInitializedEvent should decode");

        assert_eq!(
            decoded.event,
            TidebookEvent::Balance(BalanceEvent::Initialized(expected))
        );
    }

    #[test]
    fn decodes_deposit_event() {
        let (body, expected) = deposit_fixture();
        let payload = encoded_event(DEPOSIT_EVENT_DISCRIMINATOR, &body);
        let raw = raw_event(&payload, 71);

        let decoded = decode_event(&raw).expect("valid DepositEvent should decode");

        assert_eq!(
            decoded.event,
            TidebookEvent::Balance(BalanceEvent::Deposited(expected))
        );
    }

    #[test]
    fn decodes_withdrawal_event() {
        let (body, expected) = withdrawal_fixture();
        let payload = encoded_event(WITHDRAWAL_EVENT_DISCRIMINATOR, &body);
        let raw = raw_event(&payload, 72);

        let decoded = decode_event(&raw).expect("valid WithdrawalEvent should decode");

        assert_eq!(
            decoded.event,
            TidebookEvent::Balance(BalanceEvent::Withdrawn(expected))
        );
    }

    #[test]
    fn decodes_protocol_initialized_event() {
        let (body, expected) = protocol_initialized_fixture();
        let payload = encoded_event(PROTOCOL_INITIALIZED_DISCRIMINATOR, &body);
        let raw = raw_event(&payload, 80);

        let decoded = decode_event(&raw).expect("valid ProtocolInitializedEvent should decode");

        assert_eq!(
            decoded.event,
            TidebookEvent::Governance(GovernanceEvent::ProtocolInitialized(expected))
        );
    }

    #[test]
    fn decodes_admin_added_event() {
        let (body, expected) = admin_added_fixture();
        let payload = encoded_event(ADMIN_ADDED_DISCRIMINATOR, &body);
        let raw = raw_event(&payload, 81);

        let decoded = decode_event(&raw).expect("valid AdminAddedEvent should decode");

        assert_eq!(
            decoded.event,
            TidebookEvent::Governance(GovernanceEvent::AdminAdded(expected))
        );
    }

    #[test]
    fn decodes_both_admin_status_variants() {
        for (variant, status) in [(0, AdminStatus::Active), (1, AdminStatus::Disabled)] {
            let (body, expected) = admin_status_changed_fixture(variant, status);
            let payload = encoded_event(ADMIN_STATUS_CHANGED_DISCRIMINATOR, &body);
            let raw = raw_event(&payload, 82 + usize::from(variant));

            let decoded = decode_event(&raw).expect("valid AdminStatusChangedEvent should decode");

            assert_eq!(
                decoded.event,
                TidebookEvent::Governance(GovernanceEvent::AdminStatusChanged(expected))
            );
        }
    }

    #[test]
    fn rejects_invalid_admin_status_variant() {
        let (body, _) = admin_status_changed_fixture(2, AdminStatus::Active);
        let payload = encoded_event(ADMIN_STATUS_CHANGED_DISCRIMINATOR, &body);
        let raw = raw_event(&payload, 84);

        assert!(matches!(
            decode_event(&raw),
            Err(EventDecodeError::InvalidEventBody {
                event_name: "AdminStatusChangedEvent",
                ..
            })
        ));
    }

    #[test]
    fn decodes_admin_removed_event() {
        let (body, expected) = admin_removed_fixture();
        let payload = encoded_event(ADMIN_REMOVED_DISCRIMINATOR, &body);
        let raw = raw_event(&payload, 85);

        let decoded = decode_event(&raw).expect("valid AdminRemovedEvent should decode");

        assert_eq!(
            decoded.event,
            TidebookEvent::Governance(GovernanceEvent::AdminRemoved(expected))
        );
    }

    #[test]
    fn decodes_taker_fee_updated_event() {
        let (body, expected) = taker_fee_updated_fixture();
        let payload = encoded_event(TAKER_FEE_UPDATED_DISCRIMINATOR, &body);
        let raw = raw_event(&payload, 86);

        let decoded = decode_event(&raw).expect("valid TakerFeeUpdatedEvent should decode");

        assert_eq!(
            decoded.event,
            TidebookEvent::Governance(GovernanceEvent::TakerFeeUpdated(expected))
        );
    }

    #[test]
    fn decodes_protocol_fees_withdrawn_event() {
        let (body, expected) = protocol_fees_withdrawn_fixture();
        let payload = encoded_event(PROTOCOL_FEES_WITHDRAWN_DISCRIMINATOR, &body);
        let raw = raw_event(&payload, 87);

        let decoded = decode_event(&raw).expect("valid ProtocolFeesWithdrawnEvent should decode");

        assert_eq!(
            decoded.event,
            TidebookEvent::Fee(FeeEvent::ProtocolFeesWithdrawn(expected))
        );
    }

    #[test]
    fn decodes_test_faucet_initialized_event() {
        let (body, expected) = test_faucet_initialized_fixture();
        let payload = encoded_event(TEST_FAUCET_INITIALIZED_DISCRIMINATOR, &body);
        let raw = raw_event(&payload, 88);

        let decoded = decode_event(&raw).expect("valid TestFaucetInitializedEvent should decode");

        assert_eq!(
            decoded.event,
            TidebookEvent::Faucet(FaucetEvent::Initialized(expected))
        );
    }

    #[test]
    fn decodes_test_faucet_claimed_event() {
        let (body, expected) = test_faucet_claimed_fixture();
        let payload = encoded_event(TEST_FAUCET_CLAIMED_DISCRIMINATOR, &body);
        let raw = raw_event(&payload, 89);

        let decoded = decode_event(&raw).expect("valid TestFaucetClaimedEvent should decode");

        assert_eq!(
            decoded.event,
            TidebookEvent::Faucet(FaucetEvent::Claimed(expected))
        );
    }

    #[test]
    fn decodes_test_faucet_closed_event() {
        let (body, expected) = test_faucet_closed_fixture();
        let payload = encoded_event(TEST_FAUCET_CLOSED_DISCRIMINATOR, &body);
        let raw = raw_event(&payload, 90);

        let decoded = decode_event(&raw).expect("valid TestFaucetClosedEvent should decode");

        assert_eq!(
            decoded.event,
            TidebookEvent::Faucet(FaucetEvent::Closed(expected))
        );
    }

    #[test]
    fn rejects_trailing_bytes_for_every_supported_event() {
        let (market_initialized, _) = market_initialized_fixture();
        let (market_status_changed, _, _) = market_status_changed_body(0);
        let (market_closed, _) = market_closed_fixture();
        let (order_placed, _) = order_placed_fixture(0, OrderSide::Bid);
        let (order_canceled, _) = order_canceled_fixture(false);
        let (fill, _) = fill_fixture(0, OrderSide::Bid);
        let (trader_balance_initialized, _) = trader_balance_initialized_fixture();
        let (deposit, _) = deposit_fixture();
        let (withdrawal, _) = withdrawal_fixture();
        let (protocol_initialized, _) = protocol_initialized_fixture();
        let (admin_added, _) = admin_added_fixture();
        let (admin_status_changed, _) = admin_status_changed_fixture(0, AdminStatus::Active);
        let (admin_removed, _) = admin_removed_fixture();
        let (taker_fee_updated, _) = taker_fee_updated_fixture();
        let (protocol_fees_withdrawn, _) = protocol_fees_withdrawn_fixture();
        let (test_faucet_initialized, _) = test_faucet_initialized_fixture();
        let (test_faucet_claimed, _) = test_faucet_claimed_fixture();
        let (test_faucet_closed, _) = test_faucet_closed_fixture();

        let fixtures = [
            (
                MARKET_INITIALIZED_DISCRIMINATOR,
                market_initialized,
                "MarketInitializedEvent",
            ),
            (
                MARKET_STATUS_CHANGED_DISCRIMINATOR,
                market_status_changed,
                "MarketStatusChangedEvent",
            ),
            (
                MARKET_CLOSED_DISCRIMINATOR,
                market_closed,
                "MarketClosedEvent",
            ),
            (ORDER_PLACED_DISCRIMINATOR, order_placed, "OrderPlacedEvent"),
            (
                ORDER_CANCELED_DISCRIMINATOR,
                order_canceled,
                "OrderCanceledEvent",
            ),
            (FILL_EVENT_DISCRIMINATOR, fill, "FillEvent"),
            (
                TRADER_BALANCE_INITIALIZED_DISCRIMINATOR,
                trader_balance_initialized,
                "TraderBalanceInitializedEvent",
            ),
            (DEPOSIT_EVENT_DISCRIMINATOR, deposit, "DepositEvent"),
            (
                WITHDRAWAL_EVENT_DISCRIMINATOR,
                withdrawal,
                "WithdrawalEvent",
            ),
            (
                PROTOCOL_INITIALIZED_DISCRIMINATOR,
                protocol_initialized,
                "ProtocolInitializedEvent",
            ),
            (ADMIN_ADDED_DISCRIMINATOR, admin_added, "AdminAddedEvent"),
            (
                ADMIN_STATUS_CHANGED_DISCRIMINATOR,
                admin_status_changed,
                "AdminStatusChangedEvent",
            ),
            (
                ADMIN_REMOVED_DISCRIMINATOR,
                admin_removed,
                "AdminRemovedEvent",
            ),
            (
                TAKER_FEE_UPDATED_DISCRIMINATOR,
                taker_fee_updated,
                "TakerFeeUpdatedEvent",
            ),
            (
                PROTOCOL_FEES_WITHDRAWN_DISCRIMINATOR,
                protocol_fees_withdrawn,
                "ProtocolFeesWithdrawnEvent",
            ),
            (
                TEST_FAUCET_INITIALIZED_DISCRIMINATOR,
                test_faucet_initialized,
                "TestFaucetInitializedEvent",
            ),
            (
                TEST_FAUCET_CLAIMED_DISCRIMINATOR,
                test_faucet_claimed,
                "TestFaucetClaimedEvent",
            ),
            (
                TEST_FAUCET_CLOSED_DISCRIMINATOR,
                test_faucet_closed,
                "TestFaucetClosedEvent",
            ),
        ];

        for (log_index, (discriminator, mut body, event_name)) in fixtures.into_iter().enumerate() {
            body.push(0);
            let payload = encoded_event(discriminator, &body);
            let raw = raw_event(&payload, 100 + log_index);

            assert!(matches!(
                decode_event(&raw),
                Err(EventDecodeError::InvalidEventBody {
                    event_name: actual_event_name,
                    ..
                }) if actual_event_name == event_name
            ));
        }
    }
}

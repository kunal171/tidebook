//! Typed values prepared for insertion into PostgreSQL.

use sqlx::types::Json;
use thiserror::Error;

use crate::{
    decoder::{DecodedEventLog, TidebookEvent},
    rpc::TransactionRecord,
};

/// A finalized Solana transaction converted to PostgreSQL-compatible types.
#[derive(Debug, Clone)]
pub struct TransactionInsert {
    pub signature: String,
    pub slot: i64,
    pub block_time: Option<i64>,
    pub raw_logs: Json<Vec<String>>,
}

/// One decoded Tidebook event converted to PostgreSQL-compatible types.
#[derive(Debug, Clone)]
pub struct EventInsert {
    pub log_index: i32,
    pub event_type: &'static str,
    pub payload_base64: String,
    pub decoded_event: Json<TidebookEvent>,
}

/// Conversion failures caused by PostgreSQL's signed integer boundaries.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum StorageModelError {
    #[error("Solana slot {slot} exceeds PostgreSQL BIGINT range")]
    SlotOutOfRange { slot: u64 },

    #[error("event log index {log_index} exceeds PostgreSQL INTEGER range")]
    LogIndexOutOfRange { log_index: usize },
}

impl TryFrom<&TransactionRecord> for TransactionInsert {
    type Error = StorageModelError;

    fn try_from(record: &TransactionRecord) -> Result<Self, Self::Error> {
        let slot = i64::try_from(record.slot)
            .map_err(|_| StorageModelError::SlotOutOfRange { slot: record.slot })?;

        Ok(Self {
            signature: record.signature.clone(),
            slot,
            block_time: record.block_time,
            raw_logs: Json(record.logs.clone()),
        })
    }
}

impl TryFrom<&DecodedEventLog> for EventInsert {
    type Error = StorageModelError;

    fn try_from(event: &DecodedEventLog) -> Result<Self, Self::Error> {
        let log_index =
            i32::try_from(event.log_index).map_err(|_| StorageModelError::LogIndexOutOfRange {
                log_index: event.log_index,
            })?;

        Ok(Self {
            log_index,
            event_type: event.event.event_type(),
            payload_base64: event.payload_base64.clone(),
            decoded_event: Json(event.event.clone()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoder::{MarketEvent, MarketInitializedEvent};
    use solana_pubkey::Pubkey;

    fn transaction(slot: u64) -> TransactionRecord {
        TransactionRecord {
            signature: "test-signature".to_owned(),
            slot,
            block_time: Some(1_700_000_000),
            logs: vec!["Program data: payload".to_owned()],
        }
    }

    fn decoded_event(log_index: usize) -> DecodedEventLog {
        DecodedEventLog {
            log_index,
            payload_base64: "payload".to_owned(),
            event: TidebookEvent::Market(MarketEvent::Initialized(MarketInitializedEvent {
                market: Pubkey::new_from_array([1; 32]),
                market_fees: Pubkey::new_from_array([2; 32]),
                authority: Pubkey::new_from_array([3; 32]),
                base_mint: Pubkey::new_from_array([4; 32]),
                quote_mint: Pubkey::new_from_array([5; 32]),
                base_vault: Pubkey::new_from_array([6; 32]),
                quote_vault: Pubkey::new_from_array([7; 32]),
                price_tick_size: 10_000,
                quantity_lot_size: 100_000,
            })),
        }
    }

    #[test]
    fn converts_maximum_postgres_slot_without_truncation() {
        let record = transaction(i64::MAX as u64);

        let insert = TransactionInsert::try_from(&record).expect("i64::MAX must fit BIGINT");

        assert_eq!(insert.slot, i64::MAX);
        assert_eq!(insert.signature, record.signature);
        assert_eq!(insert.raw_logs.0, record.logs);
    }

    #[test]
    fn rejects_slot_above_postgres_bigint_range() {
        let record = transaction((i64::MAX as u64) + 1);

        assert_eq!(
            TransactionInsert::try_from(&record).expect_err("overflowing slot must fail"),
            StorageModelError::SlotOutOfRange { slot: record.slot }
        );
    }

    #[test]
    fn converts_event_with_stable_type_and_json() {
        let event = decoded_event(i32::MAX as usize);

        let insert = EventInsert::try_from(&event).expect("i32::MAX must fit INTEGER");

        assert_eq!(insert.log_index, i32::MAX);
        assert_eq!(insert.event_type, "MarketInitializedEvent");
        assert_eq!(insert.payload_base64, event.payload_base64);
        assert_eq!(insert.decoded_event.0, event.event);
    }

    #[test]
    fn rejects_log_index_above_postgres_integer_range() {
        let event = decoded_event((i32::MAX as usize) + 1);

        assert_eq!(
            EventInsert::try_from(&event).expect_err("overflowing log index must fail"),
            StorageModelError::LogIndexOutOfRange {
                log_index: event.log_index,
            }
        );
    }
}

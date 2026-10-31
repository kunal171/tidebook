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
    /// Base58 identity used as the source table primary key.
    pub signature: String,

    /// Checked signed representation required by PostgreSQL `BIGINT`.
    pub slot: i64,

    /// Checked execution position used for deterministic same-slot ordering.
    pub transaction_index: Option<i32>,

    /// Optional Unix timestamp; Solana RPC may legitimately omit it.
    pub block_time: Option<i64>,

    /// Ordered, lossless transaction logs stored as JSONB.
    pub raw_logs: Json<Vec<String>>,
}

/// One decoded Tidebook event converted to PostgreSQL-compatible types.
#[derive(Debug, Clone)]
pub struct EventInsert {
    /// Checked source-log position and per-transaction event identity.
    pub log_index: i32,

    /// Stable Anchor event name, independent of Rust debug formatting.
    pub event_type: &'static str,

    /// Original Base64 emitted by the program for future re-decoding.
    pub payload_base64: String,

    /// Current typed schema serialized as queryable JSONB.
    pub decoded_event: Json<TidebookEvent>,
}

/// Conversion failures caused by PostgreSQL's signed integer boundaries.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum StorageModelError {
    #[error("Solana slot {slot} exceeds PostgreSQL BIGINT range")]
    SlotExceedsBigInt { slot: u64 },

    #[error("event log index {log_index} exceeds PostgreSQL INTEGER range")]
    LogIndexExceedsInteger { log_index: usize },

    #[error("transaction index {transaction_index} exceeds PostgreSQL INTEGER range")]
    TransactionIndexExceedsInteger { transaction_index: u32 },
}

impl TryFrom<&TransactionRecord> for TransactionInsert {
    type Error = StorageModelError;

    fn try_from(record: &TransactionRecord) -> Result<Self, Self::Error> {
        let slot = i64::try_from(record.slot)
            .map_err(|_| StorageModelError::SlotExceedsBigInt { slot: record.slot })?;

        let transaction_index = record
            .transaction_index
            .map(|index| {
                i32::try_from(index).map_err(|_| {
                    StorageModelError::TransactionIndexExceedsInteger {
                        transaction_index: index,
                    }
                })
            })
            .transpose()?;

        Ok(Self {
            signature: record.signature.clone(),
            slot,
            transaction_index,
            block_time: record.block_time,
            raw_logs: Json(record.logs.clone()),
        })
    }
}

impl TryFrom<&DecodedEventLog> for EventInsert {
    type Error = StorageModelError;

    fn try_from(event: &DecodedEventLog) -> Result<Self, Self::Error> {
        let log_index = i32::try_from(event.log_index).map_err(|_| {
            StorageModelError::LogIndexExceedsInteger {
                log_index: event.log_index,
            }
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
            transaction_index: Some(7),
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
        assert_eq!(insert.transaction_index, Some(7));
        assert_eq!(insert.signature, record.signature);
        assert_eq!(insert.raw_logs.0, record.logs);
    }

    #[test]
    fn rejects_slot_above_postgres_bigint_range() {
        let record = transaction((i64::MAX as u64) + 1);

        assert_eq!(
            TransactionInsert::try_from(&record).expect_err("overflowing slot must fail"),
            StorageModelError::SlotExceedsBigInt { slot: record.slot }
        );
    }

    #[test]
    fn rejects_transaction_index_above_postgres_integer_range() {
        let mut record = transaction(42);
        record.transaction_index = Some((i32::MAX as u32) + 1);

        assert_eq!(
            TransactionInsert::try_from(&record)
                .expect_err("overflowing transaction index must fail"),
            StorageModelError::TransactionIndexExceedsInteger {
                transaction_index: (i32::MAX as u32) + 1,
            }
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
            StorageModelError::LogIndexExceedsInteger {
                log_index: event.log_index,
            }
        );
    }
}

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

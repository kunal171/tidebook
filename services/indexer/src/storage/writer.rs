//! Atomic persistence of finalized transactions and their decoded events.

use sqlx::PgPool;
use thiserror::Error;

use crate::{decoder::DecodedEventLog, rpc::TransactionRecord};

use super::models::{EventInsert, StorageModelError, TransactionInsert};

const SOURCE_SCHEMA_VERSION: i32 = 1;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error(transparent)]
    Model(#[from] StorageModelError),

    #[error(transparent)]
    Database(#[from] sqlx::Error),

    #[error("transaction {signature} already exists with different source data")]
    TransactionConflict { signature: String },

    #[error("event ({signature}, {log_index}) already exists with different source data")]
    EventConflict { signature: String, log_index: i32 },
}

/// Atomically stores one finalized transaction and all its Tidebook events.
///
/// Exact replays succeed idempotently. A conflicting row with the same identity
/// but different source data fails and rolls back the complete database
/// transaction instead of silently accepting inconsistent history.
pub async fn persist_transaction(
    pool: &PgPool,
    transaction: &TransactionRecord,
    events: &[DecodedEventLog],
) -> Result<(), StorageError> {
    // Perform all fallible integer conversions before opening a database
    // transaction, keeping its lifetime as short as possible.
    let transaction_insert = TransactionInsert::try_from(transaction)?;
    let event_inserts = events
        .iter()
        .map(EventInsert::try_from)
        .collect::<Result<Vec<_>, _>>()?;

    let mut database_transaction = pool.begin().await?;

    let transaction_result = sqlx::query(
        r#"
        INSERT INTO chain_transactions (
            signature,
            slot,
            block_time,
            raw_logs,
            schema_version
        )
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (signature) DO UPDATE
        SET signature = EXCLUDED.signature
        WHERE chain_transactions.slot = EXCLUDED.slot
          AND chain_transactions.block_time
              IS NOT DISTINCT FROM EXCLUDED.block_time
          AND chain_transactions.raw_logs = EXCLUDED.raw_logs
          AND chain_transactions.schema_version = EXCLUDED.schema_version
        "#,
    )
    .bind(&transaction_insert.signature)
    .bind(transaction_insert.slot)
    .bind(transaction_insert.block_time)
    .bind(&transaction_insert.raw_logs)
    .bind(SOURCE_SCHEMA_VERSION)
    .execute(&mut *database_transaction)
    .await?;

    if transaction_result.rows_affected() != 1 {
        return Err(StorageError::TransactionConflict {
            signature: transaction_insert.signature,
        });
    }

    for event in event_inserts {
        let event_result = sqlx::query(
            r#"
            INSERT INTO chain_events (
                signature,
                log_index,
                event_type,
                payload_base64,
                decoded_event,
                schema_version
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (signature, log_index) DO UPDATE
            SET signature = EXCLUDED.signature
            WHERE chain_events.event_type = EXCLUDED.event_type
              AND chain_events.payload_base64 = EXCLUDED.payload_base64
              AND chain_events.decoded_event = EXCLUDED.decoded_event
              AND chain_events.schema_version = EXCLUDED.schema_version
            "#,
        )
        .bind(&transaction_insert.signature)
        .bind(event.log_index)
        .bind(event.event_type)
        .bind(&event.payload_base64)
        .bind(&event.decoded_event)
        .bind(SOURCE_SCHEMA_VERSION)
        .execute(&mut *database_transaction)
        .await?;

        if event_result.rows_affected() != 1 {
            return Err(StorageError::EventConflict {
                signature: transaction_insert.signature,
                log_index: event.log_index,
            });
        }
    }

    database_transaction.commit().await?;

    Ok(())
}

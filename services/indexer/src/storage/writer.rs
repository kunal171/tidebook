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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoder::{MarketEvent, MarketInitializedEvent, TidebookEvent};
    use solana_pubkey::Pubkey;

    fn transaction(signature: &str, slot: u64) -> TransactionRecord {
        TransactionRecord {
            signature: signature.to_owned(),
            slot,
            block_time: Some(1_700_000_000),
            logs: vec!["Program data: payload".to_owned()],
        }
    }

    fn event(log_index: usize, payload_base64: &str) -> DecodedEventLog {
        DecodedEventLog {
            log_index,
            payload_base64: payload_base64.to_owned(),
            event: TidebookEvent::Market(MarketEvent::Initialized(MarketInitializedEvent {
                market: Pubkey::new_from_array([11; 32]),
                market_fees: Pubkey::new_from_array([12; 32]),
                authority: Pubkey::new_from_array([13; 32]),
                base_mint: Pubkey::new_from_array([14; 32]),
                quote_mint: Pubkey::new_from_array([15; 32]),
                base_vault: Pubkey::new_from_array([16; 32]),
                quote_vault: Pubkey::new_from_array([17; 32]),
                price_tick_size: 10_000,
                quantity_lot_size: 100_000,
            })),
        }
    }

    #[sqlx::test(migrations = "./migrations")]
    #[ignore = "requires a PostgreSQL server supplied through DATABASE_URL"]
    async fn exact_replay_is_idempotent_and_conflicts_roll_back(pool: PgPool) {
        let source_transaction = transaction("signature-one", 42);
        let source_event = event(7, "payload-one");

        persist_transaction(
            &pool,
            &source_transaction,
            std::slice::from_ref(&source_event),
        )
        .await
        .expect("first ingestion must succeed");
        persist_transaction(
            &pool,
            &source_transaction,
            std::slice::from_ref(&source_event),
        )
        .await
        .expect("exact replay must succeed");

        let transaction_count: i64 = sqlx::query_scalar("SELECT count(*) FROM chain_transactions")
            .fetch_one(&pool)
            .await
            .expect("transaction count query must succeed");
        let event_count: i64 = sqlx::query_scalar("SELECT count(*) FROM chain_events")
            .fetch_one(&pool)
            .await
            .expect("event count query must succeed");

        assert_eq!(transaction_count, 1);
        assert_eq!(event_count, 1);

        let conflicting_transaction = transaction("signature-one", 43);
        assert!(matches!(
            persist_transaction(
                &pool,
                &conflicting_transaction,
                std::slice::from_ref(&source_event),
            )
            .await,
            Err(StorageError::TransactionConflict { .. })
        ));

        let conflicting_event = event(7, "different-payload");
        assert!(matches!(
            persist_transaction(
                &pool,
                &source_transaction,
                std::slice::from_ref(&conflicting_event),
            )
            .await,
            Err(StorageError::EventConflict { .. })
        ));

        let atomic_transaction = transaction("signature-two", 44);
        let duplicate_identity_events = [event(8, "first"), event(8, "second")];
        assert!(matches!(
            persist_transaction(&pool, &atomic_transaction, &duplicate_identity_events).await,
            Err(StorageError::EventConflict { .. })
        ));

        let atomic_transaction_count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM chain_transactions WHERE signature = $1")
                .bind(&atomic_transaction.signature)
                .fetch_one(&pool)
                .await
                .expect("atomic rollback transaction query must succeed");
        let atomic_event_count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM chain_events WHERE signature = $1")
                .bind(&atomic_transaction.signature)
                .fetch_one(&pool)
                .await
                .expect("atomic rollback event query must succeed");

        assert_eq!(atomic_transaction_count, 0);
        assert_eq!(atomic_event_count, 0);
    }
}

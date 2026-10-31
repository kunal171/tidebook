//! Durable forward progress for finalized live ingestion.

use solana_pubkey::Pubkey;
use solana_signature::{ParseSignatureError, Signature};
use sqlx::PgPool;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpdateDecision {
    Keep,
    Replace,
}

/// Newest finalized transaction completely persisted by live synchronization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveCheckpoint {
    /// Signature used as the `until` cursor during reconnect catch-up.
    pub signature: Signature,

    /// Finalized slot used to prevent accidental backward movement.
    pub slot: u64,

    /// Optional execution position for deterministic same-slot ordering.
    pub transaction_index: Option<u32>,
}

/// Failures while loading or decoding durable live progress.
#[derive(Debug, Error)]
pub enum LiveCheckpointError {
    #[error(transparent)]
    Database(#[from] sqlx::Error),

    #[error("stored live checkpoint signature is invalid")]
    InvalidSignature(#[from] ParseSignatureError),

    #[error("stored live checkpoint slot {slot} is negative")]
    NegativeSlot { slot: i64 },

    #[error("stored transaction index {transaction_index} is negative")]
    NegativeTransactionIndex { transaction_index: i32 },

    #[error("live checkpoint slot {slot} exceeds PostgreSQL BIGINT range")]
    SlotOutOfRange { slot: u64 },

    #[error(
        "live checkpoint transaction index {transaction_index} \
        exceeds PostgreSQL INTEGER range"
    )]
    TransactionIndexOutOfRange { transaction_index: u32 },

    #[error(
        "live checkpoint attempted to move backward: \
        stored={stored:?}, incoming={incoming:?}"
    )]
    Backward {
        stored: LiveCheckpoint,
        incoming: LiveCheckpoint,
    },

    #[error(
        "live checkpoint ordering is ambiguous within one slot: \
        stored={stored:?}, incoming={incoming:?}"
    )]
    AmbiguousSameSlot {
        stored: LiveCheckpoint,
        incoming: LiveCheckpoint,
    },

    #[error(
        "live checkpoint position conflicts with existing data: \
        stored={stored:?}, incoming={incoming:?}"
    )]
    PositionConflict {
        stored: LiveCheckpoint,
        incoming: LiveCheckpoint,
    },
}

#[derive(Debug, sqlx::FromRow)]
struct LiveCheckpointRow {
    latest_finalized_signature: String,
    latest_finalized_slot: i64,
    transaction_index: Option<i32>,
}

impl TryFrom<LiveCheckpointRow> for LiveCheckpoint {
    type Error = LiveCheckpointError;

    fn try_from(row: LiveCheckpointRow) -> Result<Self, Self::Error> {
        let signature = row.latest_finalized_signature.parse::<Signature>()?;

        let slot = u64::try_from(row.latest_finalized_slot).map_err(|_| {
            LiveCheckpointError::NegativeSlot {
                slot: row.latest_finalized_slot,
            }
        })?;

        let transaction_index = row
            .transaction_index
            .map(|index| {
                u32::try_from(index).map_err(|_| LiveCheckpointError::NegativeTransactionIndex {
                    transaction_index: index,
                })
            })
            .transpose()?;

        Ok(Self {
            signature,
            slot,
            transaction_index,
        })
    }
}

/// Loads the newest completely persisted live position.
///
/// `None` means live synchronization has not yet established its initial
/// high-water mark.
pub async fn load_live_checkpoint(
    pool: &PgPool,
    program_id: &Pubkey,
) -> Result<Option<LiveCheckpoint>, LiveCheckpointError> {
    let row = sqlx::query_as::<_, LiveCheckpointRow>(
        r#"
        SELECT
            latest_finalized_signature,
            latest_finalized_slot,
            transaction_index
        FROM live_checkpoints
        WHERE program_id = $1
        "#,
    )
    .bind(program_id.to_string())
    .fetch_optional(pool)
    .await?;

    row.map(LiveCheckpoint::try_from).transpose()
}

fn decide_update(
    stored: &LiveCheckpoint,
    incoming: &LiveCheckpoint,
) -> Result<UpdateDecision, LiveCheckpointError> {
    // The same signature identifies one immutable transaction. A newly
    // available transaction index may enrich it, but its position cannot move.
    if stored.signature == incoming.signature {
        let conflicting_index = matches!(
            (stored.transaction_index, incoming.transaction_index),
            (Some(stored_index), Some(incoming_index))
                if stored_index != incoming_index
        );

        if stored.slot != incoming.slot || conflicting_index {
            return Err(LiveCheckpointError::PositionConflict {
                stored: stored.clone(),
                incoming: incoming.clone(),
            });
        }

        return Ok(
            if stored.transaction_index.is_none()
                && incoming.transaction_index.is_some()
            {
                UpdateDecision::Replace
            } else {
                // Exact replay, or a replay missing metadata already stored.
                UpdateDecision::Keep
            },
        );
    }

    match incoming.slot.cmp(&stored.slot) {
        std::cmp::Ordering::Greater => Ok(UpdateDecision::Replace),

        std::cmp::Ordering::Less => {
            Err(LiveCheckpointError::Backward {
                stored: stored.clone(),
                incoming: incoming.clone(),
            })
        }

        std::cmp::Ordering::Equal => {
            match (
                stored.transaction_index,
                incoming.transaction_index,
            ) {
                (Some(stored_index), Some(incoming_index))
                    if incoming_index > stored_index =>
                {
                    Ok(UpdateDecision::Replace)
                }

                (Some(stored_index), Some(incoming_index))
                    if incoming_index < stored_index =>
                {
                    Err(LiveCheckpointError::Backward {
                        stored: stored.clone(),
                        incoming: incoming.clone(),
                    })
                }

                (Some(_), Some(_)) => {
                    Err(LiveCheckpointError::PositionConflict {
                        stored: stored.clone(),
                        incoming: incoming.clone(),
                    })
                }

                _ => Err(LiveCheckpointError::AmbiguousSameSlot {
                    stored: stored.clone(),
                    incoming: incoming.clone(),
                }),
            }
        }
    }
}

/// Inserts or advances the durable live checkpoint without allowing regression.
///
/// A transaction-scoped advisory lock serializes writers even when the program
/// has no checkpoint row yet. A row lock alone cannot protect that initial
/// missing-row case.
pub async fn save_live_checkpoint(
    pool: &PgPool,
    program_id: &Pubkey,
    incoming: &LiveCheckpoint,
) -> Result<(), LiveCheckpointError> {
    // Perform fallible conversions before opening the database transaction.
    let slot = i64::try_from(incoming.slot).map_err(|_| {
        LiveCheckpointError::SlotOutOfRange {
            slot: incoming.slot,
        }
    })?;

    let transaction_index = incoming
        .transaction_index
        .map(|index| {
            i32::try_from(index).map_err(|_| {
                LiveCheckpointError::TransactionIndexOutOfRange {
                    transaction_index: index,
                }
            })
        })
        .transpose()?;

    let program_id = program_id.to_string();
    let signature = incoming.signature.to_string();
    let mut transaction = pool.begin().await?;

    // Hash collisions only serialize unrelated programs; they cannot weaken
    // correctness. The lock lasts until this database transaction finishes.
    sqlx::query(
        "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
    )
    .bind(&program_id)
    .execute(&mut *transaction)
    .await?;

    let stored_row = sqlx::query_as::<_, LiveCheckpointRow>(
        r#"
        SELECT
            latest_finalized_signature,
            latest_finalized_slot,
            transaction_index
        FROM live_checkpoints
        WHERE program_id = $1
        FOR UPDATE
        "#,
    )
    .bind(&program_id)
    .fetch_optional(&mut *transaction)
    .await?;

    match stored_row {
        None => {
            sqlx::query(
                r#"
                INSERT INTO live_checkpoints (
                    program_id,
                    latest_finalized_signature,
                    latest_finalized_slot,
                    transaction_index
                )
                VALUES ($1, $2, $3, $4)
                "#,
            )
            .bind(&program_id)
            .bind(&signature)
            .bind(slot)
            .bind(transaction_index)
            .execute(&mut *transaction)
            .await?;
        }

        Some(row) => {
            let stored = LiveCheckpoint::try_from(row)?;

            if decide_update(&stored, incoming)?
                == UpdateDecision::Replace
            {
                sqlx::query(
                    r#"
                    UPDATE live_checkpoints
                    SET latest_finalized_signature = $2,
                        latest_finalized_slot = $3,
                        transaction_index = $4,
                        updated_at = NOW()
                    WHERE program_id = $1
                    "#,
                )
                .bind(&program_id)
                .bind(&signature)
                .bind(slot)
                .bind(transaction_index)
                .execute(&mut *transaction)
                .await?;
            }
        }
    }

    transaction.commit().await?;

    Ok(())
}

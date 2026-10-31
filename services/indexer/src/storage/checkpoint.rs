//! Durable progress tracking for finalized historical backfills.

use solana_pubkey::Pubkey;
use solana_signature::{ParseSignatureError, Signature};
use sqlx::PgPool;
use thiserror::Error;

/// Last completely persisted position of a program's historical backfill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackfillCheckpoint {
    /// Cursor passed to the next `getSignaturesForAddress` request.
    pub before_signature: Option<Signature>,

    /// Whether RPC has confirmed that no older signatures remain.
    pub completed: bool,
}

#[derive(Debug, Error)]
pub enum CheckpointError {
    #[error(transparent)]
    Database(#[from] sqlx::Error),

    #[error("stored backfill cursor is not a valid Solana signature")]
    InvalidSignature(#[from] ParseSignatureError),
}

#[derive(Debug, sqlx::FromRow)]
struct CheckpointRow {
    before_signature: Option<String>,
    completed: bool,
}

/// Loads existing progress or returns the initial, not-started state.
pub async fn load_backfill_checkpoint(
    pool: &PgPool,
    program_id: &Pubkey,
) -> Result<BackfillCheckpoint, CheckpointError> {
    let row = sqlx::query_as::<_, CheckpointRow>(
        r#"
        SELECT before_signature, completed
        FROM backfill_checkpoints
        WHERE program_id = $1
        "#,
    )
    .bind(program_id.to_string())
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Ok(BackfillCheckpoint {
            before_signature: None,
            completed: false,
        });
    };

    let before_signature = row
        .before_signature
        .map(|signature| signature.parse::<Signature>())
        .transpose()?;

    Ok(BackfillCheckpoint {
        before_signature,
        completed: row.completed,
    })
}

/// Records progress after every transaction in the page has been persisted.
///
/// Calling this before page processing finishes would risk permanently
/// skipping source history. The caller therefore invokes it only after the
/// complete page succeeds.
pub async fn save_backfill_checkpoint(
    pool: &PgPool,
    program_id: &Pubkey,
    before_signature: Option<&Signature>,
    completed: bool,
) -> Result<(), CheckpointError> {
    let before_signature = before_signature.map(ToString::to_string);

    sqlx::query(
        r#"
        INSERT INTO backfill_checkpoints (
            program_id,
            before_signature,
            completed
        )
        VALUES ($1, $2, $3)
        ON CONFLICT (program_id) DO UPDATE
        SET before_signature = EXCLUDED.before_signature,
            completed = EXCLUDED.completed,
            updated_at = NOW()
        "#,
    )
    .bind(program_id.to_string())
    .bind(before_signature)
    .bind(completed)
    .execute(pool)
    .await?;

    Ok(())
}
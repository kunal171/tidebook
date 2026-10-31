//! Durable, bounded historical ingestion orchestration.

use std::error::Error;

use solana_pubkey::Pubkey;
use solana_rpc_client::rpc_client::RpcClient;
use sqlx::PgPool;

use crate::{
    processor::process_signature,
    rpc::fetch_signature_page,
    storage::{load_backfill_checkpoint, save_backfill_checkpoint},
};

/// Operational bounds for one historical ingestion run.
#[derive(Debug, Clone, Copy)]
pub struct BackfillConfig {
    pub signature_page_limit: usize,
    pub max_pages_per_run: usize,
}

/// Resumes finalized history and advances its cursor after complete pages.
///
/// A transaction, decoding, or storage failure returns before the checkpoint
/// update. The next run therefore retries the incomplete page, relying on the
/// source tables' idempotent writes for already committed transactions.
pub async fn run_backfill(
    client: &RpcClient,
    pool: &PgPool,
    program_id: &Pubkey,
    config: BackfillConfig,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let checkpoint = load_backfill_checkpoint(pool, program_id).await?;

    if checkpoint.completed {
        println!("Historical backfill is already complete.");
        return Ok(());
    }

    let mut before_signature = checkpoint.before_signature;

    for page_number in 1..=config.max_pages_per_run {
        let page = fetch_signature_page(
            client,
            program_id,
            before_signature.as_ref(),
            config.signature_page_limit,
        )?;
        let successful_count = page.records.len();
        let next_before = page.next_before;

        println!(
            "Backfill page {page_number}: {successful_count} successful finalized transactions"
        );

        for signature in page.records {
            process_signature(client, pool, program_id, &signature).await?;
        }

        let completed = next_before.is_none();

        // This is deliberately the final mutation for a page. If any record
        // above fails, the stored cursor still points at the previous page.
        save_backfill_checkpoint(pool, program_id, next_before.as_ref(), completed).await?;

        if completed {
            println!("Historical backfill reached the end of available history.");
            return Ok(());
        }

        println!("Backfill page {page_number} committed; checkpoint advanced.");
        before_signature = next_before;
    }

    println!(
        "Stopped after {} pages; rerun the indexer to continue.",
        config.max_pages_per_run
    );

    Ok(())
}

//! Tidebook off-chain indexing service.
//!
//! The binary is intentionally limited to dependency construction. Historical
//! ingestion policy lives in `backfill`, while transaction processing and
//! diagnostic output live in their own modules.

mod backfill;
mod config;
mod decoder;
mod events;
mod live;
mod processor;
mod reporter;
mod rpc;
mod storage;

use std::{error::Error, str::FromStr};

use backfill::{run_backfill, BackfillConfig, BackfillOutcome};
use config::Config;
use solana_commitment_config::CommitmentConfig;
use solana_pubkey::Pubkey;
use solana_rpc_client::rpc_client::RpcClient;
use storage::{connect, load_live_checkpoint, LiveCheckpoint};

// Conservative development defaults bound both RPC pressure per request and
// total work per process invocation. Durable cursors make stopping at the cap
// safe; production configuration can replace these constants later.
const SIGNATURE_PAGE_LIMIT: usize = 20;
const MAX_PAGES_PER_RUN: usize = 10;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let config = Config::from_env()?;
    let program_id = Pubkey::from_str(&config.program_id)?;
    let pool = connect(&config.database_url).await?;

    println!("Tidebook indexer");
    println!("RPC endpoint: {}", config.rpc_url);
    println!("Program: {program_id}");
    println!("PostgreSQL connected; pool size={}", pool.size());

    // Reuse one client for signature and transaction RPC calls so its HTTP
    // connection pool survives for the complete bounded backfill run.
    let client = RpcClient::new_with_commitment(config.rpc_url, CommitmentConfig::finalized());

    let backfill_outcome = run_backfill(
        &client,
        &pool,
        &program_id,
        BackfillConfig {
            signature_page_limit: SIGNATURE_PAGE_LIMIT,
            max_pages_per_run: MAX_PAGES_PER_RUN,
        },
    )
    .await?;

    if backfill_outcome == BackfillOutcome::PageLimitReached {
        println!("Historical backfill remains incomplete; live sync will not start.");
        return Ok(());
    }

    let live_checkpoint: Option<LiveCheckpoint> = load_live_checkpoint(&pool, &program_id).await?;

    match live_checkpoint {
        Some(checkpoint) => {
            println!(
                "Live checkpoint: slot={} signature={} transaction_index={:?}",
                checkpoint.slot, checkpoint.signature, checkpoint.transaction_index,
            );
        }
        None => {
            println!("Live checkpoint is not initialized; startup catch-up must establish it.");
        }
    }

    println!("Historical backfill status: {backfill_outcome:?}");

    Ok(())
}

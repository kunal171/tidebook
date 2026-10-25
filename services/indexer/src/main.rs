//! Tidebook off-chain indexing service.
//!
//! The first milestone backfills finalized Tidebook transactions and decodes
//! their events. Database storage and live subscriptions are intentionally
//! deferred until historical ingestion is correct.

mod config;
mod rpc;
mod events;

use events::extract_program_data;
use std::{error::Error, str::FromStr};
use config::Config;
use rpc::{fetch_recent_successful_signatures, fetch_transaction};
use solana_commitment_config::CommitmentConfig;
use solana_pubkey::Pubkey;
use solana_rpc_client::rpc_client::RpcClient;

const INITIAL_SIGNATURE_LIMIT: usize = 20;

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let config = Config::from_env();
    let program_id = Pubkey::from_str(&config.program_id)?;

    println!("Tidebook indexer");
    println!("RPC endpoint: {}", config.rpc_url);
    println!("Program: {program_id}");

    // Reuse one blocking client for the signature page and every transaction
    // fetch. Besides avoiding repeated client construction, this preserves the
    // HTTP connection pool during a backfill batch.
    let client =
        RpcClient::new_with_commitment(config.rpc_url.clone(), CommitmentConfig::finalized());

    let signatures =
        fetch_recent_successful_signatures(&client, &program_id, INITIAL_SIGNATURE_LIMIT)?;

    println!(
        "Found {} successful finalized transactions:",
        signatures.len()
    );

    for signature in signatures {
        let Some(transaction) = fetch_transaction(&client, &signature)? else {
            println!(
                "Skipped transaction without successful metadata: {}",
                signature.signature
            );
            continue;
        };
        let raw_events = extract_program_data(&transaction.logs, &program_id);

        println!(
            "slot={} signature={} block_time={:?} logs={} tidebook_events={}",
            transaction.slot,
            transaction.signature,
            transaction.block_time,
            transaction.logs.len(),
            raw_events.len(),
        );

        for event in raw_events {
            println!(
                "  event_log_index={} payload={}",
                event.log_index,
                event.payload_base64,
            );
        }

    }

    Ok(())
}

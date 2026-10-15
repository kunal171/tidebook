//! Tidebook off-chain indexing service.
//!
//! The first milestone backfills finalized Tidebook transactions and decodes
//! their events. Database storage and live subscriptions are intentionally
//! deferred until historical ingestion is correct.

mod config;
mod rpc;

use std::{error::Error, str::FromStr};

use config::Config;
use rpc::fetch_recent_successful_signatures;
use solana_pubkey::Pubkey;

const INITIAL_SIGNATURE_LIMIT: usize = 20;

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let config = Config::from_env();
    let program_id = Pubkey::from_str(&config.program_id)?;

    println!("Tidebook indexer");
    println!("RPC endpoint: {}", config.rpc_url);
    println!("Program: {program_id}");

    let signatures =
        fetch_recent_successful_signatures(&config.rpc_url, &program_id, INITIAL_SIGNATURE_LIMIT)?;

    println!(
        "Found {} successful finalized transactions:",
        signatures.len()
    );

    for record in signatures {
        println!(
            "slot={} signature={} block_time={:?}",
            record.slot, record.signature, record.block_time,
        );
    }

    Ok(())
}

//! Tidebook off-chain indexing service.
//!
//! The first milestone backfills finalized Tidebook transactions and decodes
//! their events. Database storage and live subscriptions are intentionally
//! deferred until historical ingestion is correct.

mod config;

use config::Config;

fn main() {
    let config = Config::from_env();

    println!("Tidebook indexer");
    println!("RPC endpoint: {}", config.rpc_url);
    println!("Program: {}", config.program_id);
}

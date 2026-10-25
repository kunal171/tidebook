//! Runtime configuration for the Tidebook indexer.

use std::env;

const DEFAULT_RPC_URL: &str = "https://api.devnet.solana.com";
const DEFAULT_PROGRAM_ID: &str = "AG4ztZvcZjzFKjoXLzH8M6U3if9CwVFTXH2FHjqg7SdL";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub rpc_url: String,
    pub program_id: String,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            rpc_url: env::var("SOLANA_RPC_URL").unwrap_or_else(|_| DEFAULT_RPC_URL.to_owned()),
            program_id: env::var("TIDEBOOK_PROGRAM_ID")
                .unwrap_or_else(|_| DEFAULT_PROGRAM_ID.to_owned()),
        }
    }
}

//! Runtime configuration for the Tidebook indexer.

use std::env;
use thiserror::Error;

const DEFAULT_RPC_URL: &str = "https://api.devnet.solana.com";
const DEFAULT_PROGRAM_ID: &str = "AG4ztZvcZjzFKjoXLzH8M6U3if9CwVFTXH2FHjqg7SdL";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub rpc_url: String,
    pub program_id: String,
    pub database_url: String,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            rpc_url: env::var("SOLANA_RPC_URL").unwrap_or_else(|_| DEFAULT_RPC_URL.to_owned()),
            program_id: env::var("TIDEBOOK_PROGRAM_ID")
                .unwrap_or_else(|_| DEFAULT_PROGRAM_ID.to_owned()),
            database_url: env::var("DATABASE_URL").map_err(ConfigError::DatabaseUrl)?,
        })
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("DATABASE_URL environment variable is required")]
    DatabaseUrl(#[source] env::VarError),
}

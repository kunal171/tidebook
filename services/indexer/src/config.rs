//! Runtime configuration for the Tidebook indexer.

use std::env;
use thiserror::Error;

const DEFAULT_RPC_URL: &str = "https://api.devnet.solana.com";
const DEFAULT_PROGRAM_ID: &str = "AG4ztZvcZjzFKjoXLzH8M6U3if9CwVFTXH2FHjqg7SdL";

/// Environment-derived endpoints and identity required by one indexer run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// HTTP RPC endpoint used for finalized history and transaction retrieval.
    pub rpc_url: String,

    /// Tidebook program whose signatures and owned event logs are indexed.
    pub program_id: String,

    /// PostgreSQL connection string; deliberately has no embedded default.
    pub database_url: String,
}

impl Config {
    /// Loads configuration while keeping safe devnet defaults for public data.
    ///
    /// `DATABASE_URL` remains mandatory because silently choosing a database
    /// could write source history into an unintended local or shared instance.
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            rpc_url: env::var("SOLANA_RPC_URL").unwrap_or_else(|_| DEFAULT_RPC_URL.to_owned()),
            program_id: env::var("TIDEBOOK_PROGRAM_ID")
                .unwrap_or_else(|_| DEFAULT_PROGRAM_ID.to_owned()),
            database_url: env::var("DATABASE_URL").map_err(ConfigError::DatabaseUrl)?,
        })
    }
}

/// Configuration failures that must stop ingestion before any RPC work begins.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("DATABASE_URL environment variable is required")]
    DatabaseUrl(#[source] env::VarError),
}

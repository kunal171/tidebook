use anchor_lang::prelude::*;

#[error_code]
pub enum MarketError {
    #[msg("Market is paused")]
    MarketPaused,

    #[msg("Invalid market status")]
    InvalidMarketStatus,

    #[msg("Market already initialized")]
    MarketAlreadyInitialized,

    #[msg("Market not found")]
    MarketNotFound,
}
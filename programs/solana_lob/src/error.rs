use anchor_lang::prelude::*;

//Errors
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

    #[msg("Market is already paused")]
    MarketAlreadyPaused,

    #[msg("Market is already active")]
    MarketAlreadyActive,

    #[msg("Only the market authority can perform this action")]
    Unauthorized,

    #[msg("Market must be paused before it can be closed")]
    MarketMustBePaused,
}

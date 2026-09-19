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

    #[msg("Market is not active")]
    MarketNotActive,

    #[msg("Invalid order ID")]
    InvalidOrderId,

    #[msg("Price must be greater than zero")]
    InvalidPrice,

    #[msg("Quantity must be greater than zero")]
    InvalidQuantity,

    #[msg("Base and quote mints must be different")]
    IdenticalMints,

    #[msg("Signer is not a protocol administrator")]
    UnauthorizedAdmin,

    #[msg("Signer is not the program upgrade authority")]
    InvalidDeployer,

    #[msg("Only the protocol super admin can perform this action")]
    UnauthorizedSuperAdmin,

    #[msg("Invalid administrator address")]
    InvalidAdmin,

    #[msg("Administrator is already disabled")]
    AdminAlreadyDisabled,

    #[msg("Administrator is already active")]
    AdminAlreadyActive,

    #[msg("The super administrator cannot be disabled or removed")]
    CannotModifySuperAdmin,

    #[msg("Administrator must be disabled before removal")]
    AdminMustBeDisabled,

    #[msg("Administrator is disabled")]
    AdminDisabled,
}

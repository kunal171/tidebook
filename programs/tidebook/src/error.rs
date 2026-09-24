//! Program-specific errors for authorization, lifecycle, and order invariants.

use anchor_lang::prelude::*;

/// Stable program errors returned when an authorization, lifecycle, or order
/// invariant is violated.
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

    #[msg("Only the order owner can cancel this order")]
    UnauthorizedOrderOwner,

    #[msg("Order does not belong to the supplied market")]
    OrderMarketMismatch,

    #[msg("Only an open order can be canceled")]
    OrderNotOpen,

    #[msg("Price tick size must be greater than zero")]
    InvalidPriceTickSize,

    #[msg("Quantity lot size must be greater than zero")]
    InvalidQuantityLotSize,

    #[msg("Price must be a multiple of the market tick size")]
    PriceNotOnTick,

    #[msg("Quantity must be a multiple of the market lot size")]
    QuantityNotOnLot,

    #[msg("Order notional arithmetic overflow")]
    OrderNotionalOverflow,

    #[msg("Order notional is below one quote-mint unit")]
    OrderNotionalTooSmall,

    #[msg("Collateral mint does not match the order side")]
    InvalidCollateralMint,

    #[msg("Trader does not own the collateral token account")]
    InvalidCollateralOwner,

    #[msg("Insufficient collateral balance")]
    InsufficientCollateral,

    #[msg("Open-order counter overflow")]
    OpenOrderCountOverflow,

    #[msg("Open-order counter underflow")]
    OpenOrderCountUnderflow,

    #[msg("Market still contains open orders")]
    MarketHasOpenOrders,

    #[msg("Market vaults must be empty before closing")]
    MarketVaultNotEmpty,

    #[msg("Invalid market vault authority")]
    InvalidVaultAuthority,

    #[msg("Price level belongs to a different market")]
    PriceLevelMarketMismatch,

    #[msg("Price-level side does not match the order side")]
    PriceLevelSideMismatch,

    #[msg("Price-level price does not match the order price")]
    PriceLevelPriceMismatch,

    #[msg("Supplied order is not the current price-level tail")]
    InvalidPriceLevelTail,

    #[msg("Order does not belong to the supplied price level")]
    OrderPriceLevelMismatch,

    #[msg("Price-level order count overflow")]
    PriceLevelOrderCountOverflow,

    #[msg("Price-level quantity overflow")]
    PriceLevelQuantityOverflow,

    #[msg("Order ID counter overflow")]
    OrderIdOverflow,

    #[msg("Invalid combination of better and worse price-level neighbors")]
    InvalidPriceLevelNeighbors,

    #[msg("Supplied level is not the market's current best level")]
    BestPriceLevelMismatch,

    #[msg("New price does not have better priority than the current best price")]
    InvalidPriceLevelOrdering,

    #[msg("Supplied price-level account is not canonical")]
    NoncanonicalPriceLevel,

    #[msg("Supplied FIFO order neighbor does not match the order link")]
    InvalidOrderNeighbor,

    #[msg("Supplied FIFO neighbor does not link back to the canceled order")]
    BrokenOrderQueueLink,

    #[msg("Price-level order count underflow")]
    PriceLevelOrderCountUnderflow,

    #[msg("Price-level remaining quantity underflow")]
    PriceLevelQuantityUnderflow,

    #[msg("Supplied order account is not canonical")]
    NoncanonicalOrder,

    #[msg("Price-level rent recipient does not match the stored rent payer")]
    InvalidPriceLevelRentRecipient,

    #[msg("Final order does not match the price-level queue endpoints")]
    InvalidPriceLevelEndpoints,

    #[msg("Empty price level has a nonzero remaining quantity")]
    InvalidPriceLevelAggregate,

    #[msg("Deposit amount must be greater than zero")]
    InvalidDepositAmount,

    #[msg("Deposit mint is neither the market base nor quote mint")]
    InvalidDepositMint,

    #[msg("Trader balance belongs to a different market")]
    TraderBalanceMarketMismatch,

    #[msg("Trader balance belongs to a different owner")]
    TraderBalanceOwnerMismatch,

    #[msg("Free balance overflow")]
    FreeBalanceOverflow,

    #[msg("Locked balance overflow")]
    LockedBalanceOverflow,

    #[msg("Locked balance underflow")]
    LockedBalanceUnderflow,

    #[msg("Insufficient token balance for deposit")]
    InsufficientDepositFunds,

    #[msg("Withdrawal amount must be greater than zero")]
    InvalidWithdrawalAmount,

    #[msg("Withdrawal mint is neither the market base nor quote mint")]
    InvalidWithdrawalMint,

    #[msg("Insufficient free balance")]
    InsufficientFreeBalance,

    #[msg("Withdrawal destination is not owned by the trader")]
    InvalidWithdrawalDestinationOwner,

    #[msg("Market vault contains insufficient tokens")]
    InsufficientVaultFunds,
}

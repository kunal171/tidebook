//! Canonical Anchor error catalog.
//!
//! Anchor 1.2 permits only one `#[error_code]` definition per program IDL.
//! Domain modules re-export subsets of these variants for focused call sites,
//! while this catalog preserves the single public ABI.

use anchor_lang::prelude::*;

/// Stable program errors returned when an authorization, lifecycle, or order
/// invariant is violated.
#[error_code]
pub enum TidebookError {
    #[msg("Market is paused")]
    MarketPaused = 0,

    #[msg("Invalid market status")]
    InvalidMarketStatus = 1,

    #[msg("Market already initialized")]
    MarketAlreadyInitialized = 2,

    #[msg("Market not found")]
    MarketNotFound = 3,

    #[msg("Market is already paused")]
    MarketAlreadyPaused = 4,

    #[msg("Market is already active")]
    MarketAlreadyActive = 5,

    #[msg("Only the market authority can perform this action")]
    Unauthorized = 6,

    #[msg("Market must be paused before it can be closed")]
    MarketMustBePaused = 7,

    #[msg("Market is not active")]
    MarketNotActive = 8,

    #[msg("Invalid order ID")]
    InvalidOrderId = 9,

    #[msg("Price must be greater than zero")]
    InvalidPrice = 10,

    #[msg("Quantity must be greater than zero")]
    InvalidQuantity = 11,

    #[msg("Base and quote mints must be different")]
    IdenticalMints = 12,

    #[msg("Signer is not a protocol administrator")]
    UnauthorizedAdmin = 13,

    #[msg("Signer is not the program upgrade authority")]
    InvalidDeployer = 14,

    #[msg("Only the protocol super admin can perform this action")]
    UnauthorizedSuperAdmin = 15,

    #[msg("Invalid administrator address")]
    InvalidAdmin = 16,

    #[msg("Administrator is already disabled")]
    AdminAlreadyDisabled = 17,

    #[msg("Administrator is already active")]
    AdminAlreadyActive = 18,

    #[msg("The super administrator cannot be disabled or removed")]
    CannotModifySuperAdmin = 19,

    #[msg("Administrator must be disabled before removal")]
    AdminMustBeDisabled = 20,

    #[msg("Administrator is disabled")]
    AdminDisabled = 21,

    #[msg("Only the order owner can cancel this order")]
    UnauthorizedOrderOwner = 22,

    #[msg("Order does not belong to the supplied market")]
    OrderMarketMismatch = 23,

    #[msg("Only an open order can be canceled")]
    OrderNotOpen = 24,

    #[msg("Price tick size must be greater than zero")]
    InvalidPriceTickSize = 25,

    #[msg("Quantity lot size must be greater than zero")]
    InvalidQuantityLotSize = 26,

    #[msg("Price must be a multiple of the market tick size")]
    PriceNotOnTick = 27,

    #[msg("Quantity must be a multiple of the market lot size")]
    QuantityNotOnLot = 28,

    #[msg("Order notional arithmetic overflow")]
    OrderNotionalOverflow = 29,

    #[msg("Order notional is below one quote-mint unit")]
    OrderNotionalTooSmall = 30,

    #[msg("Collateral mint does not match the order side")]
    InvalidCollateralMint = 31,

    #[msg("Trader does not own the collateral token account")]
    InvalidCollateralOwner = 32,

    #[msg("Insufficient collateral balance")]
    InsufficientCollateral = 33,

    #[msg("Open-order counter overflow")]
    OpenOrderCountOverflow = 34,

    #[msg("Open-order counter underflow")]
    OpenOrderCountUnderflow = 35,

    #[msg("Market still contains open orders")]
    MarketHasOpenOrders = 36,

    #[msg("Market vaults must be empty before closing")]
    MarketVaultNotEmpty = 37,

    #[msg("Invalid market vault authority")]
    InvalidVaultAuthority = 38,

    #[msg("Price level belongs to a different market")]
    PriceLevelMarketMismatch = 39,

    #[msg("Price-level side does not match the order side")]
    PriceLevelSideMismatch = 40,

    #[msg("Price-level price does not match the order price")]
    PriceLevelPriceMismatch = 41,

    #[msg("Supplied order is not the current price-level tail")]
    InvalidPriceLevelTail = 42,

    #[msg("Order does not belong to the supplied price level")]
    OrderPriceLevelMismatch = 43,

    #[msg("Price-level order count overflow")]
    PriceLevelOrderCountOverflow = 44,

    #[msg("Price-level quantity overflow")]
    PriceLevelQuantityOverflow = 45,

    #[msg("Order ID counter overflow")]
    OrderIdOverflow = 46,

    #[msg("Invalid combination of better and worse price-level neighbors")]
    InvalidPriceLevelNeighbors = 47,

    #[msg("Supplied level is not the market's current best level")]
    BestPriceLevelMismatch = 48,

    #[msg("New price does not have better priority than the current best price")]
    InvalidPriceLevelOrdering = 49,

    #[msg("Supplied price-level account is not canonical")]
    NoncanonicalPriceLevel = 50,

    #[msg("Supplied FIFO order neighbor does not match the order link")]
    InvalidOrderNeighbor = 51,

    #[msg("Supplied FIFO neighbor does not link back to the canceled order")]
    BrokenOrderQueueLink = 52,

    #[msg("Price-level order count underflow")]
    PriceLevelOrderCountUnderflow = 53,

    #[msg("Price-level remaining quantity underflow")]
    PriceLevelQuantityUnderflow = 54,

    #[msg("Supplied order account is not canonical")]
    NoncanonicalOrder = 55,

    #[msg("Price-level rent recipient does not match the stored rent payer")]
    InvalidPriceLevelRentRecipient = 56,

    #[msg("Final order does not match the price-level queue endpoints")]
    InvalidPriceLevelEndpoints = 57,

    #[msg("Empty price level has a nonzero remaining quantity")]
    InvalidPriceLevelAggregate = 58,

    #[msg("Deposit amount must be greater than zero")]
    InvalidDepositAmount = 59,

    #[msg("Deposit mint is neither the market base nor quote mint")]
    InvalidDepositMint = 60,

    #[msg("Trader balance belongs to a different market")]
    TraderBalanceMarketMismatch = 61,

    #[msg("Trader balance belongs to a different owner")]
    TraderBalanceOwnerMismatch = 62,

    #[msg("Free balance overflow")]
    FreeBalanceOverflow = 63,

    #[msg("Locked balance overflow")]
    LockedBalanceOverflow = 64,

    #[msg("Locked balance underflow")]
    LockedBalanceUnderflow = 65,

    #[msg("Insufficient token balance for deposit")]
    InsufficientDepositFunds = 66,

    #[msg("Withdrawal amount must be greater than zero")]
    InvalidWithdrawalAmount = 67,

    #[msg("Withdrawal mint is neither the market base nor quote mint")]
    InvalidWithdrawalMint = 68,

    #[msg("Insufficient free balance")]
    InsufficientFreeBalance = 69,

    #[msg("Withdrawal destination is not owned by the trader")]
    InvalidWithdrawalDestinationOwner = 70,

    #[msg("Market vault contains insufficient tokens")]
    InsufficientVaultFunds = 71,

    #[msg("Maker and taker orders must be on opposite sides")]
    MatchingSameSide = 72,

    #[msg("Taker limit price does not cross the maker price")]
    OrdersDoNotCross = 73,

    #[msg("Maker order has insufficient locked collateral")]
    InvalidMakerCollateral = 74,

    #[msg("A trader cannot match against their own order")]
    SelfTradeNotAllowed = 75,

    #[msg("Matching must use the best opposing price level")]
    MakerNotAtBestPrice = 76,

    #[msg("Matching must consume the FIFO head order")]
    MakerNotFifoHead = 77,
}

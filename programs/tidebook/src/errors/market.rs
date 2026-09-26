//! Market initialization, lifecycle, vault, and market-counter errors.

pub use super::TidebookError::{
    IdenticalMints, InvalidCollateralMint, InvalidMarketStatus, InvalidPriceTickSize,
    InvalidQuantityLotSize, InvalidVaultAuthority, MarketAlreadyActive, MarketAlreadyInitialized,
    MarketAlreadyPaused, MarketHasOpenOrders, MarketMustBePaused, MarketNotActive, MarketNotFound,
    MarketPaused, MarketVaultNotEmpty, OpenOrderCountOverflow, OpenOrderCountUnderflow,
    Unauthorized,
};

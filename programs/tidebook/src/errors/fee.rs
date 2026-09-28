//! Fee-configuration and fee-calculation errors.

pub use super::TidebookError::{
    FeeCalculationOverflow, FeeRateTooHigh, MarketFeesMarketMismatch, MarketFeesNotEmpty,
    MarketFeesQuoteMintMismatch,
};

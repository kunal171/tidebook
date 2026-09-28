//! Fee-configuration and fee-calculation errors.

pub use super::TidebookError::{
    FeeAccrualOverflow, FeeCalculationOverflow, FeeRateTooHigh, InsufficientAccruedFees,
    InvalidFeeDestinationMint, InvalidFeeWithdrawalAmount, MarketFeesMarketMismatch,
    MarketFeesNotEmpty, MarketFeesQuoteMintMismatch,
};

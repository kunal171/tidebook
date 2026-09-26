//! Matching-price, maker-priority, collateral, and self-trade errors.

pub use super::TidebookError::{
    InvalidMakerCollateral, MakerNotAtBestPrice, MakerNotFifoHead, MatchingSameSide,
    OrdersDoNotCross, SelfTradeNotAllowed,
};

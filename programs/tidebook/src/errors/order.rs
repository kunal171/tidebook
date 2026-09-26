//! Limit-order, collateral, FIFO queue, and price-level invariant errors.

pub use super::TidebookError::{
    BestPriceLevelMismatch, BrokenOrderQueueLink, InsufficientCollateral, InvalidCollateralOwner,
    InvalidOrderId, InvalidOrderNeighbor, InvalidPrice, InvalidPriceLevelAggregate,
    InvalidPriceLevelEndpoints, InvalidPriceLevelNeighbors, InvalidPriceLevelOrdering,
    InvalidPriceLevelRentRecipient, InvalidPriceLevelTail, InvalidQuantity, NoncanonicalOrder,
    NoncanonicalPriceLevel, OrderIdOverflow, OrderMarketMismatch, OrderNotOpen,
    OrderNotionalOverflow, OrderNotionalTooSmall, OrderPriceLevelMismatch,
    PriceLevelMarketMismatch, PriceLevelOrderCountOverflow, PriceLevelOrderCountUnderflow,
    PriceLevelPriceMismatch, PriceLevelQuantityOverflow, PriceLevelQuantityUnderflow,
    PriceLevelSideMismatch, PriceNotOnTick, QuantityNotOnLot, UnauthorizedOrderOwner,
};

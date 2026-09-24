//! Pure deterministic calculations used by the matching engine.
//!
//! This module does not read or modify Solana accounts. Keeping matching math
//! separate lets us verify price crossing, fill quantity, and fixed-point
//! arithmetic before introducing account mutations.

use anchor_lang::prelude::*;

use crate::{error::MarketError, state::OrderSide};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FillCalculation {
    /// Base-mint atoms exchanged.
    pub base_quantity: u64,

    /// Quote-mint atoms exchanged at the maker's price.
    pub quote_quantity: u64,

    /// Resting maker price used for execution.
    pub execution_price: u64,
}

/// Returns whether an incoming taker limit accepts the resting maker price.
///
/// Bids cross asks priced at or below their limit.
/// Asks cross bids priced at or above their limit.
pub fn prices_cross(taker_side: OrderSide, taker_limit_price: u64, maker_price: u64) -> bool {
    match taker_side {
        OrderSide::Bid => taker_limit_price >= maker_price,
        OrderSide::Ask => taker_limit_price <= maker_price,
    }
}

/// Calculates one fill against one resting maker.
///
/// Execution always occurs at the maker's price. The smaller remaining
/// quantity determines whether the maker or taker is completely filled.
pub fn calculate_fill(
    maker_price: u64,
    taker_remaining: u64,
    maker_remaining: u64,
    base_decimals: u8,
) -> Result<FillCalculation> {
    let base_quantity = taker_remaining.min(maker_remaining);

    require!(base_quantity > 0, MarketError::InvalidQuantity);

    let base_scale = 10_u128
        .checked_pow(u32::from(base_decimals))
        .ok_or(MarketError::OrderNotionalOverflow)?;

    let quote_quantity = u128::from(maker_price)
        .checked_mul(u128::from(base_quantity))
        .ok_or(MarketError::OrderNotionalOverflow)?
        .checked_div(base_scale)
        .ok_or(MarketError::OrderNotionalOverflow)?;

    require!(quote_quantity > 0, MarketError::OrderNotionalTooSmall);

    let quote_quantity =
        u64::try_from(quote_quantity).map_err(|_| error!(MarketError::OrderNotionalOverflow))?;

    Ok(FillCalculation {
        base_quantity,
        quote_quantity,
        execution_price: maker_price,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE_DECIMALS: u8 = 6;
    const MAKER_PRICE: u64 = 25_000_000;

    #[test]
    fn bid_crosses_equal_or_lower_ask_price() {
        assert!(prices_cross(OrderSide::Bid, 25, 25));
        assert!(prices_cross(OrderSide::Bid, 25, 24));
        assert!(!prices_cross(OrderSide::Bid, 25, 26));
    }

    #[test]
    fn ask_crosses_equal_or_higher_bid_price() {
        assert!(prices_cross(OrderSide::Ask, 25, 25));
        assert!(prices_cross(OrderSide::Ask, 25, 26));
        assert!(!prices_cross(OrderSide::Ask, 25, 24));
    }

    #[test]
    fn equal_remaining_quantities_fill_both_orders() {
        let fill = calculate_fill(MAKER_PRICE, 2_000_000, 2_000_000, BASE_DECIMALS).unwrap();

        assert_eq!(fill.base_quantity, 2_000_000);
        assert_eq!(fill.quote_quantity, 50_000_000);
        assert_eq!(fill.execution_price, MAKER_PRICE);
    }

    #[test]
    fn smaller_maker_is_fully_filled() {
        let fill = calculate_fill(MAKER_PRICE, 5_000_000, 2_000_000, BASE_DECIMALS).unwrap();

        assert_eq!(fill.base_quantity, 2_000_000);
        assert_eq!(fill.quote_quantity, 50_000_000);
    }

    #[test]
    fn smaller_taker_is_fully_filled_and_maker_remains() {
        let fill = calculate_fill(MAKER_PRICE, 2_000_000, 5_000_000, BASE_DECIMALS).unwrap();

        assert_eq!(fill.base_quantity, 2_000_000);
        assert_eq!(fill.quote_quantity, 50_000_000);
    }

    #[test]
    fn execution_uses_maker_price_not_taker_limit() {
        let taker_limit = 30_000_000;
        assert!(prices_cross(OrderSide::Bid, taker_limit, MAKER_PRICE));

        let fill = calculate_fill(MAKER_PRICE, 1_000_000, 1_000_000, BASE_DECIMALS).unwrap();

        assert_eq!(fill.execution_price, MAKER_PRICE);
        assert_ne!(fill.execution_price, taker_limit);
    }

    #[test]
    fn zero_remaining_quantity_is_rejected() {
        assert!(calculate_fill(MAKER_PRICE, 0, 1_000_000, BASE_DECIMALS).is_err());
        assert!(calculate_fill(MAKER_PRICE, 1_000_000, 0, BASE_DECIMALS).is_err());
    }

    #[test]
    fn fill_that_rounds_to_zero_quote_atoms_is_rejected() {
        assert!(calculate_fill(1, 1, 1, 9).is_err());
    }

    #[test]
    fn decimal_scale_overflow_is_rejected() {
        assert!(calculate_fill(1, 1, 1, 39).is_err());
    }

    #[test]
    fn quote_quantity_larger_than_u64_is_rejected() {
        assert!(calculate_fill(u64::MAX, 2, 2, 0).is_err());
    }
}

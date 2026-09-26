//! Pure deterministic calculations used by the matching engine.
//!
//! This module does not read or modify Solana accounts. Keeping matching math
//! separate lets us verify price crossing, fill quantity, and fixed-point
//! arithmetic before introducing account mutations.

use anchor_lang::prelude::*;

use crate::{
    errors::{matching, order},
    state::OrderSide,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FillCalculation {
    /// Base-mint atoms exchanged.
    pub base_quantity: u64,

    /// Quote-mint atoms exchanged at the maker's price.
    pub quote_quantity: u64,

    /// Resting maker price used for execution.
    pub execution_price: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettlementPlan {
    /// Deterministic base/quote amounts executed at the maker price.
    pub fill: FillCalculation,

    /// Amount removed from the maker's locked balance and order collateral.
    pub maker_locked_debit: u64,

    /// Rounding remainder returned to the maker when a bid is fully filled.
    pub maker_quote_refund: u64,

    pub maker_fully_filled: bool,
    pub taker_fully_filled: bool,
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

    require!(base_quantity > 0, order::InvalidQuantity);

    let base_scale = 10_u128
        .checked_pow(u32::from(base_decimals))
        .ok_or(order::OrderNotionalOverflow)?;

    let quote_quantity = u128::from(maker_price)
        .checked_mul(u128::from(base_quantity))
        .ok_or(order::OrderNotionalOverflow)?
        .checked_div(base_scale)
        .ok_or(order::OrderNotionalOverflow)?;

    require!(quote_quantity > 0, order::OrderNotionalTooSmall);

    let quote_quantity =
        u64::try_from(quote_quantity).map_err(|_| error!(order::OrderNotionalOverflow))?;

    Ok(FillCalculation {
        base_quantity,
        quote_quantity,
        execution_price: maker_price,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn calculate_settlement(
    taker_side: OrderSide,
    taker_limit_price: u64,
    taker_remaining: u64,
    maker_side: OrderSide,
    maker_price: u64,
    maker_remaining: u64,
    maker_locked_collateral: u64,
    base_decimals: u8,
) -> Result<SettlementPlan> {
    // A match must always consume liquidity from the opposite side. Checking
    // this before price crossing avoids interpreting a same-side price as a
    // valid trade.
    require!(taker_side != maker_side, matching::MatchingSameSide);

    require!(
        prices_cross(taker_side, taker_limit_price, maker_price),
        matching::OrdersDoNotCross
    );

    let fill = calculate_fill(maker_price, taker_remaining, maker_remaining, base_decimals)?;

    let maker_fully_filled = fill.base_quantity == maker_remaining;
    let taker_fully_filled = fill.base_quantity == taker_remaining;

    let (maker_locked_debit, maker_quote_refund) = match maker_side {
        OrderSide::Ask => {
            // Ask collateral is denominated directly in base atoms, so it must
            // always equal the remaining order quantity. Unlike bid collateral,
            // no fixed-point rounding dust is possible.
            require!(
                maker_locked_collateral == maker_remaining,
                matching::InvalidMakerCollateral
            );

            (fill.base_quantity, 0)
        }

        OrderSide::Bid => {
            // Partial bid fills consume only their calculated quote amount. On
            // the final fill, consume everything still attached to the order
            // and return any accumulated floor-division dust to quote_free.
            let locked_debit = if maker_fully_filled {
                maker_locked_collateral
            } else {
                fill.quote_quantity
            };

            require!(
                locked_debit >= fill.quote_quantity && maker_locked_collateral >= locked_debit,
                matching::InvalidMakerCollateral
            );

            (locked_debit, locked_debit - fill.quote_quantity)
        }
    };

    Ok(SettlementPlan {
        fill,
        maker_locked_debit,
        maker_quote_refund,
        maker_fully_filled,
        taker_fully_filled,
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

    #[test]
    fn same_side_orders_cannot_settle() {
        let bid_result = calculate_settlement(
            OrderSide::Bid,
            30_000_000,
            1_000_000,
            OrderSide::Bid,
            MAKER_PRICE,
            1_000_000,
            25_000_000,
            BASE_DECIMALS,
        );
        let ask_result = calculate_settlement(
            OrderSide::Ask,
            20_000_000,
            1_000_000,
            OrderSide::Ask,
            MAKER_PRICE,
            1_000_000,
            1_000_000,
            BASE_DECIMALS,
        );

        assert!(bid_result.is_err());
        assert!(ask_result.is_err());
    }

    #[test]
    fn non_crossing_bid_and_ask_cannot_settle() {
        let bid_below_ask = calculate_settlement(
            OrderSide::Bid,
            24_000_000,
            1_000_000,
            OrderSide::Ask,
            MAKER_PRICE,
            1_000_000,
            1_000_000,
            BASE_DECIMALS,
        );
        let ask_above_bid = calculate_settlement(
            OrderSide::Ask,
            26_000_000,
            1_000_000,
            OrderSide::Bid,
            MAKER_PRICE,
            1_000_000,
            25_000_000,
            BASE_DECIMALS,
        );

        assert!(bid_below_ask.is_err());
        assert!(ask_above_bid.is_err());
    }

    #[test]
    fn incoming_bid_partially_fills_maker_ask() {
        let plan = calculate_settlement(
            OrderSide::Bid,
            30_000_000,
            2_000_000,
            OrderSide::Ask,
            MAKER_PRICE,
            5_000_000,
            5_000_000,
            BASE_DECIMALS,
        )
        .unwrap();

        assert_eq!(plan.fill.base_quantity, 2_000_000);
        assert_eq!(plan.fill.quote_quantity, 50_000_000);
        assert_eq!(plan.maker_locked_debit, 2_000_000);
        assert_eq!(plan.maker_quote_refund, 0);
        assert!(!plan.maker_fully_filled);
        assert!(plan.taker_fully_filled);
    }

    #[test]
    fn incoming_ask_partially_fills_maker_bid() {
        let plan = calculate_settlement(
            OrderSide::Ask,
            20_000_000,
            2_000_000,
            OrderSide::Bid,
            MAKER_PRICE,
            5_000_000,
            125_000_000,
            BASE_DECIMALS,
        )
        .unwrap();

        assert_eq!(plan.fill.base_quantity, 2_000_000);
        assert_eq!(plan.fill.quote_quantity, 50_000_000);
        assert_eq!(plan.maker_locked_debit, 50_000_000);
        assert_eq!(plan.maker_quote_refund, 0);
        assert!(!plan.maker_fully_filled);
        assert!(plan.taker_fully_filled);
    }

    #[test]
    fn smaller_maker_sets_only_maker_filled_flag() {
        let plan = calculate_settlement(
            OrderSide::Bid,
            30_000_000,
            5_000_000,
            OrderSide::Ask,
            MAKER_PRICE,
            2_000_000,
            2_000_000,
            BASE_DECIMALS,
        )
        .unwrap();

        assert!(plan.maker_fully_filled);
        assert!(!plan.taker_fully_filled);
    }

    #[test]
    fn equal_quantities_fill_both_maker_and_taker() {
        let plan = calculate_settlement(
            OrderSide::Bid,
            30_000_000,
            2_000_000,
            OrderSide::Ask,
            MAKER_PRICE,
            2_000_000,
            2_000_000,
            BASE_DECIMALS,
        )
        .unwrap();

        assert!(plan.maker_fully_filled);
        assert!(plan.taker_fully_filled);
    }

    #[test]
    fn final_maker_bid_fill_refunds_rounding_dust() {
        // With one base decimal, a price of 15 and quantity of one executes for
        // floor(15 / 10) = 1 quote atom. Two locked atoms model one atom of
        // rounding dust accumulated across earlier partial fills.
        let plan =
            calculate_settlement(OrderSide::Ask, 15, 1, OrderSide::Bid, 15, 1, 2, 1).unwrap();

        assert_eq!(plan.fill.quote_quantity, 1);
        assert_eq!(plan.maker_locked_debit, 2);
        assert_eq!(plan.maker_quote_refund, 1);
        assert!(plan.maker_fully_filled);
        assert!(plan.taker_fully_filled);
    }

    #[test]
    fn partial_maker_bid_requires_enough_locked_quote() {
        let result = calculate_settlement(
            OrderSide::Ask,
            20_000_000,
            2_000_000,
            OrderSide::Bid,
            MAKER_PRICE,
            5_000_000,
            49_999_999,
            BASE_DECIMALS,
        );

        assert!(result.is_err());
    }

    #[test]
    fn maker_ask_collateral_must_equal_remaining_quantity() {
        let insufficient = calculate_settlement(
            OrderSide::Bid,
            30_000_000,
            2_000_000,
            OrderSide::Ask,
            MAKER_PRICE,
            5_000_000,
            4_999_999,
            BASE_DECIMALS,
        );
        let excess = calculate_settlement(
            OrderSide::Bid,
            30_000_000,
            2_000_000,
            OrderSide::Ask,
            MAKER_PRICE,
            5_000_000,
            5_000_001,
            BASE_DECIMALS,
        );

        assert!(insufficient.is_err());
        assert!(excess.is_err());
    }
}

//! Pure protocol-fee calculations.
//!
//! Fees are charged in quote-token atoms to the taker. This module performs
//! no account mutations, allowing rounding and overflow behavior to be tested
//! independently from matching and settlement.

use anchor_lang::prelude::*;

use crate::errors::fee;

/// One basis point is 1 / 10,000.
pub const BASIS_POINTS_DENOMINATOR: u128 = 10_000;

/// Research-phase safety cap: 1,000 bps = 10%.
pub const MAX_TAKER_FEE_BPS: u16 = 1_000;

/// Calculates the quote-denominated fee charged to a taker.
///
/// The result rounds upward. Therefore, when fees are enabled, every nonzero
/// fill pays at least one quote atom. This prevents traders from avoiding fees
/// by splitting trades into many tiny fills.
///
/// Rounding upward slightly favors the protocol for very small fills. We accept
/// that tradeoff because it gives deterministic anti-fragmentation behavior.
pub fn calculate_taker_fee(quote_quantity: u64, fee_bps: u16) -> Result<u64> {
    require!(fee_bps <= MAX_TAKER_FEE_BPS, fee::FeeRateTooHigh);

    if quote_quantity == 0 || fee_bps == 0 {
        return Ok(0);
    }

    let numerator = u128::from(quote_quantity)
        .checked_mul(u128::from(fee_bps))
        .and_then(|value| value.checked_add(BASIS_POINTS_DENOMINATOR - 1))
        .ok_or(fee::FeeCalculationOverflow)?;

    let fee = numerator / BASIS_POINTS_DENOMINATOR;

    u64::try_from(fee).map_err(|_| error!(fee::FeeCalculationOverflow))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_fee_rate_charges_nothing() {
        assert_eq!(calculate_taker_fee(50_000_000, 0).unwrap(), 0);
    }

    #[test]
    fn zero_quote_quantity_charges_nothing() {
        assert_eq!(calculate_taker_fee(0, MAX_TAKER_FEE_BPS).unwrap(), 0);
    }

    #[test]
    fn exact_percentage_fee_is_calculated_in_quote_atoms() {
        // 100 bps is 1%: 1% of 1,000,000 quote atoms is 10,000.
        assert_eq!(calculate_taker_fee(1_000_000, 100).unwrap(), 10_000);
    }

    #[test]
    fn fractional_fee_rounds_up_to_one_quote_atom() {
        // 1 * 1 / 10,000 is below one atom, but nonzero fees use ceiling
        // division so tiny fills cannot avoid the protocol charge.
        assert_eq!(calculate_taker_fee(1, 1).unwrap(), 1);
    }

    #[test]
    fn maximum_fee_rate_is_allowed() {
        assert_eq!(
            calculate_taker_fee(10_000, MAX_TAKER_FEE_BPS).unwrap(),
            1_000
        );
    }

    #[test]
    fn fee_rate_above_maximum_is_rejected() {
        let result = calculate_taker_fee(10_000, MAX_TAKER_FEE_BPS + 1);

        assert!(result.is_err());
    }

    #[test]
    fn largest_quote_quantity_does_not_overflow() {
        // At the 10% cap, ceiling(u64::MAX / 10) remains representable.
        assert_eq!(
            calculate_taker_fee(u64::MAX, MAX_TAKER_FEE_BPS).unwrap(),
            u64::MAX / 10 + 1
        );
    }

    #[test]
    fn fee_never_exceeds_gross_quote_quantity_at_allowed_rates() {
        let quote_quantities = [1, 2, 9, 10, 9_999, 10_000, 1_000_000, u64::MAX];
        let fee_rates = [1, 25, 100, MAX_TAKER_FEE_BPS];

        for quote_quantity in quote_quantities {
            for fee_bps in fee_rates {
                let fee = calculate_taker_fee(quote_quantity, fee_bps).unwrap();

                assert!(fee > 0);
                assert!(fee <= quote_quantity);
            }
        }
    }
}

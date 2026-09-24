//! Canonical PDA derivation helpers shared by program tests and clients.

use anchor_lang::prelude::Pubkey;

use crate::{
    constants::{PRICE_LEVEL_SEED, TRADER_BALANCE_SEED},
    state::OrderSide,
};

/// Derives the unique price-level PDA for one market, side, and price.
///
/// Explicit side bytes and little-endian price encoding are part of the public
/// address contract and must remain identical in Rust tests and web clients.
pub fn derive_price_level_pda(
    program_id: &Pubkey,
    market: &Pubkey,
    side: OrderSide,
    price: u64,
) -> (Pubkey, u8) {
    // Bind the byte array so every seed slice lives through PDA derivation.
    let price_bytes = price.to_le_bytes();

    Pubkey::find_program_address(
        &[
            PRICE_LEVEL_SEED,
            market.as_ref(),
            side.seed(),
            price_bytes.as_ref(),
        ],
        program_id,
    )
}

/// Derives the canonical balance account for one trader in one market.
///
/// Seeds:
/// ["trader_balance", market, owner]
pub fn derive_trader_balance_pda(
    program_id: &Pubkey,
    market: &Pubkey,
    owner: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[TRADER_BALANCE_SEED, market.as_ref(), owner.as_ref()],
        program_id,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_inputs_produce_identical_addresses() {
        let market = Pubkey::new_unique();

        let first = derive_price_level_pda(&crate::ID, &market, OrderSide::Bid, 100);
        let second = derive_price_level_pda(&crate::ID, &market, OrderSide::Bid, 100);

        assert_eq!(first, second);
    }

    #[test]
    fn bid_and_ask_levels_have_different_addresses() {
        let market = Pubkey::new_unique();

        let (bid, _) = derive_price_level_pda(&crate::ID, &market, OrderSide::Bid, 100);
        let (ask, _) = derive_price_level_pda(&crate::ID, &market, OrderSide::Ask, 100);

        assert_ne!(bid, ask);
    }

    #[test]
    fn different_prices_have_different_addresses() {
        let market = Pubkey::new_unique();

        let (first, _) = derive_price_level_pda(&crate::ID, &market, OrderSide::Bid, 100);
        let (second, _) = derive_price_level_pda(&crate::ID, &market, OrderSide::Bid, 110);

        assert_ne!(first, second);
    }

    #[test]
    fn different_markets_have_different_addresses() {
        let first_market = Pubkey::new_unique();
        let second_market = Pubkey::new_unique();

        let (first, _) = derive_price_level_pda(&crate::ID, &first_market, OrderSide::Bid, 100);
        let (second, _) = derive_price_level_pda(&crate::ID, &second_market, OrderSide::Bid, 100);

        assert_ne!(first, second);
    }

    #[test]
    fn trader_balance_pda_is_deterministic() {
        let program_id = Pubkey::new_unique();
        let market = Pubkey::new_unique();
        let owner = Pubkey::new_unique();

        let first = derive_trader_balance_pda(&program_id, &market, &owner);
        let second = derive_trader_balance_pda(&program_id, &market, &owner);

        assert_eq!(first, second);
    }

    #[test]
    fn trader_balance_pda_changes_for_different_owners() {
        let program_id = Pubkey::new_unique();
        let market = Pubkey::new_unique();

        let first_owner = Pubkey::new_unique();
        let second_owner = Pubkey::new_unique();

        let first = derive_trader_balance_pda(&program_id, &market, &first_owner);
        let second = derive_trader_balance_pda(&program_id, &market, &second_owner);

        assert_ne!(first.0, second.0);
    }

    #[test]
    fn trader_balance_pda_changes_for_different_markets() {
        let program_id = Pubkey::new_unique();
        let owner = Pubkey::new_unique();

        let first_market = Pubkey::new_unique();
        let second_market = Pubkey::new_unique();

        let first = derive_trader_balance_pda(&program_id, &first_market, &owner);
        let second = derive_trader_balance_pda(&program_id, &second_market, &owner);

        assert_ne!(first.0, second.0);
    }
}

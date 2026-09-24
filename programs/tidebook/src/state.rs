//! Persistent account layouts and enum state shared across instructions.
//!
//! Any layout change affects already-created accounts and therefore requires an
//! explicit migration or a fresh deployment during the research phase.

use anchor_lang::prelude::*;

/// Configuration and lifecycle state for one ordered base/quote market.
#[account]
#[derive(InitSpace)]
pub struct Market {
    pub authority: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub status: MarketStatus,
    /// Monotonic market-local ID assigned to the next submitted order.
    pub next_order_id: u64,
    pub base_decimals: u8,
    pub quote_decimals: u8,
    /// Minimum price increment in raw quote-price units.
    pub price_tick_size: u64,
    /// Minimum quantity increment in base-mint atoms.
    pub quantity_lot_size: u64,
    /// Reserved for the matching-engine milestone; not maintained yet.
    pub best_bid: Option<u64>,
    /// Reserved for the matching-engine milestone; not maintained yet.
    pub best_ask: Option<u64>,
    /// Number of orders still eligible for matching or cancellation.
    pub open_order_count: u64,
    pub bump: u8,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, InitSpace, Debug)]
pub enum MarketStatus {
    Active,
    Paused,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, InitSpace, Debug)]
pub enum OrderSide {
    Bid,
    Ask,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, InitSpace, Debug)]
pub enum OrderStatus {
    Open,
    Filled,
    Canceled,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, InitSpace, Debug)]
pub enum AdminStatus {
    Active,
    Disabled,
}

#[account]
#[derive(InitSpace)]
/// One order submission and its mutable queue and lifecycle state.
pub struct Order {
    pub owner: Pubkey,
    pub market: Pubkey,
    pub order_id: u64,
    pub side: OrderSide,
    pub price: u64,
    /// Canonical price-level PDA containing this order.
    pub price_level: Pubkey,
    /// Older order at the same price.
    pub previous_order: Option<Pubkey>,
    /// Newer order at the same price.
    pub next_order: Option<Pubkey>,
    pub quantity: u64,
    pub remaining_quantity: u64,
    /// Base atoms for asks or quote atoms for bids currently held in custody.
    pub locked_collateral: u64,
    pub status: OrderStatus,
    pub bump: u8,
}

/// Singleton governance state created by the program upgrade authority.
#[account]
#[derive(InitSpace)]
pub struct ProtocolConfig {
    pub super_admin: Pubkey,
    pub bump: u8,
}

/// Independent role record used to authorize market creation.
#[account]
#[derive(InitSpace)]
pub struct AdminRecord {
    pub authority: Pubkey,
    pub added_by: Pubkey,
    pub status: AdminStatus,
    pub bump: u8,
}

/// One active price in a market-side order-book index.
#[account]
#[derive(InitSpace)]
pub struct PriceLevel {
    /// Market containing this price level.
    pub market: Pubkey,

    /// Bid or ask side.
    pub side: OrderSide,

    /// Tick-aligned price shared by every order in this queue.
    pub price: u64,

    /// Adjacent price with higher matching priority.
    pub better_price: Option<u64>,

    /// Adjacent price with lower matching priority.
    pub worse_price: Option<u64>,

    /// Oldest open order at this price.
    pub first_order: Option<Pubkey>,

    /// Newest open order at this price.
    pub last_order: Option<Pubkey>,

    /// Sum of remaining base quantity across queued orders.
    pub total_remaining_quantity: u64,

    /// Number of open orders at this price.
    pub order_count: u64,

    /// Wallet that receives rent when this level is closed.
    pub rent_payer: Pubkey,

    /// Canonical price-level PDA bump.
    pub bump: u8,
}

/// Canonical internal balance ledger for one trader in one market.
///
/// Tokens represented here physically remain in the market's SPL token vaults.
/// `free` balances can be withdrawn or committed to new orders.
/// `locked` balances collateralize currently open orders.
///
/// There is exactly one account for each `(market, owner)` pair.
#[account]
#[derive(InitSpace)]
pub struct TraderBalance {
    /// Market whose vault assets back these balances.
    pub market: Pubkey,

    /// Wallet that owns this balance record.
    pub owner: Pubkey,

    /// Base tokens available for withdrawal or new ask orders.
    pub base_free: u64,

    /// Base tokens reserved by open ask orders.
    pub base_locked: u64,

    /// Quote tokens available for withdrawal or new bid orders.
    pub quote_free: u64,

    /// Quote tokens reserved by open bid orders.
    pub quote_locked: u64,

    /// PDA bump stored for future signer and validation logic.
    pub bump: u8,
}

impl OrderSide {
    /// Stable PDA seed independent of Rust enum representation.
    pub const fn seed(self) -> &'static [u8] {
        match self {
            Self::Bid => b"bid",
            Self::Ask => b"ask",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_account_size_is_stable() {
        // Excludes Anchor's 8-byte account discriminator.
        assert_eq!(Order::INIT_SPACE, 205);
    }

    #[test]
    fn price_level_account_size_is_stable() {
        // Excludes Anchor's 8-byte account discriminator.
        assert_eq!(PriceLevel::INIT_SPACE, 174);
    }

    #[test]
    fn trader_balance_account_size_is_stable() {
        // Excludes Anchor's 8-byte account discriminator.
        assert_eq!(TraderBalance::INIT_SPACE, 97);
    }

    #[test]
    fn order_side_seeds_are_stable_and_distinct() {
        assert_eq!(OrderSide::Bid.seed(), b"bid");
        assert_eq!(OrderSide::Ask.seed(), b"ask");
        assert_ne!(OrderSide::Bid.seed(), OrderSide::Ask.seed());
    }
}

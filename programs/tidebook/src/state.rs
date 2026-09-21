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
/// One immutable order submission and its mutable lifecycle state.
pub struct Order {
    pub owner: Pubkey,
    pub market: Pubkey,
    pub order_id: u64,
    pub side: OrderSide,
    pub price: u64,
    pub quantity: u64,
    pub remaining_quantity: u64,
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

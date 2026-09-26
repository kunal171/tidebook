//! Market creation, lifecycle, and shutdown events.

use anchor_lang::prelude::*;

use crate::state::MarketStatus;

/// Records creation of a market and its canonical custody vaults.
#[event]
#[derive(Debug, PartialEq, Eq)]
pub struct MarketInitializedEvent {
    pub market: Pubkey,
    pub authority: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub base_vault: Pubkey,
    pub quote_vault: Pubkey,
    pub price_tick_size: u64,
    pub quantity_lot_size: u64,
}

/// Records a successful pause or unpause transition.
#[event]
#[derive(Debug, PartialEq, Eq)]
pub struct MarketStatusChangedEvent {
    pub market: Pubkey,
    pub authority: Pubkey,
    pub status: MarketStatus,
}

/// Records successful closure of a paused, empty market and both vaults.
#[event]
#[derive(Debug, PartialEq, Eq)]
pub struct MarketClosedEvent {
    pub market: Pubkey,
    pub authority: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub base_vault: Pubkey,
    pub quote_vault: Pubkey,
}

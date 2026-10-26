//! Off-chain representations of Tidebook's serialized Anchor events.
//!
//! Field order and field types must exactly match the corresponding on-chain
//! event declarations. Borsh is positional and does not encode field names.

use borsh::BorshDeserialize;
use solana_pubkey::Pubkey;

#[derive(Debug, Clone, PartialEq, Eq, BorshDeserialize)]
pub struct MarketInitializedEvent {
    pub market: Pubkey,
    pub market_fees: Pubkey,
    pub authority: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub base_vault: Pubkey,
    pub quote_vault: Pubkey,
    pub price_tick_size: u64,
    pub quantity_lot_size: u64,
}

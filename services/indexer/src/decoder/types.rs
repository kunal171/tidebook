//! Off-chain representations of Tidebook's serialized Anchor events.
//!
//! Field order and field types must exactly match the corresponding on-chain
//! event declarations. Borsh is positional and does not encode field names.

use borsh::BorshDeserialize;
use solana_pubkey::Pubkey;

/// Off-chain mirror of the market-creation event emitted by the program.
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

/// On-chain market lifecycle state.
///
/// Borsh stores fieldless enum variants by their zero-based position. Do not
/// reorder these variants: `Active` is encoded as `0` and `Paused` as `1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshDeserialize)]
pub enum MarketStatus {
    Active,
    Paused,
}

/// Records an authorized pause or unpause transition.
#[derive(Debug, Clone, PartialEq, Eq, BorshDeserialize)]
pub struct MarketStatusChangedEvent {
    pub market: Pubkey,
    pub authority: Pubkey,
    pub status: MarketStatus,
}

/// Records successful closure of the market, fee state, and custody vaults.
#[derive(Debug, Clone, PartialEq, Eq, BorshDeserialize)]
pub struct MarketClosedEvent {
    pub market: Pubkey,
    pub market_fees: Pubkey,
    pub authority: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub base_vault: Pubkey,
    pub quote_vault: Pubkey,
}

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

/// Side of a resting order.
///
/// Borsh encodes `Bid` as `0` and `Ask` as `1`. Variant order must remain
/// identical to the on-chain `OrderSide`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshDeserialize)]
pub enum OrderSide {
    Bid,
    Ask,
}

/// Records an order entering a price-level FIFO queue.
#[derive(Debug, Clone, PartialEq, Eq, BorshDeserialize)]
pub struct OrderPlacedEvent {
    pub market: Pubkey,
    pub order: Pubkey,
    pub order_id: u64,
    pub owner: Pubkey,
    pub side: OrderSide,
    pub price: u64,
    pub quantity: u64,
    pub locked_collateral: u64,
    pub price_level: Pubkey,
}

/// Records owner-authorized cancellation of an open order.
#[derive(Debug, Clone, PartialEq, Eq, BorshDeserialize)]
pub struct OrderCanceledEvent {
    pub market: Pubkey,
    pub order: Pubkey,
    pub order_id: u64,
    pub owner: Pubkey,
    pub side: OrderSide,
    pub price: u64,
    pub canceled_quantity: u64,
    pub released_collateral: u64,
    pub price_level: Pubkey,
    pub price_level_closed: bool,
}

/// Records one maker-price execution between a resting maker and an incoming
/// taker.
///
/// Field order must remain identical to the on-chain `FillEvent`. Borsh uses
/// positional encoding, so reordering fields would silently corrupt decoding.
#[derive(Debug, Clone, PartialEq, Eq, BorshDeserialize)]
pub struct FillEvent {
    pub market: Pubkey,
    pub maker_order: Pubkey,
    pub maker_order_id: u64,
    pub maker: Pubkey,
    pub taker: Pubkey,
    pub maker_side: OrderSide,
    pub execution_price: u64,
    pub base_quantity: u64,
    pub quote_quantity: u64,
    pub taker_fee_quote: u64,
    pub maker_remaining_quantity: u64,
    pub taker_remaining_quantity: u64,
}

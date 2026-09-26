//! Order-book lifecycle events outside trade execution.

use anchor_lang::prelude::*;

use crate::state::OrderSide;

/// Records either new-level insertion or same-level FIFO append.
#[event]
#[derive(Debug, PartialEq, Eq)]
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

/// Records owner-authorized removal of the remaining open quantity.
#[event]
#[derive(Debug, PartialEq, Eq)]
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

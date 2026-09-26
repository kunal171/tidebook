//! Execution events emitted when a taker settles against a resting maker.

use anchor_lang::prelude::*;

use crate::state::OrderSide;

/// Describes one successful maker-price execution.
///
/// One `match_limit_order` instruction emits exactly one event. A client may
/// include several matching instructions in one transaction, producing several
/// events in FIFO execution order.
#[event]
#[derive(Debug, PartialEq, Eq)]
pub struct FillEvent {
    /// Market where the execution occurred.
    pub market: Pubkey,

    /// Resting order consumed by this execution.
    pub maker_order: Pubkey,

    /// Market-local identifier of the resting order.
    pub maker_order_id: u64,

    /// Owner of the resting order.
    pub maker: Pubkey,

    /// Wallet submitting the incoming taker order.
    pub taker: Pubkey,

    /// Side of the resting maker order.
    pub maker_side: OrderSide,

    /// Resting maker price used for settlement.
    pub execution_price: u64,

    /// Base-mint atoms exchanged.
    pub base_quantity: u64,

    /// Quote-mint atoms exchanged.
    pub quote_quantity: u64,

    /// Maker quantity remaining after this fill.
    pub maker_remaining_quantity: u64,

    /// Incoming taker quantity remaining after this fill.
    pub taker_remaining_quantity: u64,
}

//! Cancels an open order while enforcing market membership and ownership.
//!
//! Market status is intentionally not checked so owners can cancel while a
//! market is paused.

use anchor_lang::prelude::*;

use crate::{
    constants::ORDER_SEED,
    error::MarketError,
    state::{Market, Order, OrderStatus},
};

#[derive(Accounts)]
#[instruction(order_id: u64)]
pub struct CancelLimitOrder<'info> {
    pub owner: Signer<'info>,

    pub market: Account<'info, Market>,

    #[account(
        mut,
        seeds = [
            ORDER_SEED,
            market.key().as_ref(),
            order_id.to_le_bytes().as_ref()
        ],
        bump = order.bump,
        has_one = owner @ MarketError::UnauthorizedOrderOwner,
        has_one = market @ MarketError::OrderMarketMismatch
    )]
    pub order: Account<'info, Order>,
}

pub fn handle_cancel_limit_order(ctx: Context<CancelLimitOrder>, _order_id: u64) -> Result<()> {
    let order = &mut ctx.accounts.order;

    // Cancellation deliberately does not require an active market: owners
    // must retain an exit path while trading is paused.
    require!(order.status == OrderStatus::Open, MarketError::OrderNotOpen);

    order.status = OrderStatus::Canceled;

    Ok(())
}

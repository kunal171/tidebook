//! Reactivates a paused market under its stored market authority.
//!
//! The explicit Paused precondition prevents a successful no-op transaction
//! from being mistaken for a real lifecycle transition by clients or operators.

use anchor_lang::prelude::*;

use crate::{
    errors::market,
    events::MarketStatusChangedEvent,
    state::{Market, MarketStatus},
};

#[derive(Accounts)]
pub struct UnpauseMarket<'info> {
    pub authority: Signer<'info>,

    #[account(
        mut,
        has_one = authority,
        constraint = market.status == MarketStatus::Paused @ market::MarketAlreadyActive
    )]
    pub market: Account<'info, Market>,
}

pub fn handle_unpause_market(ctx: Context<UnpauseMarket>) -> Result<()> {
    ctx.accounts.market.status = MarketStatus::Active;

    emit!(MarketStatusChangedEvent {
        market: ctx.accounts.market.key(),
        authority: ctx.accounts.authority.key(),
        status: MarketStatus::Active,
    });

    Ok(())
}

//! Pauses an active market under its stored market authority.
//!
//! Pausing blocks new liquidity but deliberately does not block cancellation;
//! traders must retain an exit path while market administration investigates.

use anchor_lang::prelude::*;

use crate::{
    errors::market,
    state::{Market, MarketStatus},
};

#[derive(Accounts)]
pub struct PauseMarket<'info> {
    pub authority: Signer<'info>,

    #[account(
        mut,
        has_one = authority,
        constraint = market.status == MarketStatus::Active @ market::MarketAlreadyPaused
    )]
    pub market: Account<'info, Market>,
}

pub fn handle_pause_market(ctx: Context<PauseMarket>) -> Result<()> {
    // Cancellation has no Active constraint, so this transition freezes only
    // placement and preserves owner-controlled collateral withdrawal.
    ctx.accounts.market.status = MarketStatus::Paused;
    Ok(())
}

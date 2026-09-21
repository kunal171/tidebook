//! Reactivates a paused market under its stored market authority.

use anchor_lang::prelude::*;

use crate::{
    error::MarketError,
    state::{Market, MarketStatus},
};

#[derive(Accounts)]
pub struct UnpauseMarket<'info> {
    pub authority: Signer<'info>,

    #[account(
        mut,
        has_one = authority,
        constraint = market.status == MarketStatus::Paused @ MarketError::MarketAlreadyActive
    )]
    pub market: Account<'info, Market>,
}

pub fn handle_unpause_market(ctx: Context<UnpauseMarket>) -> Result<()> {
    ctx.accounts.market.status = MarketStatus::Active;
    Ok(())
}

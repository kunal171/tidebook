use anchor_lang::prelude::*;

use crate::{
    error::MarketError,
    state::{Market, MarketStatus},
};

//Pause active Market Pair
#[derive(Accounts)]
pub struct PauseMarket<'info> {
    pub authority: Signer<'info>,

    #[account(
        mut,
        has_one = authority,
        constraint = market.status == MarketStatus::Active @ MarketError::MarketAlreadyPaused
    )]
    pub market: Account<'info, Market>,
}

pub fn handle_pause_market(ctx: Context<PauseMarket>) -> Result<()> {
    ctx.accounts.market.status = MarketStatus::Paused;
    Ok(())
}

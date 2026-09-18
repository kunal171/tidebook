use anchor_lang::prelude::*;

use crate::{error::MarketError, state::Market};

//Pause active Market Pair
#[derive(Accounts)]
pub struct PauseMarket<'info> {
    pub authority: Signer<'info>,

    #[account(
        mut,
        has_one = authority,
        constraint = market.status == 0 @ MarketError::MarketAlreadyPaused
    )]
    pub market: Account<'info, Market>,
}

pub fn handle_pause_market(ctx: Context<PauseMarket>) -> Result<()> {
    ctx.accounts.market.status = 1;
    Ok(())
}

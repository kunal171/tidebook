use anchor_lang::prelude::*;

use crate::{error::MarketError, state::Market};

#[derive(Accounts)]
pub struct UnpauseMarket<'info> {
    pub authority: Signer<'info>,

    #[account(
        mut,
        has_one = authority,
        constraint = market.status == 1 @ MarketError::MarketAlreadyActive
    )]
    pub market: Account<'info, Market>,
}

pub fn handle_unpause_market(ctx: Context<UnpauseMarket>) -> Result<()> {
    ctx.accounts.market.status = 0;
    Ok(())
}

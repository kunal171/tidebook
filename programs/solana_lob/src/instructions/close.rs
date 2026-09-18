use anchor_lang::prelude::*;

use crate::{error::MarketError, state::Market};

#[derive(Accounts)]
pub struct CloseMarket<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        mut,
        has_one = authority,
        close = authority,
        constraint = market.status == 1 @ MarketError::MarketMustBePaused
    )]
    pub market: Account<'info, Market>,
}

pub fn handle_close_market(ctx: Context<CloseMarket>) -> Result<()> {
    msg!("Closing market {}", ctx.accounts.market.key());
    Ok(())
}

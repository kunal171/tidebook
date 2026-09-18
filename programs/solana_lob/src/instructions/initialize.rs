use anchor_lang::prelude::*;

use crate::{constants::MARKET_SEED, state::Market};

#[derive(Accounts)]
pub struct InitializeMarket<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(
        init,
        payer = authority,
        space = 8 + Market::INIT_SPACE,
        seeds = [
            MARKET_SEED,
            base_mint.key().as_ref(),
            quote_mint.key().as_ref()
        ],
        bump
    )]
    pub market: Account<'info, Market>,

    /// CHECK: base mint account; we use its key to derive the market PDA and
    /// validate it as a real SPL mint in later instruction logic.
    pub base_mint: UncheckedAccount<'info>,
    /// CHECK: quote mint account; we use its key to derive the market PDA and
    /// validate it as a real SPL mint in later instruction logic.
    pub quote_mint: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

pub fn handle_initialize_market(
    ctx: Context<InitializeMarket>,
) -> Result<()> {
    let market = &mut ctx.accounts.market;

    market.authority = ctx.accounts.authority.key();
    market.base_mint = ctx.accounts.base_mint.key();
    market.quote_mint = ctx.accounts.quote_mint.key();
    market.status = 0; // active
    market.next_order_id = 1;
    market.best_bid = None;
    market.best_ask = None;
    market.bump = ctx.bumps.market;

    Ok(())
}

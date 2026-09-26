//! Initializes the canonical balance ledger for one trader in one market.
//!
//! This instruction only creates internal accounting state. It does not move
//! tokens. Separate deposit and withdrawal instructions move assets between a
//! wallet token account and the market vault while updating this ledger.

use anchor_lang::prelude::*;

use crate::{
    constants::TRADER_BALANCE_SEED,
    events::TraderBalanceInitializedEvent,
    state::{Market, TraderBalance},
};

#[derive(Accounts)]
pub struct InitializeTraderBalance<'info> {
    /// Trader owns this balance record and pays its rent.
    #[account(mut)]
    pub owner: Signer<'info>,

    /// Market whose vaults will eventually back these balances.
    pub market: Account<'info, Market>,

    /// Exactly one balance account may exist per `(market, owner)` pair.
    #[account(
        init,
        payer = owner,
        space = 8 + TraderBalance::INIT_SPACE,
        seeds = [
            TRADER_BALANCE_SEED,
            market.key().as_ref(),
            owner.key().as_ref(),
        ],
        bump
    )]
    pub trader_balance: Account<'info, TraderBalance>,

    pub system_program: Program<'info, System>,
}

pub fn handle_initialize_trader_balance(ctx: Context<InitializeTraderBalance>) -> Result<()> {
    let trader_balance = &mut ctx.accounts.trader_balance;

    trader_balance.market = ctx.accounts.market.key();
    trader_balance.owner = ctx.accounts.owner.key();

    // These assignments are explicit even though newly allocated account data
    // begins as zero. They document the protocol's initial accounting state.
    trader_balance.base_free = 0;
    trader_balance.base_locked = 0;
    trader_balance.quote_free = 0;
    trader_balance.quote_locked = 0;

    trader_balance.bump = ctx.bumps.trader_balance;

    emit!(TraderBalanceInitializedEvent {
        market: ctx.accounts.market.key(),
        owner: ctx.accounts.owner.key(),
        trader_balance: ctx.accounts.trader_balance.key(),
    });

    Ok(())
}

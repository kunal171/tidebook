//! Deposits base or quote tokens into a market vault.
//!
//! The token transfer and internal free-balance credit happen atomically.
//! Failure of either operation rolls back the entire instruction.

use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, TransferChecked};

use crate::{
    constants::{TRADER_BALANCE_SEED, VAULT_AUTHORITY_SEED, VAULT_SEED},
    errors::balance,
    events::DepositEvent,
    state::{Market, TraderBalance},
};

#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    pub market: Account<'info, Market>,

    #[account(
        mut,
        seeds = [
            TRADER_BALANCE_SEED,
            market.key().as_ref(),
            owner.key().as_ref(),
        ],
        bump = trader_balance.bump,
        constraint = trader_balance.market == market.key()
            @ balance::TraderBalanceMarketMismatch,
        constraint = trader_balance.owner == owner.key()
            @ balance::TraderBalanceOwnerMismatch
    )]
    pub trader_balance: Account<'info, TraderBalance>,

    /// Base or quote mint selected for this deposit.
    pub deposit_mint: Account<'info, Mint>,

    /// Trader-owned source token account.
    #[account(
        mut,
        constraint = trader_token_account.owner == owner.key()
            @ balance::TraderBalanceOwnerMismatch,
        constraint = trader_token_account.mint == deposit_mint.key()
            @ balance::InvalidDepositMint
    )]
    pub trader_token_account: Account<'info, TokenAccount>,

    /// CHECK: Canonical stateless authority for this market's vaults.
    #[account(
        seeds = [
            VAULT_AUTHORITY_SEED,
            market.key().as_ref(),
        ],
        bump
    )]
    pub vault_authority: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [
            VAULT_SEED,
            market.key().as_ref(),
            deposit_mint.key().as_ref(),
        ],
        bump,
        token::mint = deposit_mint,
        token::authority = vault_authority
    )]
    pub market_vault: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

pub fn handle_deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
    require!(amount > 0, balance::InvalidDepositAmount);

    require!(
        ctx.accounts.trader_token_account.amount >= amount,
        balance::InsufficientDepositFunds
    );

    let mint = ctx.accounts.deposit_mint.key();

    // Calculate the next balance before moving tokens. This prevents a transfer
    // from being attempted when the internal accounting cannot represent it.
    let next_free_balance = if mint == ctx.accounts.market.base_mint {
        ctx.accounts
            .trader_balance
            .base_free
            .checked_add(amount)
            .ok_or(balance::FreeBalanceOverflow)?
    } else if mint == ctx.accounts.market.quote_mint {
        ctx.accounts
            .trader_balance
            .quote_free
            .checked_add(amount)
            .ok_or(balance::FreeBalanceOverflow)?
    } else {
        return err!(balance::InvalidDepositMint);
    };

    token::transfer_checked(
        CpiContext::new(
            ctx.accounts.token_program.key(),
            TransferChecked {
                from: ctx.accounts.trader_token_account.to_account_info(),
                mint: ctx.accounts.deposit_mint.to_account_info(),
                to: ctx.accounts.market_vault.to_account_info(),
                authority: ctx.accounts.owner.to_account_info(),
            },
        ),
        amount,
        ctx.accounts.deposit_mint.decimals,
    )?;

    if mint == ctx.accounts.market.base_mint {
        ctx.accounts.trader_balance.base_free = next_free_balance;
    } else {
        ctx.accounts.trader_balance.quote_free = next_free_balance;
    }

    emit!(DepositEvent {
        market: ctx.accounts.market.key(),
        owner: ctx.accounts.owner.key(),
        trader_balance: ctx.accounts.trader_balance.key(),
        mint,
        market_vault: ctx.accounts.market_vault.key(),
        amount,
        free_balance: next_free_balance,
    });

    Ok(())
}

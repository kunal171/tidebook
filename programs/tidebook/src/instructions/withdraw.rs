//! Withdraws free base or quote tokens from a market vault.
//!
//! Locked balances cannot be withdrawn. The free-balance debit and SPL token
//! transfer execute atomically and remain available while the market is paused.

use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, TransferChecked};

use crate::{
    constants::{TRADER_BALANCE_SEED, VAULT_AUTHORITY_SEED, VAULT_SEED},
    errors::balance,
    events::WithdrawalEvent,
    state::{Market, TraderBalance},
};

#[derive(Accounts)]
pub struct Withdraw<'info> {
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

    /// Base or quote mint selected for withdrawal.
    pub withdrawal_mint: Account<'info, Mint>,

    /// Trader-owned destination token account.
    #[account(
        mut,
        constraint = owner_token_account.owner == owner.key()
            @ balance::InvalidWithdrawalDestinationOwner,
        constraint = owner_token_account.mint == withdrawal_mint.key()
            @ balance::InvalidWithdrawalMint
    )]
    pub owner_token_account: Account<'info, TokenAccount>,

    /// CHECK: Canonical PDA that owns the market vault.
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
            withdrawal_mint.key().as_ref(),
        ],
        bump,
        token::mint = withdrawal_mint,
        token::authority = vault_authority
    )]
    pub market_vault: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

pub fn handle_withdraw(ctx: Context<Withdraw>, amount: u64) -> Result<()> {
    require!(amount > 0, balance::InvalidWithdrawalAmount);

    let mint = ctx.accounts.withdrawal_mint.key();

    // Calculate the final accounting state before invoking the token program.
    let next_free_balance = if mint == ctx.accounts.market.base_mint {
        ctx.accounts
            .trader_balance
            .base_free
            .checked_sub(amount)
            .ok_or(balance::InsufficientFreeBalance)?
    } else if mint == ctx.accounts.market.quote_mint {
        ctx.accounts
            .trader_balance
            .quote_free
            .checked_sub(amount)
            .ok_or(balance::InsufficientFreeBalance)?
    } else {
        return err!(balance::InvalidWithdrawalMint);
    };

    // A violation here indicates broken internal accounting because the vault
    // should always back every free and locked balance.
    require!(
        ctx.accounts.market_vault.amount >= amount,
        balance::InsufficientVaultFunds
    );

    let market_key = ctx.accounts.market.key();
    let vault_authority_bump = [ctx.bumps.vault_authority];

    let vault_authority_seeds: &[&[u8]] = &[
        VAULT_AUTHORITY_SEED,
        market_key.as_ref(),
        &vault_authority_bump,
    ];

    let signer_seeds = &[vault_authority_seeds];

    token::transfer_checked(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.key(),
            TransferChecked {
                from: ctx.accounts.market_vault.to_account_info(),
                mint: ctx.accounts.withdrawal_mint.to_account_info(),
                to: ctx.accounts.owner_token_account.to_account_info(),
                authority: ctx.accounts.vault_authority.to_account_info(),
            },
            signer_seeds,
        ),
        amount,
        ctx.accounts.withdrawal_mint.decimals,
    )?;

    if mint == ctx.accounts.market.base_mint {
        ctx.accounts.trader_balance.base_free = next_free_balance;
    } else {
        ctx.accounts.trader_balance.quote_free = next_free_balance;
    }

    emit!(WithdrawalEvent {
        market: market_key,
        owner: ctx.accounts.owner.key(),
        trader_balance: ctx.accounts.trader_balance.key(),
        mint,
        market_vault: ctx.accounts.market_vault.key(),
        amount,
        free_balance: next_free_balance,
    });

    Ok(())
}

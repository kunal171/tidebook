//! Safely closes a paused market, fee accumulator, and canonical token vaults.
//!
//! Shutdown is rejected while open orders, protocol fees, or vault balances
//! remain.

use anchor_lang::prelude::*;
use anchor_spl::token::{self, CloseAccount, Token, TokenAccount};

use crate::{
    constants::{MARKET_FEES_SEED, VAULT_AUTHORITY_SEED, VAULT_SEED},
    errors::{fee, market},
    events::MarketClosedEvent,
    state::{Market, MarketFees, MarketStatus},
};

#[derive(Accounts)]
pub struct CloseMarket<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        mut,
        has_one = authority,
        close = authority,
        constraint = market.status == MarketStatus::Paused @ market::MarketMustBePaused
    )]
    pub market: Account<'info, Market>,

    /// Canonical protocol-fee claim backed by the quote vault.
    ///
    /// Anchor closes this account only after the handler succeeds, returning
    /// its rent to the same authority that receives the market and vault rent.
    #[account(
        mut,
        close = authority,
        seeds = [
            MARKET_FEES_SEED,
            market.key().as_ref()
        ],
        bump = market_fees.bump,
        constraint = market_fees.market == market.key()
            @ fee::MarketFeesMarketMismatch,
        constraint = market_fees.quote_mint == market.quote_mint
            @ fee::MarketFeesQuoteMintMismatch
    )]
    pub market_fees: Account<'info, MarketFees>,

    /// CHECK: Seed-constrained authority for both market vaults.
    #[account(
        seeds = [
            VAULT_AUTHORITY_SEED,
            market.key().as_ref()
        ],
        bump
    )]
    pub vault_authority: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [
            VAULT_SEED,
            market.key().as_ref(),
            market.base_mint.as_ref()
        ],
        bump,
        constraint = base_vault.mint == market.base_mint
            @ market::InvalidCollateralMint,
        constraint = base_vault.owner == vault_authority.key()
            @ market::InvalidVaultAuthority
    )]
    pub base_vault: Account<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [
            VAULT_SEED,
            market.key().as_ref(),
            market.quote_mint.as_ref()
        ],
        bump,
        constraint = quote_vault.mint == market.quote_mint
            @ market::InvalidCollateralMint,
        constraint = quote_vault.owner == vault_authority.key()
            @ market::InvalidVaultAuthority
    )]
    pub quote_vault: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

pub fn handle_close_market(ctx: Context<CloseMarket>) -> Result<()> {
    // The counter protects indexed orders; checking token balances separately
    // also catches unsolicited transfers and accounting bugs before shutdown.
    require!(
        ctx.accounts.market.open_order_count == 0,
        market::MarketHasOpenOrders
    );

    // Protocol revenue is a real claim on quote-vault assets. It must be
    // withdrawn through the future governed fee-withdrawal instruction rather
    // than silently discarded during market shutdown.
    require!(
        ctx.accounts.market_fees.accrued_quote_fees == 0,
        fee::MarketFeesNotEmpty
    );

    require!(
        ctx.accounts.base_vault.amount == 0 && ctx.accounts.quote_vault.amount == 0,
        market::MarketVaultNotEmpty
    );
    msg!("Closing market {}", ctx.accounts.market.key());

    let market_key = ctx.accounts.market.key();
    let vault_authority_bump = [ctx.bumps.vault_authority];

    let vault_authority_seeds: &[&[u8]] = &[
        VAULT_AUTHORITY_SEED,
        market_key.as_ref(),
        &vault_authority_bump,
    ];

    let signer_seeds = &[vault_authority_seeds];

    // Both CPIs and Anchor's later `close = authority` market cleanup are in
    // one Solana transaction. Any failure rolls back every lamport and account.
    token::close_account(CpiContext::new_with_signer(
        ctx.accounts.token_program.key(),
        CloseAccount {
            account: ctx.accounts.base_vault.to_account_info(),
            destination: ctx.accounts.authority.to_account_info(),
            authority: ctx.accounts.vault_authority.to_account_info(),
        },
        signer_seeds,
    ))?;

    token::close_account(CpiContext::new_with_signer(
        ctx.accounts.token_program.key(),
        CloseAccount {
            account: ctx.accounts.quote_vault.to_account_info(),
            destination: ctx.accounts.authority.to_account_info(),
            authority: ctx.accounts.vault_authority.to_account_info(),
        },
        signer_seeds,
    ))?;

    emit!(MarketClosedEvent {
        market: market_key,
        market_fees: ctx.accounts.market_fees.key(),
        authority: ctx.accounts.authority.key(),
        base_mint: ctx.accounts.market.base_mint,
        quote_mint: ctx.accounts.market.quote_mint,
        base_vault: ctx.accounts.base_vault.key(),
        quote_vault: ctx.accounts.quote_vault.key(),
    });

    Ok(())
}

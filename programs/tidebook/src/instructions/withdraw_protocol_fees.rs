//! Withdraws accrued quote-denominated protocol fees under super-admin control.
//!
//! Fee tokens remain in the market quote vault until this instruction moves
//! them to a chosen quote-token treasury account. The destination need not be
//! owned by the signer, allowing governance to use a dedicated treasury, but
//! it must use the market's quote mint.

use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, TransferChecked};

use crate::{
    constants::{
        MARKET_FEES_SEED, MARKET_SEED, PROTOCOL_CONFIG_SEED, VAULT_AUTHORITY_SEED, VAULT_SEED,
    },
    errors::{admin, balance, fee},
    events::ProtocolFeesWithdrawnEvent,
    state::{Market, MarketFees, ProtocolConfig},
};

#[derive(Accounts)]
pub struct WithdrawProtocolFees<'info> {
    pub super_admin: Signer<'info>,

    #[account(
        seeds = [PROTOCOL_CONFIG_SEED],
        bump = protocol_config.bump,
        has_one = super_admin @ admin::UnauthorizedSuperAdmin
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,

    #[account(
        seeds = [
            MARKET_SEED,
            market.base_mint.as_ref(),
            market.quote_mint.as_ref(),
        ],
        bump = market.bump
    )]
    pub market: Account<'info, Market>,

    #[account(
        mut,
        seeds = [MARKET_FEES_SEED, market.key().as_ref()],
        bump = market_fees.bump,
        constraint = market_fees.market == market.key()
            @ fee::MarketFeesMarketMismatch,
        constraint = market_fees.quote_mint == market.quote_mint
            @ fee::MarketFeesQuoteMintMismatch
    )]
    pub market_fees: Account<'info, MarketFees>,

    #[account(
        constraint = quote_mint.key() == market.quote_mint
            @ fee::InvalidFeeDestinationMint
    )]
    pub quote_mint: Account<'info, Mint>,

    /// Treasury chosen by the super-admin; only its mint is constrained.
    #[account(
        mut,
        constraint = destination_quote_account.mint == quote_mint.key()
            @ fee::InvalidFeeDestinationMint
    )]
    pub destination_quote_account: Account<'info, TokenAccount>,

    /// CHECK: Canonical PDA that signs transfers from both market vaults.
    #[account(
        seeds = [VAULT_AUTHORITY_SEED, market.key().as_ref()],
        bump
    )]
    pub vault_authority: UncheckedAccount<'info>,

    #[account(
        mut,
        seeds = [VAULT_SEED, market.key().as_ref(), quote_mint.key().as_ref()],
        bump,
        token::mint = quote_mint,
        token::authority = vault_authority
    )]
    pub quote_vault: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

pub fn handle_withdraw_protocol_fees(
    ctx: Context<WithdrawProtocolFees>,
    amount: u64,
) -> Result<()> {
    require!(amount > 0, fee::InvalidFeeWithdrawalAmount);

    let next_accrued_quote_fees = ctx
        .accounts
        .market_fees
        .accrued_quote_fees
        .checked_sub(amount)
        .ok_or(fee::InsufficientAccruedFees)?;

    require!(
        ctx.accounts.quote_vault.amount >= amount,
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
                from: ctx.accounts.quote_vault.to_account_info(),
                mint: ctx.accounts.quote_mint.to_account_info(),
                to: ctx.accounts.destination_quote_account.to_account_info(),
                authority: ctx.accounts.vault_authority.to_account_info(),
            },
            signer_seeds,
        ),
        amount,
        ctx.accounts.quote_mint.decimals,
    )?;

    ctx.accounts.market_fees.accrued_quote_fees = next_accrued_quote_fees;

    emit!(ProtocolFeesWithdrawnEvent {
        market: market_key,
        market_fees: ctx.accounts.market_fees.key(),
        super_admin: ctx.accounts.super_admin.key(),
        quote_mint: ctx.accounts.quote_mint.key(),
        quote_vault: ctx.accounts.quote_vault.key(),
        destination_quote_account: ctx.accounts.destination_quote_account.key(),
        amount,
        remaining_accrued_quote_fees: next_accrued_quote_fees,
    });

    Ok(())
}

//! Atomically initializes a market and its canonical SPL Token vaults.
//!
//! Anchor constraints establish the active-admin boundary, ordered mint pair,
//! market PDA, stateless vault authority, and mint-specific vault addresses.

use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token, TokenAccount};

use crate::{
    constants::{ADMIN_SEED, MARKET_SEED, VAULT_AUTHORITY_SEED, VAULT_SEED},
    errors::{admin, market},
    events::MarketInitializedEvent,
    state::{AdminRecord, AdminStatus, Market, MarketStatus},
};

#[derive(Accounts)]
pub struct InitializeMarket<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        seeds = [ADMIN_SEED, authority.key().as_ref()],
        bump = admin_record.bump,
        has_one = authority @ admin::UnauthorizedAdmin,
        constraint = admin_record.status == AdminStatus::Active
            @ admin::AdminDisabled
    )]
    pub admin_record: Account<'info, AdminRecord>,

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

    /// SPL mint for the asset being traded.
    pub base_mint: Account<'info, Mint>,

    /// SPL mint used to denominate order prices.
    #[account(
    constraint = base_mint.key() != quote_mint.key()
        @ market::IdenticalMints
    )]
    pub quote_mint: Account<'info, Mint>,

    /// CHECK: This is a seed-constrained, stateless PDA. It owns both vaults
    /// and will sign token instructions through `invoke_signed`.
    #[account(
        seeds = [
            VAULT_AUTHORITY_SEED,
            market.key().as_ref()
        ],
        bump
    )]
    pub vault_authority: UncheckedAccount<'info>,

    #[account(
        init,
        payer = authority,
        token::mint = base_mint,
        token::authority = vault_authority,
        seeds = [
            VAULT_SEED,
            market.key().as_ref(),
            base_mint.key().as_ref()
        ],
        bump
    )]
    /// Canonical custody account for the market's base asset.
    pub base_vault: Account<'info, TokenAccount>,

    #[account(
        init,
        payer = authority,
        token::mint = quote_mint,
        token::authority = vault_authority,
        seeds = [
            VAULT_SEED,
            market.key().as_ref(),
            quote_mint.key().as_ref()
        ],
        bump
    )]
    /// Canonical custody account for the market's quote asset.
    pub quote_vault: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,

    pub system_program: Program<'info, System>,
}

pub fn handle_initialize_market(
    ctx: Context<InitializeMarket>,
    price_tick_size: u64,
    quantity_lot_size: u64,
) -> Result<()> {
    // Validate configuration before relying on it in order modulo checks.
    require!(price_tick_size > 0, market::InvalidPriceTickSize);
    require!(quantity_lot_size > 0, market::InvalidQuantityLotSize);

    // Anchor has already created the market and both token vaults at this
    // point. Any later failure rolls the entire instruction back atomically.
    let market = &mut ctx.accounts.market;

    market.authority = ctx.accounts.authority.key();
    market.base_mint = ctx.accounts.base_mint.key();
    market.quote_mint = ctx.accounts.quote_mint.key();
    market.base_decimals = ctx.accounts.base_mint.decimals;
    market.quote_decimals = ctx.accounts.quote_mint.decimals;
    market.price_tick_size = price_tick_size;
    market.quantity_lot_size = quantity_lot_size;
    market.status = MarketStatus::Active;
    market.next_order_id = 1;
    market.best_bid = None;
    market.best_ask = None;
    market.open_order_count = 0;
    market.bump = ctx.bumps.market;

    emit!(MarketInitializedEvent {
        market: ctx.accounts.market.key(),
        authority: ctx.accounts.authority.key(),
        base_mint: ctx.accounts.base_mint.key(),
        quote_mint: ctx.accounts.quote_mint.key(),
        base_vault: ctx.accounts.base_vault.key(),
        quote_vault: ctx.accounts.quote_vault.key(),
        price_tick_size,
        quantity_lot_size,
    });

    Ok(())
}

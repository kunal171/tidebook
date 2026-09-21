use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token, TokenAccount};

use crate::{
    constants::{ADMIN_SEED, MARKET_SEED, VAULT_AUTHORITY_SEED, VAULT_SEED},
    error::MarketError,
    state::{AdminRecord, AdminStatus, Market, MarketStatus},
};

#[derive(Accounts)]
pub struct InitializeMarket<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        seeds = [ADMIN_SEED, authority.key().as_ref()],
        bump = admin_record.bump,
        has_one = authority @ MarketError::UnauthorizedAdmin,
        constraint = admin_record.status == AdminStatus::Active
            @ MarketError::AdminDisabled
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

    /// CHECK: base mint account; we use its key to derive the market PDA and
    /// validate it as a real SPL mint in later instruction logic.
    pub base_mint: Account<'info, Mint>,

    /// CHECK: quote mint account; we use its key to derive the market PDA and
    /// validate it as a real SPL mint in later instruction logic.
    #[account(
    constraint = base_mint.key() != quote_mint.key()
        @ MarketError::IdenticalMints
    )]
    pub quote_mint: Account<'info, Mint>,

    /// CHECK: Canonical PDA that signs for both market vaults.
    /// It stores no data.
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
    pub quote_vault: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,

    pub system_program: Program<'info, System>,
}

pub fn handle_initialize_market(
    ctx: Context<InitializeMarket>,
    price_tick_size: u64,
    quantity_lot_size: u64,
) -> Result<()> {
    require!(price_tick_size > 0, MarketError::InvalidPriceTickSize);
    require!(quantity_lot_size > 0, MarketError::InvalidQuantityLotSize);

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
    market.bump = ctx.bumps.market;

    Ok(())
}

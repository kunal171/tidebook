//! Inserts the first order at a new distinct price level.
//!
//! The client supplies the optional adjacent better and worse levels. The
//! program treats them as untrusted hints and validates ordering and reciprocal
//! links before modifying the sorted level index.

use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token, TokenAccount};

use crate::{
    constants::{ORDER_SEED, PRICE_LEVEL_SEED, VAULT_AUTHORITY_SEED, VAULT_SEED},
    error::MarketError,
    state::{Market, MarketStatus, Order, OrderSide, PriceLevel},
};

#[derive(Accounts)]
#[instruction(side: OrderSide, price: u64, quantity: u64)]
pub struct InsertLimitOrder<'info> {
    #[account(mut)]
    pub trader: Signer<'info>,

    #[account(
        mut,
        constraint = market.status == MarketStatus::Active
            @ MarketError::MarketNotActive
    )]
    pub market: Account<'info, Market>,

    #[account(
        init,
        payer = trader,
        space = 8 + Order::INIT_SPACE,
        seeds = [
            ORDER_SEED,
            market.key().as_ref(),
            market.next_order_id.to_le_bytes().as_ref()
        ],
        bump
    )]
    pub order: Account<'info, Order>,

    #[account(
        init,
        payer = trader,
        space = 8 + PriceLevel::INIT_SPACE,
        seeds = [
            PRICE_LEVEL_SEED,
            market.key().as_ref(),
            side.seed(),
            price.to_le_bytes().as_ref()
        ],
        bump
    )]
    pub price_level: Account<'info, PriceLevel>,

    /// Mint selected as collateral: base for asks and quote for bids.
    pub collateral_mint: Account<'info, Mint>,

    #[account(
        mut,
        constraint = trader_collateral.owner == trader.key()
            @ MarketError::InvalidCollateralOwner,
        constraint = trader_collateral.mint == collateral_mint.key()
            @ MarketError::InvalidCollateralMint
    )]
    pub trader_collateral: Account<'info, TokenAccount>,

    /// CHECK: Canonical stateless authority of the market vaults.
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
            collateral_mint.key().as_ref()
        ],
        bump,
        token::mint = collateral_mint,
        token::authority = vault_authority
    )]
    pub market_vault: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,

    pub system_program: Program<'info, System>,

    /// Adjacent level with higher matching priority, or `None` for a new best.
    #[account(mut)]
    pub better_level: Option<Account<'info, PriceLevel>>,

    /// Adjacent level with lower matching priority, or `None` for a new worst.
    #[account(mut)]
    pub worse_level: Option<Account<'info, PriceLevel>>,
}

pub fn handle_insert_limit_order(
    _ctx: Context<InsertLimitOrder>,
    _side: OrderSide,
    _price: u64,
    _quantity: u64,
) -> Result<()> {
    err!(MarketError::PriceLevelInsertionNotImplemented)
}

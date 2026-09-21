//! Cancels an open order and refunds its remaining locked collateral.
//!
//! Market status is intentionally not checked so owners can cancel while a
//! market is paused.

use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, TransferChecked};

use crate::{
    constants::{ORDER_SEED, VAULT_AUTHORITY_SEED, VAULT_SEED},
    error::MarketError,
    state::{Market, Order, OrderSide, OrderStatus},
};

#[derive(Accounts)]
#[instruction(order_id: u64)]
pub struct CancelLimitOrder<'info> {
    pub owner: Signer<'info>,

    pub market: Account<'info, Market>,

    #[account(
        mut,
        seeds = [
            ORDER_SEED,
            market.key().as_ref(),
            order_id.to_le_bytes().as_ref()
        ],
        bump = order.bump,
        has_one = owner @ MarketError::UnauthorizedOrderOwner,
        has_one = market @ MarketError::OrderMarketMismatch
    )]
    pub order: Account<'info, Order>,

    pub collateral_mint: Account<'info, Mint>,

    #[account(
        mut,
        constraint = owner_collateral.owner == owner.key()
            @ MarketError::InvalidCollateralOwner,
        constraint = owner_collateral.mint == collateral_mint.key()
            @ MarketError::InvalidCollateralMint
    )]
    pub owner_collateral: Account<'info, TokenAccount>,

    /// CHECK: Seed-constrained authority that signs the refund transfer.
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
}

pub fn handle_cancel_limit_order(ctx: Context<CancelLimitOrder>, _order_id: u64) -> Result<()> {
    let order = &mut ctx.accounts.order;
    // Cancellation deliberately does not require an active market: owners
    // must retain an exit path while trading is paused.
    require!(order.status == OrderStatus::Open, MarketError::OrderNotOpen);

    let expected_mint = match order.side {
        OrderSide::Ask => ctx.accounts.market.base_mint,
        OrderSide::Bid => ctx.accounts.market.quote_mint,
    };

    require_keys_eq!(
        ctx.accounts.collateral_mint.key(),
        expected_mint,
        MarketError::InvalidCollateralMint
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
                mint: ctx.accounts.collateral_mint.to_account_info(),
                to: ctx.accounts.owner_collateral.to_account_info(),
                authority: ctx.accounts.vault_authority.to_account_info(),
            },
            signer_seeds,
        ),
        order.locked_collateral,
        ctx.accounts.collateral_mint.decimals,
    )?;

    order.locked_collateral = 0;
    order.status = OrderStatus::Canceled;

    Ok(())
}

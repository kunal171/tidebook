//! Cancels an open order and refunds its remaining locked collateral.
//!
//! Market status is intentionally not checked so owners can cancel while a
//! market is paused.

use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, TransferChecked};

use crate::{
    constants::{ORDER_SEED, PRICE_LEVEL_SEED, VAULT_AUTHORITY_SEED, VAULT_SEED},
    error::MarketError,
    state::{Market, Order, OrderSide, OrderStatus, PriceLevel},
};

#[derive(Accounts)]
#[instruction(order_id: u64)]
pub struct CancelLimitOrder<'info> {
    pub owner: Signer<'info>,

    #[account(mut)]
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

    #[account(
        mut,
        seeds = [
            PRICE_LEVEL_SEED,
            market.key().as_ref(),
            order.side.seed(),
            order.price.to_le_bytes().as_ref(),
        ],
        bump = price_level.bump,
        constraint = order.price_level == price_level.key()
            @ MarketError::OrderPriceLevelMismatch,
        constraint = price_level.market == market.key()
            @ MarketError::PriceLevelMarketMismatch,
        constraint = price_level.side == order.side
            @ MarketError::PriceLevelSideMismatch,
        constraint = price_level.price == order.price
            @ MarketError::PriceLevelPriceMismatch,
    )]
    pub price_level: Account<'info, PriceLevel>,

    /// Older FIFO neighbor. None when canceling the queue head.
    #[account(mut)]
    pub previous_order: Option<Account<'info, Order>>,

    /// Newer FIFO neighbor. None when canceling the queue tail.
    #[account(mut)]
    pub next_order: Option<Account<'info, Order>>,

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
    // Cancellation deliberately does not require an active market: owners
    // must retain an exit path while trading is paused.
    require!(
        ctx.accounts.order.status == OrderStatus::Open,
        MarketError::OrderNotOpen
    );

    // Snapshot immutable order state before borrowing any queue account mutably.
    // This also keeps validation separate from the eventual state transition.
    let order_key = ctx.accounts.order.key();
    let order_side = ctx.accounts.order.side;
    let order_previous = ctx.accounts.order.previous_order;
    let order_next = ctx.accounts.order.next_order;
    let order_remaining_quantity = ctx.accounts.order.remaining_quantity;
    let locked_collateral = ctx.accounts.order.locked_collateral;

    let price_level_key = ctx.accounts.price_level.key();

    // Optional accounts are untrusted client hints. Their presence must exactly
    // match the links stored in the program-owned order.
    let previous_key = ctx
        .accounts
        .previous_order
        .as_ref()
        .map(|account| account.key());

    let next_key = ctx
        .accounts
        .next_order
        .as_ref()
        .map(|account| account.key());

    require!(
        order_previous == previous_key,
        MarketError::InvalidOrderNeighbor
    );

    require!(order_next == next_key, MarketError::InvalidOrderNeighbor);

    // A matching key is insufficient: reciprocal links prove that the supplied
    // accounts are the adjacent FIFO nodes rather than unrelated orders.
    if let Some(previous_order) = ctx.accounts.previous_order.as_ref() {
        require!(
            previous_order.status == OrderStatus::Open,
            MarketError::OrderNotOpen
        );

        require_keys_eq!(
            previous_order.price_level,
            price_level_key,
            MarketError::OrderPriceLevelMismatch
        );

        require!(
            previous_order.next_order == Some(order_key),
            MarketError::BrokenOrderQueueLink
        );
    }

    if let Some(next_order) = ctx.accounts.next_order.as_ref() {
        require!(
            next_order.status == OrderStatus::Open,
            MarketError::OrderNotOpen
        );

        require_keys_eq!(
            next_order.price_level,
            price_level_key,
            MarketError::OrderPriceLevelMismatch
        );

        require!(
            next_order.previous_order == Some(order_key),
            MarketError::BrokenOrderQueueLink
        );
    }

    let expected_mint = match order_side {
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

    // Calculate all fallible counter changes before the token CPI or mutations.
    // Solana would roll back on failure regardless, but this ordering keeps the
    // state-transition boundary explicit and easy to audit.
    let next_open_order_count = ctx
        .accounts
        .market
        .open_order_count
        .checked_sub(1)
        .ok_or(MarketError::OpenOrderCountUnderflow)?;

    let next_level_order_count = ctx
        .accounts
        .price_level
        .order_count
        .checked_sub(1)
        .ok_or(MarketError::PriceLevelOrderCountUnderflow)?;

    let next_level_quantity = ctx
        .accounts
        .price_level
        .total_remaining_quantity
        .checked_sub(order_remaining_quantity)
        .ok_or(MarketError::PriceLevelQuantityUnderflow)?;

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
        locked_collateral,
        ctx.accounts.collateral_mint.decimals,
    )?;

    // Splice the node out in O(1). Missing neighbors identify head or tail; when
    // both are missing this leaves an empty level for the next closure milestone.
    if let Some(previous_order) = ctx.accounts.previous_order.as_mut() {
        previous_order.next_order = next_key;
    } else {
        // The canceled order was the FIFO head.
        ctx.accounts.price_level.first_order = next_key;
    }

    if let Some(next_order) = ctx.accounts.next_order.as_mut() {
        next_order.previous_order = previous_key;
    } else {
        // The canceled order was the FIFO tail.
        ctx.accounts.price_level.last_order = previous_key;
    }

    ctx.accounts.price_level.order_count = next_level_order_count;
    ctx.accounts.price_level.total_remaining_quantity = next_level_quantity;

    // Clear queue links as well as balances so canceled orders cannot be
    // mistaken for live index members by clients or later matching code.
    {
        let order = &mut ctx.accounts.order;

        order.locked_collateral = 0;
        order.remaining_quantity = 0;
        order.previous_order = None;
        order.next_order = None;
        order.status = OrderStatus::Canceled;
    }
    ctx.accounts.market.open_order_count = next_open_order_count;

    Ok(())
}

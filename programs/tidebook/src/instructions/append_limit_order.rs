//! Appends a limit order to an existing price level.
//!
//! The current tail order is supplied explicitly and validated through
//! reciprocal links before the FIFO queue is mutated.

use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, TransferChecked};

use crate::{
    constants::{ORDER_SEED, PRICE_LEVEL_SEED, VAULT_AUTHORITY_SEED, VAULT_SEED},
    error::MarketError,
    state::{Market, MarketStatus, Order, OrderSide, OrderStatus, PriceLevel},
};

#[derive(Accounts)]
#[instruction(side: OrderSide, price: u64, quantity: u64)]
pub struct AppendLimitOrder<'info> {
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
        mut,
        seeds = [
            PRICE_LEVEL_SEED,
            market.key().as_ref(),
            side.seed(),
            price.to_le_bytes().as_ref()
        ],
        bump = price_level.bump,
        constraint = price_level.market == market.key()
            @ MarketError::PriceLevelMarketMismatch,
        constraint = price_level.side == side
            @ MarketError::PriceLevelSideMismatch,
        constraint = price_level.price == price
            @ MarketError::PriceLevelPriceMismatch
    )]
    pub price_level: Account<'info, PriceLevel>,

    #[account(
        mut,
        constraint = price_level.last_order == Some(previous_order.key())
            @ MarketError::InvalidPriceLevelTail,
        constraint = previous_order.price_level == price_level.key()
            @ MarketError::OrderPriceLevelMismatch,
        constraint = previous_order.status == OrderStatus::Open
            @ MarketError::OrderNotOpen,
        constraint = previous_order.next_order.is_none()
            @ MarketError::InvalidPriceLevelTail
    )]
    pub previous_order: Account<'info, Order>,

    /// Mint used as collateral: base for asks and quote for bids.
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
}

pub fn handle_append_limit_order(
    ctx: Context<AppendLimitOrder>,
    side: OrderSide,
    price: u64,
    quantity: u64,
) -> Result<()> {
    require!(price > 0, MarketError::InvalidPrice);
    require!(quantity > 0, MarketError::InvalidQuantity);

    let market_key = ctx.accounts.market.key();
    let order_key = ctx.accounts.order.key();
    let price_level_key = ctx.accounts.price_level.key();
    let previous_order_key = ctx.accounts.previous_order.key();
    let trader_key = ctx.accounts.trader.key();

    let market = &ctx.accounts.market;

    require!(
        price % market.price_tick_size == 0,
        MarketError::PriceNotOnTick
    );

    require!(
        quantity % market.quantity_lot_size == 0,
        MarketError::QuantityNotOnLot
    );

    let base_scale = 10_u128
        .checked_pow(u32::from(market.base_decimals))
        .ok_or(MarketError::OrderNotionalOverflow)?;

    let quote_notional = u128::from(price)
        .checked_mul(u128::from(quantity))
        .ok_or(MarketError::OrderNotionalOverflow)?
        .checked_div(base_scale)
        .ok_or(MarketError::OrderNotionalOverflow)?;

    require!(quote_notional > 0, MarketError::OrderNotionalTooSmall);

    let (expected_mint, locked_collateral) = match side {
        OrderSide::Ask => (market.base_mint, quantity),
        OrderSide::Bid => {
            let quote_amount = u64::try_from(quote_notional)
                .map_err(|_| error!(MarketError::OrderNotionalOverflow))?;

            (market.quote_mint, quote_amount)
        }
    };

    require_keys_eq!(
        ctx.accounts.collateral_mint.key(),
        expected_mint,
        MarketError::InvalidCollateralMint
    );

    require!(
        ctx.accounts.trader_collateral.amount >= locked_collateral,
        MarketError::InsufficientCollateral
    );

    let next_order_id = market
        .next_order_id
        .checked_add(1)
        .ok_or(MarketError::OrderIdOverflow)?;

    let next_open_order_count = market
        .open_order_count
        .checked_add(1)
        .ok_or(MarketError::OpenOrderCountOverflow)?;

    let next_level_quantity = ctx
        .accounts
        .price_level
        .total_remaining_quantity
        .checked_add(quantity)
        .ok_or(MarketError::PriceLevelQuantityOverflow)?;

    let next_level_count = ctx
        .accounts
        .price_level
        .order_count
        .checked_add(1)
        .ok_or(MarketError::PriceLevelOrderCountOverflow)?;

    token::transfer_checked(
        CpiContext::new(
            ctx.accounts.token_program.key(),
            TransferChecked {
                from: ctx.accounts.trader_collateral.to_account_info(),
                mint: ctx.accounts.collateral_mint.to_account_info(),
                to: ctx.accounts.market_vault.to_account_info(),
                authority: ctx.accounts.trader.to_account_info(),
            },
        ),
        locked_collateral,
        ctx.accounts.collateral_mint.decimals,
    )?;

    // Link the old tail to the new order.
    ctx.accounts.previous_order.next_order = Some(order_key);

    // The new order becomes the final item in the FIFO queue.
    let order = &mut ctx.accounts.order;
    order.owner = trader_key;
    order.market = market_key;
    order.order_id = ctx.accounts.market.next_order_id;
    order.side = side;
    order.price = price;
    order.price_level = price_level_key;
    order.previous_order = Some(previous_order_key);
    order.next_order = None;
    order.quantity = quantity;
    order.remaining_quantity = quantity;
    order.locked_collateral = locked_collateral;
    order.status = OrderStatus::Open;
    order.bump = ctx.bumps.order;

    // The head remains unchanged; only the tail and aggregates move.
    let price_level = &mut ctx.accounts.price_level;
    price_level.last_order = Some(order_key);
    price_level.total_remaining_quantity = next_level_quantity;
    price_level.order_count = next_level_count;

    let market = &mut ctx.accounts.market;
    market.next_order_id = next_order_id;
    market.open_order_count = next_open_order_count;

    Ok(())
}

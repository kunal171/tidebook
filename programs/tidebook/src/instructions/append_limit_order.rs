//! Appends a limit order to an existing price level.
//!
//! The current tail order is supplied explicitly and validated through
//! reciprocal links before the FIFO queue is mutated.

use anchor_lang::prelude::*;

use crate::{
    constants::{ORDER_SEED, PRICE_LEVEL_SEED, TRADER_BALANCE_SEED},
    errors::{balance, market, order},
    events::OrderPlacedEvent,
    state::{Market, MarketStatus, Order, OrderSide, OrderStatus, PriceLevel, TraderBalance},
};

#[derive(Accounts)]
#[instruction(side: OrderSide, price: u64, quantity: u64)]
pub struct AppendLimitOrder<'info> {
    #[account(mut)]
    pub trader: Signer<'info>,

    #[account(
        mut,
        constraint = market.status == MarketStatus::Active
            @ market::MarketNotActive
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
            @ order::PriceLevelMarketMismatch,
        constraint = price_level.side == side
            @ order::PriceLevelSideMismatch,
        constraint = price_level.price == price
            @ order::PriceLevelPriceMismatch
    )]
    pub price_level: Account<'info, PriceLevel>,

    #[account(
        mut,
        constraint = price_level.last_order == Some(previous_order.key())
            @ order::InvalidPriceLevelTail,
        constraint = previous_order.price_level == price_level.key()
            @ order::OrderPriceLevelMismatch,
        constraint = previous_order.status == OrderStatus::Open
            @ order::OrderNotOpen,
        constraint = previous_order.next_order.is_none()
            @ order::InvalidPriceLevelTail
    )]
    pub previous_order: Account<'info, Order>,

    /// Canonical internal ledger that funds and collateralizes this order.
    #[account(
        mut,
        seeds = [
            TRADER_BALANCE_SEED,
            market.key().as_ref(),
            trader.key().as_ref(),
        ],
        bump = trader_balance.bump,
        constraint = trader_balance.market == market.key()
            @ balance::TraderBalanceMarketMismatch,
        constraint = trader_balance.owner == trader.key()
            @ balance::TraderBalanceOwnerMismatch
    )]
    pub trader_balance: Account<'info, TraderBalance>,

    pub system_program: Program<'info, System>,
}

pub fn handle_append_limit_order(
    ctx: Context<AppendLimitOrder>,
    side: OrderSide,
    price: u64,
    quantity: u64,
) -> Result<()> {
    require!(price > 0, order::InvalidPrice);
    require!(quantity > 0, order::InvalidQuantity);

    let market_key = ctx.accounts.market.key();
    let order_key = ctx.accounts.order.key();
    let price_level_key = ctx.accounts.price_level.key();
    let previous_order_key = ctx.accounts.previous_order.key();
    let trader_key = ctx.accounts.trader.key();

    let market = &ctx.accounts.market;

    require!(
        price.is_multiple_of(market.price_tick_size),
        order::PriceNotOnTick
    );

    require!(
        quantity.is_multiple_of(market.quantity_lot_size),
        order::QuantityNotOnLot
    );

    let base_scale = 10_u128
        .checked_pow(u32::from(market.base_decimals))
        .ok_or(order::OrderNotionalOverflow)?;

    let quote_notional = u128::from(price)
        .checked_mul(u128::from(quantity))
        .ok_or(order::OrderNotionalOverflow)?
        .checked_div(base_scale)
        .ok_or(order::OrderNotionalOverflow)?;

    require!(quote_notional > 0, order::OrderNotionalTooSmall);

    let locked_collateral = match side {
        OrderSide::Ask => quantity,
        OrderSide::Bid => {
            u64::try_from(quote_notional).map_err(|_| error!(order::OrderNotionalOverflow))?
        }
    };

    // Reserving collateral is now an internal ledger transition. SPL tokens
    // remain in the market vault until an explicit withdrawal.
    let (next_free_balance, next_locked_balance) = match side {
        OrderSide::Ask => (
            ctx.accounts
                .trader_balance
                .base_free
                .checked_sub(locked_collateral)
                .ok_or(balance::InsufficientFreeBalance)?,
            ctx.accounts
                .trader_balance
                .base_locked
                .checked_add(locked_collateral)
                .ok_or(balance::LockedBalanceOverflow)?,
        ),
        OrderSide::Bid => (
            ctx.accounts
                .trader_balance
                .quote_free
                .checked_sub(locked_collateral)
                .ok_or(balance::InsufficientFreeBalance)?,
            ctx.accounts
                .trader_balance
                .quote_locked
                .checked_add(locked_collateral)
                .ok_or(balance::LockedBalanceOverflow)?,
        ),
    };

    let order_id = market.next_order_id;

    let next_order_id = order_id.checked_add(1).ok_or(order::OrderIdOverflow)?;

    let next_open_order_count = market
        .open_order_count
        .checked_add(1)
        .ok_or(market::OpenOrderCountOverflow)?;

    let next_level_quantity = ctx
        .accounts
        .price_level
        .total_remaining_quantity
        .checked_add(quantity)
        .ok_or(order::PriceLevelQuantityOverflow)?;

    let next_level_count = ctx
        .accounts
        .price_level
        .order_count
        .checked_add(1)
        .ok_or(order::PriceLevelOrderCountOverflow)?;

    match side {
        OrderSide::Ask => {
            ctx.accounts.trader_balance.base_free = next_free_balance;
            ctx.accounts.trader_balance.base_locked = next_locked_balance;
        }
        OrderSide::Bid => {
            ctx.accounts.trader_balance.quote_free = next_free_balance;
            ctx.accounts.trader_balance.quote_locked = next_locked_balance;
        }
    }

    // Link the old tail to the new order.
    ctx.accounts.previous_order.next_order = Some(order_key);

    // The new order becomes the final item in the FIFO queue.
    let order = &mut ctx.accounts.order;
    order.owner = trader_key;
    order.market = market_key;
    order.order_id = order_id;
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

    emit!(OrderPlacedEvent {
        market: market_key,
        order: order_key,
        order_id,
        owner: trader_key,
        side,
        price,
        quantity,
        locked_collateral,
        price_level: price_level_key,
    });

    Ok(())
}

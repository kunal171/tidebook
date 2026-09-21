//! Validates and records a market-local limit order.
//!
//! This milestone records intent only; token collateral is not transferred or
//! locked until the custody flow is implemented.

use anchor_lang::prelude::*;

use crate::{
    constants::ORDER_SEED,
    error::MarketError,
    state::{Market, MarketStatus, Order, OrderSide, OrderStatus},
};

#[derive(Accounts)]
pub struct PlaceLimitOrder<'info> {
    #[account(mut)]
    pub trader: Signer<'info>,

    #[account(
        mut,
        constraint = market.status == MarketStatus::Active @ MarketError::MarketNotActive
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

    pub system_program: Program<'info, System>,
}

pub fn handle_place_limit_order(
    ctx: Context<PlaceLimitOrder>,
    side: OrderSide,
    price: u64,
    quantity: u64,
) -> Result<()> {
    require!(price > 0, MarketError::InvalidPrice);
    require!(quantity > 0, MarketError::InvalidQuantity);

    let market = &mut ctx.accounts.market;
    let order = &mut ctx.accounts.order;

    require!(
        price % market.price_tick_size == 0,
        MarketError::PriceNotOnTick
    );

    require!(
        quantity % market.quantity_lot_size == 0,
        MarketError::QuantityNotOnLot
    );

    // Prices are quote atoms per whole base token, while quantities are base
    // atoms. Dividing by the base scale converts their product to quote atoms.
    let base_scale = 10_u128
        .checked_pow(u32::from(market.base_decimals))
        .ok_or(MarketError::OrderNotionalOverflow)?;

    let quote_notional = u128::from(price)
        .checked_mul(u128::from(quantity))
        .ok_or(MarketError::OrderNotionalOverflow)?
        .checked_div(base_scale)
        .ok_or(MarketError::OrderNotionalOverflow)?;

    require!(quote_notional > 0, MarketError::OrderNotionalTooSmall);

    order.owner = ctx.accounts.trader.key();
    order.market = market.key();
    order.order_id = market.next_order_id;
    order.side = side;
    order.price = price;
    order.quantity = quantity;
    order.remaining_quantity = quantity;
    order.status = OrderStatus::Open;
    order.bump = ctx.bumps.order;

    // Increment only after the order is fully initialized. Solana transaction
    // atomicity rolls both writes back if the instruction later fails.
    market.next_order_id += 1;

    Ok(())
}

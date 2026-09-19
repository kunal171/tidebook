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

    order.owner = ctx.accounts.trader.key();
    order.market = market.key();
    order.order_id = market.next_order_id;
    order.side = side;
    order.price = price;
    order.quantity = quantity;
    order.remaining_quantity = quantity;
    order.status = OrderStatus::Open;
    order.bump = ctx.bumps.order;

    market.next_order_id += 1;

    Ok(())
}

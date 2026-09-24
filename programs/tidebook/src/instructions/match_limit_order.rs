//! Executes one taker fill against the best FIFO maker.
//!
//! The first milestone supports only a complete taker fill against a maker that
//! remains open. Therefore no queue links or price levels are removed yet.

use anchor_lang::prelude::*;

use crate::{
    constants::{MARKET_SEED, ORDER_SEED, PRICE_LEVEL_SEED, TRADER_BALANCE_SEED},
    error::MarketError,
    matching::calculate_settlement,
    state::{Market, MarketStatus, Order, OrderSide, OrderStatus, PriceLevel, TraderBalance},
};

/// Accounts required to settle one incoming taker entirely against the current
/// best FIFO maker without removing the maker from the book.
///
/// Every mutable account is canonical and program-owned. In particular, the
/// FIFO-head and best-price constraints prevent a client from skipping older
/// liquidity or selecting a worse execution price.
#[derive(Accounts)]
#[instruction(taker_side: OrderSide, limit_price: u64, quantity: u64)]
pub struct MatchLimitOrder<'info> {
    pub taker: Signer<'info>,

    #[account(
        mut,
        seeds = [
            MARKET_SEED,
            market.base_mint.as_ref(),
            market.quote_mint.as_ref(),
        ],
        bump = market.bump,
        constraint = market.status == MarketStatus::Active
            @ MarketError::MarketNotActive
    )]
    pub market: Box<Account<'info, Market>>,

    #[account(
        mut,
        seeds = [
            ORDER_SEED,
            market.key().as_ref(),
            maker_order.order_id.to_le_bytes().as_ref(),
        ],
        bump = maker_order.bump,
        has_one = market @ MarketError::OrderMarketMismatch,
        constraint = maker_order.status == OrderStatus::Open
            @ MarketError::OrderNotOpen,
        constraint = maker_order.owner != taker.key()
            @ MarketError::SelfTradeNotAllowed
    )]
    pub maker_order: Box<Account<'info, Order>>,

    #[account(
        mut,
        seeds = [
            PRICE_LEVEL_SEED,
            market.key().as_ref(),
            maker_order.side.seed(),
            maker_order.price.to_le_bytes().as_ref(),
        ],
        bump = maker_price_level.bump,
        constraint = maker_order.price_level == maker_price_level.key()
            @ MarketError::OrderPriceLevelMismatch,
        constraint = maker_price_level.market == market.key()
            @ MarketError::PriceLevelMarketMismatch,
        constraint = maker_price_level.side == maker_order.side
            @ MarketError::PriceLevelSideMismatch,
        constraint = maker_price_level.price == maker_order.price
            @ MarketError::PriceLevelPriceMismatch,
        constraint = maker_price_level.first_order == Some(maker_order.key())
            @ MarketError::MakerNotFifoHead,
        constraint = match maker_order.side {
            OrderSide::Ask => market.best_ask == Some(maker_order.price),
            OrderSide::Bid => market.best_bid == Some(maker_order.price),
        } @ MarketError::MakerNotAtBestPrice
    )]
    pub maker_price_level: Box<Account<'info, PriceLevel>>,

    #[account(
        mut,
        seeds = [
            TRADER_BALANCE_SEED,
            market.key().as_ref(),
            maker_order.owner.as_ref(),
        ],
        bump = maker_balance.bump,
        constraint = maker_balance.market == market.key()
            @ MarketError::TraderBalanceMarketMismatch,
        constraint = maker_balance.owner == maker_order.owner
            @ MarketError::TraderBalanceOwnerMismatch
    )]
    pub maker_balance: Box<Account<'info, TraderBalance>>,

    #[account(
        mut,
        seeds = [
            TRADER_BALANCE_SEED,
            market.key().as_ref(),
            taker.key().as_ref(),
        ],
        bump = taker_balance.bump,
        constraint = taker_balance.market == market.key()
            @ MarketError::TraderBalanceMarketMismatch,
        constraint = taker_balance.owner == taker.key()
            @ MarketError::TraderBalanceOwnerMismatch
    )]
    pub taker_balance: Box<Account<'info, TraderBalance>>,
}

pub fn handle_match_limit_order(
    ctx: Context<MatchLimitOrder>,
    taker_side: OrderSide,
    limit_price: u64,
    quantity: u64,
) -> Result<()> {
    require!(limit_price > 0, MarketError::InvalidPrice);
    require!(quantity > 0, MarketError::InvalidQuantity);

    let market = &ctx.accounts.market;

    require!(
        limit_price.is_multiple_of(market.price_tick_size),
        MarketError::PriceNotOnTick
    );

    require!(
        quantity.is_multiple_of(market.quantity_lot_size),
        MarketError::QuantityNotOnLot
    );

    // This first on-chain slice deliberately avoids FIFO and level removal.
    // The taker must be completely filled while the maker remains open.
    require!(
        quantity < ctx.accounts.maker_order.remaining_quantity,
        MarketError::MakerMustRemainOpen
    );

    let plan = calculate_settlement(
        taker_side,
        limit_price,
        quantity,
        ctx.accounts.maker_order.side,
        ctx.accounts.maker_order.price,
        ctx.accounts.maker_order.remaining_quantity,
        ctx.accounts.maker_order.locked_collateral,
        market.base_decimals,
    )?;

    require!(
        plan.taker_fully_filled && !plan.maker_fully_filled,
        MarketError::MakerMustRemainOpen
    );

    // Calculate every fallible result before mutating any account. Solana would
    // roll the instruction back after an error, but precomputation keeps the
    // settlement transition auditable and prevents accidental partial writes
    // if this logic is later extracted or reused.
    let next_order_remaining = ctx
        .accounts
        .maker_order
        .remaining_quantity
        .checked_sub(plan.fill.base_quantity)
        .ok_or(MarketError::PriceLevelQuantityUnderflow)?;

    let next_order_collateral = ctx
        .accounts
        .maker_order
        .locked_collateral
        .checked_sub(plan.maker_locked_debit)
        .ok_or(MarketError::InvalidMakerCollateral)?;

    let next_level_quantity = ctx
        .accounts
        .maker_price_level
        .total_remaining_quantity
        .checked_sub(plan.fill.base_quantity)
        .ok_or(MarketError::PriceLevelQuantityUnderflow)?;

    match taker_side {
        OrderSide::Bid => {
            // The taker pays quote from free balance and receives base. The
            // resting ask releases the same base quantity from locked balance
            // and receives the maker-price quote proceeds as free balance.
            let next_taker_quote_free = ctx
                .accounts
                .taker_balance
                .quote_free
                .checked_sub(plan.fill.quote_quantity)
                .ok_or(MarketError::InsufficientFreeBalance)?;

            let next_taker_base_free = ctx
                .accounts
                .taker_balance
                .base_free
                .checked_add(plan.fill.base_quantity)
                .ok_or(MarketError::FreeBalanceOverflow)?;

            let next_maker_base_locked = ctx
                .accounts
                .maker_balance
                .base_locked
                .checked_sub(plan.maker_locked_debit)
                .ok_or(MarketError::LockedBalanceUnderflow)?;

            let next_maker_quote_free = ctx
                .accounts
                .maker_balance
                .quote_free
                .checked_add(plan.fill.quote_quantity)
                .ok_or(MarketError::FreeBalanceOverflow)?;

            ctx.accounts.taker_balance.quote_free = next_taker_quote_free;
            ctx.accounts.taker_balance.base_free = next_taker_base_free;
            ctx.accounts.maker_balance.base_locked = next_maker_base_locked;
            ctx.accounts.maker_balance.quote_free = next_maker_quote_free;
        }

        OrderSide::Ask => {
            // The taker pays base and receives quote. The resting bid releases
            // quote collateral and receives base. Rounding refunds are zero in
            // this partial-maker milestone, but applying the plan here keeps
            // the transition compatible with the later final-fill path.
            let next_taker_base_free = ctx
                .accounts
                .taker_balance
                .base_free
                .checked_sub(plan.fill.base_quantity)
                .ok_or(MarketError::InsufficientFreeBalance)?;

            let next_taker_quote_free = ctx
                .accounts
                .taker_balance
                .quote_free
                .checked_add(plan.fill.quote_quantity)
                .ok_or(MarketError::FreeBalanceOverflow)?;

            let next_maker_quote_locked = ctx
                .accounts
                .maker_balance
                .quote_locked
                .checked_sub(plan.maker_locked_debit)
                .ok_or(MarketError::LockedBalanceUnderflow)?;

            let next_maker_base_free = ctx
                .accounts
                .maker_balance
                .base_free
                .checked_add(plan.fill.base_quantity)
                .ok_or(MarketError::FreeBalanceOverflow)?;

            let next_maker_quote_free = ctx
                .accounts
                .maker_balance
                .quote_free
                .checked_add(plan.maker_quote_refund)
                .ok_or(MarketError::FreeBalanceOverflow)?;

            ctx.accounts.taker_balance.base_free = next_taker_base_free;
            ctx.accounts.taker_balance.quote_free = next_taker_quote_free;
            ctx.accounts.maker_balance.quote_locked = next_maker_quote_locked;
            ctx.accounts.maker_balance.base_free = next_maker_base_free;
            ctx.accounts.maker_balance.quote_free = next_maker_quote_free;
        }
    }

    // The maker remains Open and stays at the FIFO head. Therefore only its
    // remaining quantities and the level aggregate change; order count, links,
    // market best pointers, and open-order count remain unchanged.
    ctx.accounts.maker_order.remaining_quantity = next_order_remaining;
    ctx.accounts.maker_order.locked_collateral = next_order_collateral;
    ctx.accounts.maker_price_level.total_remaining_quantity = next_level_quantity;

    Ok(())
}

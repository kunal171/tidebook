//! Creates a distinct price level and its first FIFO order atomically.
//!
//! Solana programs cannot scan every account belonging to a program, so the
//! client must discover and supply the adjacent price levels. Those accounts
//! are only untrusted routing hints: this instruction verifies their canonical
//! PDAs, market, side, ordering, and reciprocal links before changing state.
//!
//! The two optional neighbors encode all insertion positions:
//! - no neighbors: the first level on an empty side;
//! - only a worse neighbor: a new best level;
//! - both neighbors: a middle level;
//! - only a better neighbor: a new worst level.
//!
//! Level creation, collateral custody, neighbor rewiring, order creation, and
//! market counters intentionally happen in one instruction. Splitting them
//! across transactions could leave an empty or partially linked active level.

use anchor_lang::prelude::*;

use crate::{
    constants::{ORDER_SEED, PRICE_LEVEL_SEED, TRADER_BALANCE_SEED},
    error::MarketError,
    pda::derive_price_level_pda,
    state::{Market, MarketStatus, Order, OrderSide, OrderStatus, PriceLevel, TraderBalance},
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
            @ MarketError::TraderBalanceMarketMismatch,
        constraint = trader_balance.owner == trader.key()
            @ MarketError::TraderBalanceOwnerMismatch
    )]
    pub trader_balance: Account<'info, TraderBalance>,

    pub system_program: Program<'info, System>,

    /// Adjacent level with higher matching priority, or `None` at the best edge.
    ///
    /// Optional accounts cannot carry a static seed constraint for every
    /// insertion shape, so the handler derives and checks their PDAs manually.
    #[account(mut)]
    pub better_level: Option<Account<'info, PriceLevel>>,

    /// Adjacent level with lower matching priority, or `None` at the worst edge.
    #[account(mut)]
    pub worse_level: Option<Account<'info, PriceLevel>>,
}

pub fn handle_insert_limit_order(
    ctx: Context<InsertLimitOrder>,
    side: OrderSide,
    price: u64,
    quantity: u64,
) -> Result<()> {
    require!(price > 0, MarketError::InvalidPrice);
    require!(quantity > 0, MarketError::InvalidQuantity);

    let market_key = ctx.accounts.market.key();
    let order_key = ctx.accounts.order.key();
    let price_level_key = ctx.accounts.price_level.key();
    let trader_key = ctx.accounts.trader.key();

    let market = &ctx.accounts.market;

    // Prices and quantities are raw integers. Enforcing the market grid here
    // prevents multiple representations of the same economic price or size
    // and keeps later matching arithmetic deterministic.
    require!(
        price.is_multiple_of(market.price_tick_size),
        MarketError::PriceNotOnTick
    );

    require!(
        quantity.is_multiple_of(market.quantity_lot_size),
        MarketError::QuantityNotOnLot
    );

    // The market stores only the best price for O(1) matching entry. It does
    // not store a worst pointer; clients reach the worst by following each
    // level's `worse_price` link off-chain.
    let current_best_price = match side {
        OrderSide::Bid => market.best_bid,
        OrderSide::Ask => market.best_ask,
    };

    match (
        current_best_price,
        ctx.accounts.better_level.as_ref(),
        ctx.accounts.worse_level.as_ref(),
    ) {
        // Empty side: accepting any neighbor would allow a detached list whose
        // head disagrees with the market's best pointer.
        (None, None, None) => {}

        // New best: the sole supplied neighbor must be the level referenced by
        // the market. Its missing better link proves it is currently the head.
        (Some(current_best), None, Some(worse)) => {
            validate_level(worse, market_key, side)?;

            require!(
                worse.price == current_best,
                MarketError::BestPriceLevelMismatch
            );

            require!(
                worse.better_price.is_none(),
                MarketError::BestPriceLevelMismatch
            );

            let correctly_ordered = match side {
                OrderSide::Bid => price > worse.price,
                OrderSide::Ask => price < worse.price,
            };

            require!(correctly_ordered, MarketError::InvalidPriceLevelOrdering);
        }

        // Middle insertion: two canonical levels are insufficient by
        // themselves; their reciprocal links must prove they are adjacent.
        // Otherwise a client could skip a level and silently detach it.
        (Some(_), Some(better), Some(worse)) => {
            validate_level(better, market_key, side)?;
            validate_level(worse, market_key, side)?;

            require!(
                better.worse_price == Some(worse.price),
                MarketError::InvalidPriceLevelNeighbors
            );

            require!(
                worse.better_price == Some(better.price),
                MarketError::InvalidPriceLevelNeighbors
            );

            let correctly_ordered = match side {
                OrderSide::Bid => better.price > price && price > worse.price,
                OrderSide::Ask => better.price < price && price < worse.price,
            };

            require!(correctly_ordered, MarketError::InvalidPriceLevelOrdering);
        }

        // New worst: because Market intentionally has no worst pointer, the
        // terminal better level proves the boundary with `worse_price = None`.
        (Some(_), Some(better), None) => {
            validate_level(better, market_key, side)?;

            require!(
                better.worse_price.is_none(),
                MarketError::InvalidPriceLevelNeighbors
            );

            let correctly_ordered = match side {
                OrderSide::Bid => better.price > price,
                OrderSide::Ask => better.price < price,
            };

            require!(correctly_ordered, MarketError::InvalidPriceLevelOrdering);
        }
        // A neighbor was supplied for an empty side, or omitted for a
        // non-empty side. Both indicate stale or malformed client state.
        _ => return err!(MarketError::InvalidPriceLevelNeighbors),
    }

    // Bid collateral is quote notional, while ask collateral is base quantity.
    // Intermediate u128 arithmetic prevents multiplication overflow before the
    // final checked conversion back to the SPL Token program's u64 amount.
    let base_scale = 10_u128
        .checked_pow(u32::from(market.base_decimals))
        .ok_or(MarketError::OrderNotionalOverflow)?;

    let quote_notional = u128::from(price)
        .checked_mul(u128::from(quantity))
        .ok_or(MarketError::OrderNotionalOverflow)?
        .checked_div(base_scale)
        .ok_or(MarketError::OrderNotionalOverflow)?;

    require!(quote_notional > 0, MarketError::OrderNotionalTooSmall);

    let locked_collateral = match side {
        OrderSide::Ask => quantity,
        OrderSide::Bid => {
            u64::try_from(quote_notional).map_err(|_| error!(MarketError::OrderNotionalOverflow))?
        }
    };

    // Orders reserve funds already deposited into the market vault. No token
    // CPI occurs here; the ledger and book index mutate in one instruction.
    let (next_free_balance, next_locked_balance) = match side {
        OrderSide::Ask => (
            ctx.accounts
                .trader_balance
                .base_free
                .checked_sub(locked_collateral)
                .ok_or(MarketError::InsufficientFreeBalance)?,
            ctx.accounts
                .trader_balance
                .base_locked
                .checked_add(locked_collateral)
                .ok_or(MarketError::LockedBalanceOverflow)?,
        ),
        OrderSide::Bid => (
            ctx.accounts
                .trader_balance
                .quote_free
                .checked_sub(locked_collateral)
                .ok_or(MarketError::InsufficientFreeBalance)?,
            ctx.accounts
                .trader_balance
                .quote_locked
                .checked_add(locked_collateral)
                .ok_or(MarketError::LockedBalanceOverflow)?,
        ),
    };

    let order_id = market.next_order_id;

    let next_order_id = order_id
        .checked_add(1)
        .ok_or(MarketError::OrderIdOverflow)?;

    let next_open_order_count = market
        .open_order_count
        .checked_add(1)
        .ok_or(MarketError::OpenOrderCountOverflow)?;

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

    // Snapshot neighbor prices before borrowing the optional accounts mutably.
    // Storing prices rather than addresses keeps a level compact; canonical
    // addresses are deterministically re-derived from market, side, and price.
    let better_price = ctx.accounts.better_level.as_ref().map(|level| level.price);

    let worse_price = ctx.accounts.worse_level.as_ref().map(|level| level.price);

    // Splice the new level into the doubly linked price index. Each supplied
    // neighbor was validated above, so these writes preserve reciprocity.
    if let Some(better) = ctx.accounts.better_level.as_mut() {
        better.worse_price = Some(price);
    }

    if let Some(worse) = ctx.accounts.worse_level.as_mut() {
        worse.better_price = Some(price);
    }

    // A newly created level begins with a one-element FIFO queue: its first and
    // last pointers both reference the order created by this instruction.
    let price_level = &mut ctx.accounts.price_level;
    price_level.market = market_key;
    price_level.side = side;
    price_level.price = price;
    price_level.better_price = better_price;
    price_level.worse_price = worse_price;
    price_level.first_order = Some(order_key);
    price_level.last_order = Some(order_key);
    price_level.total_remaining_quantity = quantity;
    price_level.order_count = 1;
    price_level.rent_payer = trader_key;
    price_level.bump = ctx.bumps.price_level;

    // Orders at distinct prices are connected through PriceLevel accounts;
    // previous/next order pointers are reserved for FIFO at this exact price.
    let order = &mut ctx.accounts.order;
    order.owner = trader_key;
    order.market = market_key;
    order.order_id = order_id;
    order.side = side;
    order.price = price;
    order.price_level = price_level_key;
    order.previous_order = None;
    order.next_order = None;
    order.quantity = quantity;
    order.remaining_quantity = quantity;
    order.locked_collateral = locked_collateral;
    order.status = OrderStatus::Open;
    order.bump = ctx.bumps.order;

    let market = &mut ctx.accounts.market;

    // Only an insertion at the head changes the market entry point. Middle and
    // worst insertions deliberately leave the best pointer untouched.
    if ctx.accounts.better_level.is_none() {
        match side {
            OrderSide::Bid => market.best_bid = Some(price),
            OrderSide::Ask => market.best_ask = Some(price),
        }
    }

    market.next_order_id = next_order_id;
    market.open_order_count = next_open_order_count;

    Ok(())
}

/// Validates an optional neighbor supplied by the client.
///
/// Account ownership and deserialization prove only that this is a Tidebook
/// `PriceLevel`. The stored market/side fields and the re-derived PDA establish
/// that it is the unique canonical level eligible for this particular list.
fn validate_level(level: &Account<PriceLevel>, market: Pubkey, side: OrderSide) -> Result<()> {
    require!(
        level.market == market,
        MarketError::PriceLevelMarketMismatch
    );

    require!(level.side == side, MarketError::PriceLevelSideMismatch);

    let (expected, _) = derive_price_level_pda(&crate::ID, &market, side, level.price);

    require_keys_eq!(level.key(), expected, MarketError::NoncanonicalPriceLevel);

    Ok(())
}

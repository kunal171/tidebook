//! Cancels an open order and releases its collateral back to free balance.
//!
//! Market status is intentionally not checked so owners can cancel while a
//! market is paused. Tokens remain in the market vault until withdrawal.

use anchor_lang::prelude::*;

use crate::{
    constants::{ORDER_SEED, PRICE_LEVEL_SEED, TRADER_BALANCE_SEED},
    error::MarketError,
    pda::derive_price_level_pda,
    state::{Market, Order, OrderSide, OrderStatus, PriceLevel, TraderBalance},
};

/// Cancellation can touch two FIFO neighbors and two price-level neighbors.
///
/// Deserialized accounts are boxed to keep Anchor's generated parser below
/// Solana's 4 KiB stack-frame limit. The small heap cost preserves one atomic
/// refund, queue repair, level unlink, and account-closure instruction.
#[derive(Accounts)]
#[instruction(order_id: u64)]
pub struct CancelLimitOrder<'info> {
    pub owner: Signer<'info>,

    #[account(mut)]
    pub market: Box<Account<'info, Market>>,

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
    pub order: Box<Account<'info, Order>>,

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
    pub price_level: Box<Account<'info, PriceLevel>>,

    /// Older FIFO neighbor. None when canceling the queue head.
    #[account(mut)]
    pub previous_order: Option<Box<Account<'info, Order>>>,

    /// Newer FIFO neighbor. None when canceling the queue tail.
    #[account(mut)]
    pub next_order: Option<Box<Account<'info, Order>>>,

    /// Higher-priority price level, required only when this level becomes empty.
    #[account(mut)]
    pub better_level: Option<Box<Account<'info, PriceLevel>>>,

    /// Lower-priority price level, required only when this level becomes empty.
    #[account(mut)]
    pub worse_level: Option<Box<Account<'info, PriceLevel>>>,

    /// CHECK: Must equal the stored price-level rent payer when closing the level.
    #[account(mut)]
    pub level_rent_recipient: Option<UncheckedAccount<'info>>,

    /// Canonical ledger receiving the released collateral.
    #[account(
        mut,
        seeds = [
            TRADER_BALANCE_SEED,
            market.key().as_ref(),
            owner.key().as_ref(),
        ],
        bump = trader_balance.bump,
        constraint = trader_balance.market == market.key()
            @ MarketError::TraderBalanceMarketMismatch,
        constraint = trader_balance.owner == owner.key()
            @ MarketError::TraderBalanceOwnerMismatch
    )]
    pub trader_balance: Box<Account<'info, TraderBalance>>,
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

    let level_price = ctx.accounts.price_level.price;
    let level_better_price = ctx.accounts.price_level.better_price;
    let level_worse_price = ctx.accounts.price_level.worse_price;
    let level_first_order = ctx.accounts.price_level.first_order;
    let level_last_order = ctx.accounts.price_level.last_order;

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

    let market_key = ctx.accounts.market.key();

    // Cancellation releases only the amount still collateralizing this order.
    // Tokens stay in custody, making cancellation a pure ledger and index update.
    let (next_free_balance, next_locked_balance) = match order_side {
        OrderSide::Ask => (
            ctx.accounts
                .trader_balance
                .base_free
                .checked_add(locked_collateral)
                .ok_or(MarketError::FreeBalanceOverflow)?,
            ctx.accounts
                .trader_balance
                .base_locked
                .checked_sub(locked_collateral)
                .ok_or(MarketError::LockedBalanceUnderflow)?,
        ),
        OrderSide::Bid => (
            ctx.accounts
                .trader_balance
                .quote_free
                .checked_add(locked_collateral)
                .ok_or(MarketError::FreeBalanceOverflow)?,
            ctx.accounts
                .trader_balance
                .quote_locked
                .checked_sub(locked_collateral)
                .ok_or(MarketError::LockedBalanceUnderflow)?,
        ),
    };

    // Calculate all fallible counter changes before applying ledger or book mutations.
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

    let removes_price_level = next_level_order_count == 0;

    if !removes_price_level {
        require!(
            ctx.accounts.better_level.is_none()
                && ctx.accounts.worse_level.is_none()
                && ctx.accounts.level_rent_recipient.is_none(),
            MarketError::InvalidPriceLevelNeighbors
        );
    }

    if removes_price_level {
        require!(
            order_previous.is_none() && order_next.is_none(),
            MarketError::InvalidPriceLevelEndpoints
        );

        require!(
            level_first_order == Some(order_key) && level_last_order == Some(order_key),
            MarketError::InvalidPriceLevelEndpoints
        );

        require!(
            next_level_quantity == 0,
            MarketError::InvalidPriceLevelAggregate
        );

        if level_better_price.is_none() {
            let current_best = match order_side {
                OrderSide::Bid => ctx.accounts.market.best_bid,
                OrderSide::Ask => ctx.accounts.market.best_ask,
            };

            require!(
                current_best == Some(level_price),
                MarketError::BestPriceLevelMismatch
            );
        }

        let rent_recipient = ctx
            .accounts
            .level_rent_recipient
            .as_ref()
            .ok_or(MarketError::InvalidPriceLevelRentRecipient)?;

        require_keys_eq!(
            rent_recipient.key(),
            ctx.accounts.price_level.rent_payer,
            MarketError::InvalidPriceLevelRentRecipient
        );

        let expected_better_level = level_better_price
            .map(|price| derive_price_level_pda(ctx.program_id, &market_key, order_side, price).0);

        let expected_worse_level = level_worse_price
            .map(|price| derive_price_level_pda(ctx.program_id, &market_key, order_side, price).0);

        let supplied_better_level = ctx.accounts.better_level.as_ref().map(|level| level.key());

        let supplied_worse_level = ctx.accounts.worse_level.as_ref().map(|level| level.key());

        require!(
            supplied_better_level == expected_better_level
                && supplied_worse_level == expected_worse_level,
            MarketError::InvalidPriceLevelNeighbors
        );
    }

    if let (Some(better_level), Some(expected_price)) =
        (ctx.accounts.better_level.as_ref(), level_better_price)
    {
        require_keys_eq!(
            better_level.market,
            market_key,
            MarketError::PriceLevelMarketMismatch
        );

        require!(
            better_level.side == order_side
                && better_level.price == expected_price
                && better_level.worse_price == Some(level_price),
            MarketError::InvalidPriceLevelNeighbors
        );
    }

    if let (Some(worse_level), Some(expected_price)) =
        (ctx.accounts.worse_level.as_ref(), level_worse_price)
    {
        require_keys_eq!(
            worse_level.market,
            market_key,
            MarketError::PriceLevelMarketMismatch
        );

        require!(
            worse_level.side == order_side
                && worse_level.price == expected_price
                && worse_level.better_price == Some(level_price),
            MarketError::InvalidPriceLevelNeighbors
        );
    }

    match order_side {
        OrderSide::Ask => {
            ctx.accounts.trader_balance.base_free = next_free_balance;
            ctx.accounts.trader_balance.base_locked = next_locked_balance;
        }
        OrderSide::Bid => {
            ctx.accounts.trader_balance.quote_free = next_free_balance;
            ctx.accounts.trader_balance.quote_locked = next_locked_balance;
        }
    }

    // Splice the node out in O(1). Missing neighbors identify head or tail; when
    // both are missing, the order is the sole FIFO member and its level is
    // unlinked and closed later in this transaction.
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

    if removes_price_level {
        // Connect the higher-priority level directly to the lower-priority level.
        if let Some(better_level) = ctx.accounts.better_level.as_mut() {
            better_level.worse_price = level_worse_price;
        } else {
            // The removed level was the best level.
            match order_side {
                OrderSide::Bid => {
                    ctx.accounts.market.best_bid = level_worse_price;
                }
                OrderSide::Ask => {
                    ctx.accounts.market.best_ask = level_worse_price;
                }
            }
        }

        // Repair the reciprocal link from the lower-priority level.
        if let Some(worse_level) = ctx.accounts.worse_level.as_mut() {
            worse_level.better_price = level_better_price;
        }
    }

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

    if removes_price_level {
        let rent_recipient = ctx
            .accounts
            .level_rent_recipient
            .as_ref()
            .ok_or(MarketError::InvalidPriceLevelRentRecipient)?
            .to_account_info();

        // The level is closed only after all links and counters are repaired.
        // Any later error still rolls the entire transaction back atomically.
        ctx.accounts.price_level.close(rent_recipient)?;
    }

    Ok(())
}

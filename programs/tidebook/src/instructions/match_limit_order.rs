//! Executes one bounded taker fill against the best FIFO maker.
//!
//! A transaction touches exactly one maker. The maker may remain partially
//! open or be filled and removed from its FIFO queue. If that was the final
//! order at the best price, the empty level is unlinked and closed atomically.
//! A larger taker remainder is intentionally not persisted by this instruction;
//! the client can match another maker or post the remainder in a later step.

use anchor_lang::prelude::*;

use crate::{
    constants::{MARKET_SEED, ORDER_SEED, PRICE_LEVEL_SEED, TRADER_BALANCE_SEED},
    errors::{balance, market, matching, order},
    events::FillEvent,
    matching::calculate_settlement,
    pda::derive_price_level_pda,
    state::{Market, MarketStatus, Order, OrderSide, OrderStatus, PriceLevel, TraderBalance},
};

/// Accounts required to settle against one current best FIFO maker.
///
/// Every mutable account is canonical and program-owned. In particular, the
/// FIFO-head and best-price constraints prevent a client from skipping older
/// liquidity or selecting a worse execution price. Removal accounts are
/// optional because partial fills do not change either linked list.
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
            @ market::MarketNotActive
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
        has_one = market @ order::OrderMarketMismatch,
        constraint = maker_order.status == OrderStatus::Open
            @ order::OrderNotOpen,
        constraint = maker_order.owner != taker.key()
            @ matching::SelfTradeNotAllowed
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
            @ order::OrderPriceLevelMismatch,
        constraint = maker_price_level.market == market.key()
            @ order::PriceLevelMarketMismatch,
        constraint = maker_price_level.side == maker_order.side
            @ order::PriceLevelSideMismatch,
        constraint = maker_price_level.price == maker_order.price
            @ order::PriceLevelPriceMismatch,
        constraint = maker_price_level.first_order == Some(maker_order.key())
            @ matching::MakerNotFifoHead,
        constraint = match maker_order.side {
            OrderSide::Ask => market.best_ask == Some(maker_order.price),
            OrderSide::Bid => market.best_bid == Some(maker_order.price),
        } @ matching::MakerNotAtBestPrice
    )]
    pub maker_price_level: Box<Account<'info, PriceLevel>>,

    /// New FIFO head when the filled maker has another order behind it.
    #[account(mut)]
    pub next_order: Option<Box<Account<'info, Order>>>,

    /// Next lower-priority price level when the maker's level becomes empty.
    #[account(mut)]
    pub worse_level: Option<Box<Account<'info, PriceLevel>>>,

    /// Receives the reclaimed rent when an empty price level is closed.
    ///
    /// CHECK: The handler verifies this address against
    /// `maker_price_level.rent_payer`.
    #[account(mut)]
    pub level_rent_recipient: Option<UncheckedAccount<'info>>,

    #[account(
        mut,
        seeds = [
            TRADER_BALANCE_SEED,
            market.key().as_ref(),
            maker_order.owner.as_ref(),
        ],
        bump = maker_balance.bump,
        constraint = maker_balance.market == market.key()
            @ balance::TraderBalanceMarketMismatch,
        constraint = maker_balance.owner == maker_order.owner
            @ balance::TraderBalanceOwnerMismatch
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
            @ balance::TraderBalanceMarketMismatch,
        constraint = taker_balance.owner == taker.key()
            @ balance::TraderBalanceOwnerMismatch
    )]
    pub taker_balance: Box<Account<'info, TraderBalance>>,
}

pub fn handle_match_limit_order(
    ctx: Context<MatchLimitOrder>,
    taker_side: OrderSide,
    limit_price: u64,
    quantity: u64,
) -> Result<()> {
    require!(limit_price > 0, order::InvalidPrice);
    require!(quantity > 0, order::InvalidQuantity);

    let market = &ctx.accounts.market;

    require!(
        limit_price.is_multiple_of(market.price_tick_size),
        order::PriceNotOnTick
    );

    require!(
        quantity.is_multiple_of(market.quantity_lot_size),
        order::QuantityNotOnLot
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

    let taker_remaining_quantity = quantity
        .checked_sub(plan.fill.base_quantity)
        .ok_or(order::InvalidQuantity)?;

    // Calculate every fallible result before mutating any account. Solana would
    // roll the instruction back after an error, but precomputation keeps the
    // settlement transition auditable and prevents accidental partial writes
    // if this logic is later extracted or reused.
    let next_order_remaining = ctx
        .accounts
        .maker_order
        .remaining_quantity
        .checked_sub(plan.fill.base_quantity)
        .ok_or(order::PriceLevelQuantityUnderflow)?;

    let next_order_collateral = ctx
        .accounts
        .maker_order
        .locked_collateral
        .checked_sub(plan.maker_locked_debit)
        .ok_or(matching::InvalidMakerCollateral)?;

    let next_level_quantity = ctx
        .accounts
        .maker_price_level
        .total_remaining_quantity
        .checked_sub(plan.fill.base_quantity)
        .ok_or(order::PriceLevelQuantityUnderflow)?;

    // Snapshot relationship fields before taking mutable borrows below. These
    // values are the program-owned source of truth; optional accounts supplied
    // by the client are only accepted when they match these stored links.
    let market_key = ctx.accounts.market.key();
    let maker_key = ctx.accounts.maker_order.key();
    let maker_next = ctx.accounts.maker_order.next_order;
    let level_key = ctx.accounts.maker_price_level.key();
    let level_price = ctx.accounts.maker_price_level.price;
    let level_worse_price = ctx.accounts.maker_price_level.worse_price;
    let level_last_order = ctx.accounts.maker_price_level.last_order;
    let maker_order_id = ctx.accounts.maker_order.order_id;
    let maker_owner = ctx.accounts.maker_order.owner;
    let maker_side = ctx.accounts.maker_order.side;
    let taker_key = ctx.accounts.taker.key();

    // A partial fill changes quantities only. Counters are decremented exactly
    // once when the maker transitions from Open to Filled.
    let next_level_order_count = if plan.maker_fully_filled {
        ctx.accounts
            .maker_price_level
            .order_count
            .checked_sub(1)
            .ok_or(order::PriceLevelOrderCountUnderflow)?
    } else {
        ctx.accounts.maker_price_level.order_count
    };

    let next_open_order_count = if plan.maker_fully_filled {
        ctx.accounts
            .market
            .open_order_count
            .checked_sub(1)
            .ok_or(market::OpenOrderCountUnderflow)?
    } else {
        ctx.accounts.market.open_order_count
    };

    let removes_price_level = plan.maker_fully_filled && next_level_order_count == 0;

    // Validate the complete book transition before moving any internal funds.
    // This keeps malformed neighbor hints from reaching the mutation phase.
    if plan.maker_fully_filled {
        // The maker is already constrained to be `first_order`; a valid queue
        // head therefore cannot point to an older order.
        require!(
            ctx.accounts.maker_order.previous_order.is_none(),
            order::BrokenOrderQueueLink
        );

        let supplied_next = ctx.accounts.next_order.as_ref().map(|order| order.key());

        require!(supplied_next == maker_next, order::InvalidOrderNeighbor);

        if removes_price_level {
            // An empty level must have contained only this maker. Otherwise
            // closing it would orphan another live order.
            require!(
                maker_next.is_none() && level_last_order == Some(maker_key),
                order::InvalidPriceLevelEndpoints
            );

            require!(next_level_quantity == 0, order::InvalidPriceLevelAggregate);

            // Matching always consumes the best level, so it cannot have a
            // higher-priority predecessor.
            require!(
                ctx.accounts.maker_price_level.better_price.is_none(),
                order::BestPriceLevelMismatch
            );

            let expected_worse = level_worse_price.map(|price| {
                derive_price_level_pda(
                    ctx.program_id,
                    &market_key,
                    ctx.accounts.maker_order.side,
                    price,
                )
                .0
            });

            let supplied_worse = ctx.accounts.worse_level.as_ref().map(|level| level.key());

            require!(
                supplied_worse == expected_worse,
                order::InvalidPriceLevelNeighbors
            );

            // Price-level rent returns to its recorded creator, not to the
            // taker who happened to consume the final order.
            let rent_recipient = ctx
                .accounts
                .level_rent_recipient
                .as_ref()
                .ok_or(order::InvalidPriceLevelRentRecipient)?;

            require_keys_eq!(
                rent_recipient.key(),
                ctx.accounts.maker_price_level.rent_payer,
                order::InvalidPriceLevelRentRecipient
            );

            if let (Some(worse_level), Some(expected_price)) =
                (ctx.accounts.worse_level.as_ref(), level_worse_price)
            {
                require_keys_eq!(
                    worse_level.market,
                    market_key,
                    order::PriceLevelMarketMismatch
                );

                require!(
                    worse_level.side == ctx.accounts.maker_order.side
                        && worse_level.price == expected_price
                        && worse_level.better_price == Some(level_price),
                    order::InvalidPriceLevelNeighbors
                );
            }
        } else {
            // The filled maker was the queue head but not its only member. The
            // stored successor must be supplied so its reciprocal link can be
            // cleared and it can become the new head in O(1).
            let next_order = ctx
                .accounts
                .next_order
                .as_ref()
                .ok_or(order::InvalidOrderNeighbor)?;

            require!(next_order.status == OrderStatus::Open, order::OrderNotOpen);

            require_keys_eq!(next_order.market, market_key, order::OrderMarketMismatch);

            require_keys_eq!(
                next_order.price_level,
                level_key,
                order::OrderPriceLevelMismatch
            );

            require!(
                next_order.key() != maker_key
                    && next_order.side == ctx.accounts.maker_order.side
                    && next_order.price == level_price
                    && next_order.previous_order == Some(maker_key),
                order::BrokenOrderQueueLink
            );

            require!(next_level_quantity > 0, order::InvalidPriceLevelAggregate);

            require!(
                ctx.accounts.worse_level.is_none() && ctx.accounts.level_rent_recipient.is_none(),
                order::InvalidPriceLevelNeighbors
            );
        }
    } else {
        // Partial fills preserve every queue and level link. Rejecting unused
        // writable accounts reduces ambiguity and keeps the account contract
        // explicit for clients.
        require!(
            ctx.accounts.next_order.is_none(),
            order::InvalidOrderNeighbor
        );

        require!(
            ctx.accounts.worse_level.is_none() && ctx.accounts.level_rent_recipient.is_none(),
            order::InvalidPriceLevelNeighbors
        );
    }

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
                .ok_or(balance::InsufficientFreeBalance)?;

            let next_taker_base_free = ctx
                .accounts
                .taker_balance
                .base_free
                .checked_add(plan.fill.base_quantity)
                .ok_or(balance::FreeBalanceOverflow)?;

            let next_maker_base_locked = ctx
                .accounts
                .maker_balance
                .base_locked
                .checked_sub(plan.maker_locked_debit)
                .ok_or(balance::LockedBalanceUnderflow)?;

            let next_maker_quote_free = ctx
                .accounts
                .maker_balance
                .quote_free
                .checked_add(plan.fill.quote_quantity)
                .ok_or(balance::FreeBalanceOverflow)?;

            ctx.accounts.taker_balance.quote_free = next_taker_quote_free;
            ctx.accounts.taker_balance.base_free = next_taker_base_free;
            ctx.accounts.maker_balance.base_locked = next_maker_base_locked;
            ctx.accounts.maker_balance.quote_free = next_maker_quote_free;
        }

        OrderSide::Ask => {
            // The taker pays base and receives quote. The resting bid releases
            // quote collateral and receives base. A final bid fill may also
            // refund quote dust accumulated through fixed-point rounding.
            let next_taker_base_free = ctx
                .accounts
                .taker_balance
                .base_free
                .checked_sub(plan.fill.base_quantity)
                .ok_or(balance::InsufficientFreeBalance)?;

            let next_taker_quote_free = ctx
                .accounts
                .taker_balance
                .quote_free
                .checked_add(plan.fill.quote_quantity)
                .ok_or(balance::FreeBalanceOverflow)?;

            let next_maker_quote_locked = ctx
                .accounts
                .maker_balance
                .quote_locked
                .checked_sub(plan.maker_locked_debit)
                .ok_or(balance::LockedBalanceUnderflow)?;

            let next_maker_base_free = ctx
                .accounts
                .maker_balance
                .base_free
                .checked_add(plan.fill.base_quantity)
                .ok_or(balance::FreeBalanceOverflow)?;

            let next_maker_quote_free = ctx
                .accounts
                .maker_balance
                .quote_free
                .checked_add(plan.maker_quote_refund)
                .ok_or(balance::FreeBalanceOverflow)?;

            ctx.accounts.taker_balance.base_free = next_taker_base_free;
            ctx.accounts.taker_balance.quote_free = next_taker_quote_free;
            ctx.accounts.maker_balance.quote_locked = next_maker_quote_locked;
            ctx.accounts.maker_balance.base_free = next_maker_base_free;
            ctx.accounts.maker_balance.quote_free = next_maker_quote_free;
        }
    }

    // Ledger settlement above and index maintenance below occur in the same
    // Solana instruction, so observers can never see a filled order that still
    // occupies the book or an unfilled order that has already been removed.
    if plan.maker_fully_filled {
        if removes_price_level {
            // This level was necessarily the current best. Promote the next
            // lower-priority level and clear its backward link.
            if let Some(worse_level) = ctx.accounts.worse_level.as_mut() {
                worse_level.better_price = None;
            }

            match ctx.accounts.maker_order.side {
                OrderSide::Bid => {
                    ctx.accounts.market.best_bid = level_worse_price;
                }
                OrderSide::Ask => {
                    ctx.accounts.market.best_ask = level_worse_price;
                }
            }
        } else {
            // The successor stays at the same price and becomes the FIFO head;
            // the tail and price-level links remain unchanged.
            let next_order = ctx
                .accounts
                .next_order
                .as_mut()
                .ok_or(order::InvalidOrderNeighbor)?;

            next_order.previous_order = None;
            ctx.accounts.maker_price_level.first_order = Some(next_order.key());
        }

        ctx.accounts.maker_price_level.order_count = next_level_order_count;
        ctx.accounts.maker_price_level.total_remaining_quantity = next_level_quantity;

        // Preserve the order account as immutable trade history while removing
        // every field that could make a client mistake it for a live queue node.
        ctx.accounts.maker_order.remaining_quantity = 0;
        ctx.accounts.maker_order.locked_collateral = 0;
        ctx.accounts.maker_order.previous_order = None;
        ctx.accounts.maker_order.next_order = None;
        ctx.accounts.maker_order.status = OrderStatus::Filled;

        ctx.accounts.market.open_order_count = next_open_order_count;

        if removes_price_level {
            let rent_recipient = ctx
                .accounts
                .level_rent_recipient
                .as_ref()
                .ok_or(order::InvalidPriceLevelRentRecipient)?
                .to_account_info();

            // Close only after all market and neighboring-level links have
            // been repaired. Any failure still rolls back the whole instruction.
            ctx.accounts.maker_price_level.close(rent_recipient)?;
        }
    } else {
        // The maker remains the same FIFO head, so only quantities change.
        ctx.accounts.maker_order.remaining_quantity = next_order_remaining;
        ctx.accounts.maker_order.locked_collateral = next_order_collateral;
        ctx.accounts.maker_price_level.total_remaining_quantity = next_level_quantity;
    }

    emit!(FillEvent {
        market: market_key,
        maker_order: maker_key,
        maker_order_id,
        maker: maker_owner,
        taker: taker_key,
        maker_side,
        execution_price: plan.fill.execution_price,
        base_quantity: plan.fill.base_quantity,
        quote_quantity: plan.fill.quote_quantity,
        maker_remaining_quantity: next_order_remaining,
        taker_remaining_quantity,
    });

    Ok(())
}

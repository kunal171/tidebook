//! Inserts the first order at a new distinct price level.
//!
//! The client supplies the optional adjacent better and worse levels. The
//! program treats them as untrusted hints and validates ordering and reciprocal
//! links before modifying the sorted level index.

use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, TransferChecked};

use crate::{
    constants::{ORDER_SEED, PRICE_LEVEL_SEED, VAULT_AUTHORITY_SEED, VAULT_SEED},
    error::MarketError,
    pda::derive_price_level_pda,
    state::{Market, MarketStatus, Order, OrderSide, OrderStatus, PriceLevel},
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

    /// Mint selected as collateral: base for asks and quote for bids.
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

    /// Adjacent level with higher matching priority, or `None` for a new best.
    #[account(mut)]
    pub better_level: Option<Account<'info, PriceLevel>>,

    /// Adjacent level with lower matching priority, or `None` for a new worst.
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
    require!(
        price % market.price_tick_size == 0,
        MarketError::PriceNotOnTick
    );

    require!(
        quantity % market.quantity_lot_size == 0,
        MarketError::QuantityNotOnLot
    );

    // This milestone supports the first level on an empty side and insertion
    // immediately before the current best. Middle and worst insertion still
    // require a supplied better neighbor and are intentionally rejected.
    require!(
        ctx.accounts.better_level.is_none(),
        MarketError::PriceLevelInsertionNotImplemented
    );

    let current_best_price = match side {
        OrderSide::Bid => market.best_bid,
        OrderSide::Ask => market.best_ask,
    };

    match (current_best_price, ctx.accounts.worse_level.as_ref()) {
        // The side is empty, so no adjacent level may be supplied.
        (None, None) => {}
        // The new level becomes best and links to the previous best below it.
        (Some(current_best_price), Some(worse_level)) => {
            require!(
                worse_level.market == market_key,
                MarketError::PriceLevelMarketMismatch
            );
            require!(
                worse_level.side == side,
                MarketError::PriceLevelSideMismatch
            );
            require!(
                worse_level.price == current_best_price,
                MarketError::BestPriceLevelMismatch
            );
            require!(
                worse_level.better_price.is_none(),
                MarketError::BestPriceLevelMismatch
            );

            // Optional accounts do not have seed constraints in the Accounts
            // struct, so validate the canonical address manually.
            let (expected_worse_level, _) =
                derive_price_level_pda(&crate::ID, &market_key, side, worse_level.price);
            require_keys_eq!(
                worse_level.key(),
                expected_worse_level,
                MarketError::NoncanonicalPriceLevel
            );

            let new_price_is_better = match side {
                OrderSide::Bid => price > worse_level.price,
                OrderSide::Ask => price < worse_level.price,
            };
            require!(new_price_is_better, MarketError::InvalidPriceLevelOrdering);
        }
        // A neighbor was supplied for an empty side, or omitted for a
        // non-empty side. Both indicate stale or malformed client state.
        _ => return err!(MarketError::InvalidPriceLevelNeighbors),
    }

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

    let order_id = market.next_order_id;

    let next_order_id = order_id
        .checked_add(1)
        .ok_or(MarketError::OrderIdOverflow)?;

    let next_open_order_count = market
        .open_order_count
        .checked_add(1)
        .ok_or(MarketError::OpenOrderCountOverflow)?;

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

    // When the side was non-empty, the previous best now points upward to the
    // new best. There is no neighbor to update for the first level.
    if let Some(worse_level) = ctx.accounts.worse_level.as_mut() {
        worse_level.better_price = Some(price);
    }

    // Initialize the new best level.
    let price_level = &mut ctx.accounts.price_level;
    price_level.market = market_key;
    price_level.side = side;
    price_level.price = price;
    price_level.better_price = None;
    price_level.worse_price = current_best_price;
    price_level.first_order = Some(order_key);
    price_level.last_order = Some(order_key);
    price_level.total_remaining_quantity = quantity;
    price_level.order_count = 1;
    price_level.rent_payer = trader_key;
    price_level.bump = ctx.bumps.price_level;

    // Initialize the first and only order at the new level.
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

    match side {
        OrderSide::Bid => market.best_bid = Some(price),
        OrderSide::Ask => market.best_ask = Some(price),
    }

    market.next_order_id = next_order_id;
    market.open_order_count = next_open_order_count;

    Ok(())
}

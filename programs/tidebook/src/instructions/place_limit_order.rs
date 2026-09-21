//! Validates and records a market-local limit order.
//!
//! Bid orders lock quote tokens and ask orders lock base tokens in the
//! market's canonical vault before the order becomes visible.

use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, TransferChecked};

use crate::{
    constants::{ORDER_SEED, VAULT_AUTHORITY_SEED, VAULT_SEED},
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

    /// Mint selected as collateral:
    /// base for asks and quote for bids.
    pub collateral_mint: Account<'info, Mint>,

    #[account(
        mut,
        constraint = trader_collateral.owner == trader.key()
            @ MarketError::InvalidCollateralOwner,
        constraint = trader_collateral.mint == collateral_mint.key()
            @ MarketError::InvalidCollateralMint
    )]
    pub trader_collateral: Account<'info, TokenAccount>,

    /// CHECK: Seed-constrained, stateless owner of the market vaults.
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

    order.owner = ctx.accounts.trader.key();
    order.market = market.key();
    order.order_id = market.next_order_id;
    order.side = side;
    order.price = price;
    order.quantity = quantity;
    order.remaining_quantity = quantity;
    order.locked_collateral = locked_collateral;
    order.status = OrderStatus::Open;
    order.bump = ctx.bumps.order;

    // Increment only after the order is fully initialized. Solana transaction
    // atomicity rolls both writes back if the instruction later fails.
    market.next_order_id += 1;

    Ok(())
}

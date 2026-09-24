//! Tidebook's on-chain Anchor program entrypoints.
//!
//! The crate delegates account validation and state transitions to focused
//! instruction modules while exposing a stable interface for generated clients.

pub mod constants;
pub mod error;
pub mod instructions;
pub mod matching;
pub mod pda;
pub mod state;

use anchor_lang::prelude::*;

pub use constants::*;
pub use instructions::*;
pub use matching::*;
pub use pda::*;
pub use state::*;

declare_id!("BPdNF5CnV8z1EkHo7tcueR6wXmzZV2j6j4wsUTirPgWL");

#[program]
pub mod tidebook {
    use super::*;

    /// Creates a configured market and its canonical base/quote token vaults.
    pub fn initialize_market(
        ctx: Context<InitializeMarket>,
        price_tick_size: u64,
        quantity_lot_size: u64,
    ) -> Result<()> {
        crate::instructions::initialize::handle_initialize_market(
            ctx,
            price_tick_size,
            quantity_lot_size,
        )
    }

    /// Stops new order placement while preserving cancellation access.
    pub fn pause_market(ctx: Context<PauseMarket>) -> Result<()> {
        crate::instructions::pause::handle_pause_market(ctx)
    }

    /// Returns a paused market to active trading state.
    pub fn unpause_market(ctx: Context<UnpauseMarket>) -> Result<()> {
        crate::instructions::unpause::handle_unpause_market(ctx)
    }
    /// Closes a paused market account and returns its rent to its authority.
    pub fn close_market(ctx: Context<CloseMarket>) -> Result<()> {
        crate::instructions::close::handle_close_market(ctx)
    }

    /// Creates singleton governance state and the deployer's admin record.
    pub fn initialize_protocol(ctx: Context<InitializeProtocol>) -> Result<()> {
        crate::instructions::initialize_protocol::handle_initialize_protocol(ctx)
    }

    /// Grants an administrator role through an independent PDA record.
    pub fn add_admin(ctx: Context<AddAdmin>, new_admin: Pubkey) -> Result<()> {
        crate::instructions::add_admin::handle_add_admin(ctx, new_admin)
    }

    /// Suspends an administrator without deleting its provenance record.
    pub fn disable_admin(ctx: Context<ManageAdmin>, target_admin: Pubkey) -> Result<()> {
        crate::instructions::manage_admin::handle_disable_admin(ctx, target_admin)
    }

    /// Restores a previously disabled administrator.
    pub fn enable_admin(ctx: Context<ManageAdmin>, target_admin: Pubkey) -> Result<()> {
        crate::instructions::manage_admin::handle_enable_admin(ctx, target_admin)
    }

    /// Closes a disabled administrator record and returns its rent.
    pub fn remove_admin(ctx: Context<RemoveAdmin>, target_admin: Pubkey) -> Result<()> {
        crate::instructions::remove_admin::handle_remove_admin(ctx, target_admin)
    }

    /// Lets an order owner cancel an open order, including while paused.
    pub fn cancel_limit_order(ctx: Context<CancelLimitOrder>, order_id: u64) -> Result<()> {
        crate::instructions::cancel_limit_order::handle_cancel_limit_order(ctx, order_id)
    }

    /// Appends an order behind the current tail of an existing price level.
    pub fn append_limit_order(
        ctx: Context<AppendLimitOrder>,
        side: OrderSide,
        price: u64,
        quantity: u64,
    ) -> Result<()> {
        crate::instructions::append_limit_order::handle_append_limit_order(
            ctx, side, price, quantity,
        )
    }

    /// Inserts an order at a new distinct sorted price level.
    pub fn insert_limit_order(
        ctx: Context<InsertLimitOrder>,
        side: OrderSide,
        price: u64,
        quantity: u64,
    ) -> Result<()> {
        crate::instructions::insert_limit_order::handle_insert_limit_order(
            ctx, side, price, quantity,
        )
    }

    /// Creates the caller's canonical zeroed balance ledger for a market.
    pub fn initialize_trader_balance(ctx: Context<InitializeTraderBalance>) -> Result<()> {
        crate::instructions::initialize_trader_balance::handle_initialize_trader_balance(ctx)
    }

    /// Deposits base or quote tokens and credits the caller's free balance.
    pub fn deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
        crate::instructions::deposit::handle_deposit(ctx, amount)
    }

    /// Withdraws free base or quote tokens to the caller's token account.
    pub fn withdraw(ctx: Context<Withdraw>, amount: u64) -> Result<()> {
        crate::instructions::withdraw::handle_withdraw(ctx, amount)
    }

    /// Executes one complete taker fill against a maker that remains open.
    pub fn match_limit_order(
        ctx: Context<MatchLimitOrder>,
        taker_side: OrderSide,
        limit_price: u64,
        quantity: u64,
    ) -> Result<()> {
        crate::instructions::match_limit_order::handle_match_limit_order(
            ctx,
            taker_side,
            limit_price,
            quantity,
        )
    }
}

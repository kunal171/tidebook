pub mod constants;
pub mod error;
pub mod instructions;
pub mod state;

use anchor_lang::prelude::*;

pub use constants::*;
pub use instructions::*;
pub use state::*;

declare_id!("BPdNF5CnV8z1EkHo7tcueR6wXmzZV2j6j4wsUTirPgWL");

#[program]
pub mod tidebook {
    use super::*;

    //Initialize a new Market Pair
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

    pub fn pause_market(ctx: Context<PauseMarket>) -> Result<()> {
        crate::instructions::pause::handle_pause_market(ctx)
    }

    pub fn place_limit_order(
        ctx: Context<PlaceLimitOrder>,
        side: OrderSide,
        price: u64,
        quantity: u64,
    ) -> Result<()> {
        crate::instructions::place_limit_order::handle_place_limit_order(ctx, side, price, quantity)
    }

    pub fn unpause_market(ctx: Context<UnpauseMarket>) -> Result<()> {
        crate::instructions::unpause::handle_unpause_market(ctx)
    }
    pub fn close_market(ctx: Context<CloseMarket>) -> Result<()> {
        crate::instructions::close::handle_close_market(ctx)
    }

    pub fn initialize_protocol(ctx: Context<InitializeProtocol>) -> Result<()> {
        crate::instructions::initialize_protocol::handle_initialize_protocol(ctx)
    }

    pub fn add_admin(ctx: Context<AddAdmin>, new_admin: Pubkey) -> Result<()> {
        crate::instructions::add_admin::handle_add_admin(ctx, new_admin)
    }

    pub fn disable_admin(ctx: Context<ManageAdmin>, target_admin: Pubkey) -> Result<()> {
        crate::instructions::manage_admin::handle_disable_admin(ctx, target_admin)
    }

    pub fn enable_admin(ctx: Context<ManageAdmin>, target_admin: Pubkey) -> Result<()> {
        crate::instructions::manage_admin::handle_enable_admin(ctx, target_admin)
    }

    pub fn remove_admin(ctx: Context<RemoveAdmin>, target_admin: Pubkey) -> Result<()> {
        crate::instructions::remove_admin::handle_remove_admin(ctx, target_admin)
    }

    pub fn cancel_limit_order(ctx: Context<CancelLimitOrder>, order_id: u64) -> Result<()> {
        crate::instructions::cancel_limit_order::handle_cancel_limit_order(ctx, order_id)
    }
}

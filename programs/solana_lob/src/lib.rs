pub mod constants;
pub mod error;
pub mod instructions;
pub mod state;

use anchor_lang::prelude::*;

pub use constants::*;
pub use instructions::*;
pub use state::*;

declare_id!("E5Ms8cNg6Xvy7RLwWVgimRZwZkhcXNjGMRZRnon5Tt1D");

#[program]
pub mod solana_lob {
    use super::*;

    //Initialize a new Market Pair
    pub fn initialize_market(ctx: Context<InitializeMarket>) -> Result<()> {
        crate::instructions::initialize::handle_initialize_market(ctx)
    }

    pub fn pause_market(ctx: Context<PauseMarket>) -> Result<()> {
        crate::instructions::pause::handle_pause_market(ctx)
    }

    pub fn unpause_market(ctx: Context<UnpauseMarket>) -> Result<()> {
        crate::instructions::unpause::handle_unpause_market(ctx)
    }
    pub fn close_market(ctx: Context<CloseMarket>) -> Result<()> {
        crate::instructions::close::handle_close_market(ctx)
    }
}

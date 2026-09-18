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

    pub fn initialize_market(ctx: Context<InitializeMarket>) -> Result<()> {
        crate::instructions::initialize::handle_initialize_market(ctx)
    }
}
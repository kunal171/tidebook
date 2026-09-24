//! Creates an active administrator record under super-admin authorization.
//!
//! Each role is an independent PDA keyed by the target wallet. This avoids an
//! unbounded administrator vector and lets duplicate creation fail atomically.

use anchor_lang::prelude::*;

use crate::{
    constants::{ADMIN_SEED, PROTOCOL_CONFIG_SEED},
    error::MarketError,
    state::{AdminRecord, AdminStatus, ProtocolConfig},
};

#[derive(Accounts)]
#[instruction(new_admin: Pubkey)]
pub struct AddAdmin<'info> {
    #[account(mut)]
    /// Governance authority and rent payer for the new role record.
    pub super_admin: Signer<'info>,

    #[account(
        seeds = [PROTOCOL_CONFIG_SEED],
        bump = protocol_config.bump,
        has_one = super_admin @ MarketError::UnauthorizedSuperAdmin
    )]
    /// Singleton config proves the signer is the immutable super-admin.
    pub protocol_config: Account<'info, ProtocolConfig>,

    #[account(
        init,
        payer = super_admin,
        space = 8 + AdminRecord::INIT_SPACE,
        seeds = [ADMIN_SEED, new_admin.as_ref()],
        bump
    )]
    /// Deterministic record whose creation rejects duplicate administrators.
    pub admin_record: Account<'info, AdminRecord>,

    pub system_program: Program<'info, System>,
}

pub fn handle_add_admin(ctx: Context<AddAdmin>, new_admin: Pubkey) -> Result<()> {
    // The all-zero key cannot sign and would create an unusable role record.
    require!(new_admin != Pubkey::default(), MarketError::InvalidAdmin);

    let admin_record = &mut ctx.accounts.admin_record;
    admin_record.authority = new_admin;
    admin_record.added_by = ctx.accounts.super_admin.key();
    admin_record.status = AdminStatus::Active;
    admin_record.bump = ctx.bumps.admin_record;

    Ok(())
}

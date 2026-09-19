use anchor_lang::prelude::*;

use crate::{
    constants::{ADMIN_SEED, PROTOCOL_CONFIG_SEED},
    error::MarketError,
    state::{AdminRecord, AdminStatus, ProtocolConfig},
};

#[derive(Accounts)]
#[instruction(target_admin: Pubkey)]
pub struct RemoveAdmin<'info> {
    #[account(mut)]
    pub super_admin: Signer<'info>,

    #[account(
        seeds = [PROTOCOL_CONFIG_SEED],
        bump = protocol_config.bump,
        has_one = super_admin @ MarketError::UnauthorizedSuperAdmin,
        constraint = target_admin != protocol_config.super_admin
            @ MarketError::CannotModifySuperAdmin
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,

    #[account(
        mut,
        close = super_admin,
        seeds = [ADMIN_SEED, target_admin.as_ref()],
        bump = admin_record.bump,
        constraint = admin_record.authority == target_admin
            @ MarketError::InvalidAdmin
    )]
    pub admin_record: Account<'info, AdminRecord>,
}

pub fn handle_remove_admin(ctx: Context<RemoveAdmin>, _target_admin: Pubkey) -> Result<()> {
    require!(
        ctx.accounts.admin_record.status == AdminStatus::Disabled,
        MarketError::AdminMustBeDisabled
    );

    // Anchor closes admin_record after the handler succeeds and sends
    // its remaining lamports to super_admin.
    Ok(())
}

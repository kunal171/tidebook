//! Removes a disabled administrator record and returns its rent.
//!
//! Requiring the disabled state makes removal an explicit two-step transition.

use anchor_lang::prelude::*;

use crate::{
    constants::{ADMIN_SEED, PROTOCOL_CONFIG_SEED},
    errors::admin,
    events::AdminRemovedEvent,
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
        has_one = super_admin @ admin::UnauthorizedSuperAdmin,
        constraint = target_admin != protocol_config.super_admin
            @ admin::CannotModifySuperAdmin
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,

    #[account(
        mut,
        close = super_admin,
        seeds = [ADMIN_SEED, target_admin.as_ref()],
        bump = admin_record.bump,
        constraint = admin_record.authority == target_admin
            @ admin::InvalidAdmin
    )]
    pub admin_record: Account<'info, AdminRecord>,
}

pub fn handle_remove_admin(ctx: Context<RemoveAdmin>, _target_admin: Pubkey) -> Result<()> {
    require!(
        ctx.accounts.admin_record.status == AdminStatus::Disabled,
        admin::AdminMustBeDisabled
    );

    emit!(AdminRemovedEvent {
        admin_record: ctx.accounts.admin_record.key(),
        authority: ctx.accounts.admin_record.authority,
        removed_by: ctx.accounts.super_admin.key(),
    });

    // Anchor closes admin_record after the handler succeeds and sends
    // its remaining lamports to super_admin.
    Ok(())
}

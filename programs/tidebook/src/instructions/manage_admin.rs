//! Enables or disables administrator records under super-admin control.
//!
//! The super-admin record is protected from disabling to preserve governance.
//! Disabling retains provenance and the PDA address; removal is a separate,
//! explicit instruction that is permitted only after this state transition.

use anchor_lang::prelude::*;

use crate::{
    constants::{ADMIN_SEED, PROTOCOL_CONFIG_SEED},
    errors::admin,
    events::AdminStatusChangedEvent,
    state::{AdminRecord, AdminStatus, ProtocolConfig},
};

#[derive(Accounts)]
#[instruction(target_admin: Pubkey)]
pub struct ManageAdmin<'info> {
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
        seeds = [ADMIN_SEED, target_admin.as_ref()],
        bump = admin_record.bump,
        constraint = admin_record.authority == target_admin
            @ admin::InvalidAdmin
    )]
    pub admin_record: Account<'info, AdminRecord>,
}

/// Moves an active role to the reversible disabled state.
pub fn handle_disable_admin(ctx: Context<ManageAdmin>, _target_admin: Pubkey) -> Result<()> {
    require!(
        ctx.accounts.admin_record.status == AdminStatus::Active,
        admin::AdminAlreadyDisabled
    );

    ctx.accounts.admin_record.status = AdminStatus::Disabled;

    emit!(AdminStatusChangedEvent {
        admin_record: ctx.accounts.admin_record.key(),
        authority: ctx.accounts.admin_record.authority,
        changed_by: ctx.accounts.super_admin.key(),
        status: AdminStatus::Disabled,
    });

    Ok(())
}

/// Restores a disabled role without replacing its provenance record.
pub fn handle_enable_admin(ctx: Context<ManageAdmin>, _target_admin: Pubkey) -> Result<()> {
    require!(
        ctx.accounts.admin_record.status == AdminStatus::Disabled,
        admin::AdminAlreadyActive
    );

    ctx.accounts.admin_record.status = AdminStatus::Active;

    emit!(AdminStatusChangedEvent {
        admin_record: ctx.accounts.admin_record.key(),
        authority: ctx.accounts.admin_record.authority,
        changed_by: ctx.accounts.super_admin.key(),
        status: AdminStatus::Active,
    });

    Ok(())
}

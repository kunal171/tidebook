//! Bootstraps singleton governance from the program's upgrade authority.
//!
//! Initialization also creates the deployer's active administrator record. The
//! transaction binds governance to the upgradeable-loader metadata, so merely
//! possessing a deployment keypair is not sufficient without upgrade authority.

use anchor_lang::prelude::*;

use crate::{
    constants::{ADMIN_SEED, PROTOCOL_CONFIG_SEED},
    errors::admin,
    state::{AdminRecord, AdminStatus, ProtocolConfig},
};

#[derive(Accounts)]
pub struct InitializeProtocol<'info> {
    #[account(mut)]
    pub deployer: Signer<'info>,

    #[account(
        init,
        payer = deployer,
        space = 8 + ProtocolConfig::INIT_SPACE,
        seeds = [PROTOCOL_CONFIG_SEED],
        bump
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,

    #[account(
        init,
        payer = deployer,
        space = 8 + AdminRecord::INIT_SPACE,
        seeds = [ADMIN_SEED, deployer.key().as_ref()],
        bump
    )]
    pub deployer_admin: Account<'info, AdminRecord>,

    #[account(
        constraint = program.programdata_address()? == Some(program_data.key())
    )]
    /// Executable whose loader metadata anchors the authorization check.
    pub program: Program<'info, crate::program::Tidebook>,

    #[account(
        constraint = program_data.upgrade_authority_address == Some(deployer.key())
            @ admin::InvalidDeployer
    )]
    /// Loader state naming the program current upgrade authority.
    pub program_data: Account<'info, ProgramData>,

    pub system_program: Program<'info, System>,
}

pub fn handle_initialize_protocol(ctx: Context<InitializeProtocol>) -> Result<()> {
    let deployer = ctx.accounts.deployer.key();

    // Config and the deployer role are initialized in one transaction so the
    // protocol never exists without an administrator able to operate it.
    let protocol_config = &mut ctx.accounts.protocol_config;
    protocol_config.super_admin = deployer;
    protocol_config.bump = ctx.bumps.protocol_config;

    let deployer_admin = &mut ctx.accounts.deployer_admin;
    deployer_admin.authority = deployer;
    deployer_admin.added_by = deployer;
    deployer_admin.status = AdminStatus::Active;
    deployer_admin.bump = ctx.bumps.deployer_admin;

    Ok(())
}

//! Updates the global taker-fee rate under super-admin authorization.
//!
//! The rate is stored once in the singleton protocol configuration so every
//! market observes the same governance decision. A zero rate disables fees;
//! the shared maximum prevents accidental or malicious confiscatory settings.

use anchor_lang::prelude::*;

use crate::{
    constants::PROTOCOL_CONFIG_SEED,
    errors::{admin, fee},
    events::TakerFeeUpdatedEvent,
    fees::MAX_TAKER_FEE_BPS,
    state::ProtocolConfig,
};

#[derive(Accounts)]
pub struct SetTakerFee<'info> {
    /// Only the authority stored in the singleton config may change fees.
    pub super_admin: Signer<'info>,

    #[account(
        mut,
        seeds = [PROTOCOL_CONFIG_SEED],
        bump = protocol_config.bump,
        has_one = super_admin @ admin::UnauthorizedSuperAdmin
    )]
    pub protocol_config: Account<'info, ProtocolConfig>,
}

pub fn handle_set_taker_fee(ctx: Context<SetTakerFee>, taker_fee_bps: u16) -> Result<()> {
    require!(taker_fee_bps <= MAX_TAKER_FEE_BPS, fee::FeeRateTooHigh);

    let previous_taker_fee_bps = ctx.accounts.protocol_config.taker_fee_bps;
    ctx.accounts.protocol_config.taker_fee_bps = taker_fee_bps;

    emit!(TakerFeeUpdatedEvent {
        protocol_config: ctx.accounts.protocol_config.key(),
        super_admin: ctx.accounts.super_admin.key(),
        previous_taker_fee_bps,
        new_taker_fee_bps: taker_fee_bps,
    });

    Ok(())
}

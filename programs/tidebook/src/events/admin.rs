//! Protocol initialization and administrator-governance events.

use anchor_lang::prelude::*;

use crate::state::AdminStatus;

/// Records creation of the singleton governance configuration and the
/// deployer's initial active administrator record.
#[event]
#[derive(Debug, PartialEq, Eq)]
pub struct ProtocolInitializedEvent {
    pub protocol_config: Pubkey,
    pub super_admin: Pubkey,
    pub admin_record: Pubkey,
}

/// Records creation of an independently addressable administrator record.
#[event]
#[derive(Debug, PartialEq, Eq)]
pub struct AdminAddedEvent {
    pub admin_record: Pubkey,
    pub authority: Pubkey,
    pub added_by: Pubkey,
}

/// Records a reversible administrator enable or disable transition.
#[event]
#[derive(Debug, PartialEq, Eq)]
pub struct AdminStatusChangedEvent {
    pub admin_record: Pubkey,
    pub authority: Pubkey,
    pub changed_by: Pubkey,
    pub status: AdminStatus,
}

/// Records permanent closure of a previously disabled administrator record.
#[event]
#[derive(Debug, PartialEq, Eq)]
pub struct AdminRemovedEvent {
    pub admin_record: Pubkey,
    pub authority: Pubkey,
    pub removed_by: Pubkey,
}

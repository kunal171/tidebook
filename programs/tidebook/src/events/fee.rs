//! Protocol fee-policy and treasury events.

use anchor_lang::prelude::*;

/// Records quote-denominated protocol revenue leaving market custody.
#[event]
#[derive(Debug, PartialEq, Eq)]
pub struct ProtocolFeesWithdrawnEvent {
    pub market: Pubkey,
    pub market_fees: Pubkey,
    pub super_admin: Pubkey,
    pub quote_mint: Pubkey,
    pub quote_vault: Pubkey,
    pub destination_quote_account: Pubkey,
    pub amount: u64,
    pub remaining_accrued_quote_fees: u64,
}

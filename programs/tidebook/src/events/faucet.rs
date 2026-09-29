//! Events emitted when a test faucet is configured or used.

use anchor_lang::prelude::*;

#[event]
pub struct TestFaucetInitializedEvent {
    pub market: Pubkey,
    pub test_faucet: Pubkey,
    pub faucet_authority: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub base_claim_amount: u64,
    pub quote_claim_amount: u64,
}

#[event]
pub struct TestFaucetClaimedEvent {
    pub market: Pubkey,
    pub test_faucet: Pubkey,
    pub claimant: Pubkey,
    pub base_amount: u64,
    pub quote_amount: u64,
}

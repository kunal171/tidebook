//! Trader-ledger and vault-custody events.

use anchor_lang::prelude::*;

/// Records creation of a trader's canonical per-market internal ledger.
#[event]
#[derive(Debug, PartialEq, Eq)]
pub struct TraderBalanceInitializedEvent {
    pub market: Pubkey,
    pub owner: Pubkey,
    pub trader_balance: Pubkey,
}

/// Records tokens entering a market vault and the corresponding free-balance
/// credit. `free_balance` is the selected asset's post-deposit balance.
#[event]
#[derive(Debug, PartialEq, Eq)]
pub struct DepositEvent {
    pub market: Pubkey,
    pub owner: Pubkey,
    pub trader_balance: Pubkey,
    pub mint: Pubkey,
    pub market_vault: Pubkey,
    pub amount: u64,
    pub free_balance: u64,
}

/// Records a free-balance debit and its atomic token transfer out of custody.
/// `free_balance` is the selected asset's post-withdrawal balance.
#[event]
#[derive(Debug, PartialEq, Eq)]
pub struct WithdrawalEvent {
    pub market: Pubkey,
    pub owner: Pubkey,
    pub trader_balance: Pubkey,
    pub mint: Pubkey,
    pub market_vault: Pubkey,
    pub amount: u64,
    pub free_balance: u64,
}

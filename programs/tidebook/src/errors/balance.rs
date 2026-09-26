//! Trader-ledger, deposit, withdrawal, and vault-backing errors.

pub use super::TidebookError::{
    FreeBalanceOverflow, InsufficientDepositFunds, InsufficientFreeBalance, InsufficientVaultFunds,
    InvalidDepositAmount, InvalidDepositMint, InvalidWithdrawalAmount,
    InvalidWithdrawalDestinationOwner, InvalidWithdrawalMint, LockedBalanceOverflow,
    LockedBalanceUnderflow, TraderBalanceMarketMismatch, TraderBalanceOwnerMismatch,
};

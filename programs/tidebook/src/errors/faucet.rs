//! Errors for governance-controlled test-faucet initialization and claims.

pub use super::catalog::TidebookError::{
    FaucetMarketMismatch, FaucetMintMismatch, FaucetMintSupplyNotZero, InvalidFaucetClaimAmount,
    InvalidFaucetDestinationOwner, InvalidFaucetMintAuthority, UnauthorizedSuperAdmin,
};

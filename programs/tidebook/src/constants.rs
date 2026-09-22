//! Shared PDA namespaces and numeric boundaries used by the program and tests.

use anchor_lang::prelude::*;

/// Namespace for markets keyed by their ordered base/quote mint pair.
#[constant]
pub const MARKET_SEED: &[u8] = b"market";

/// Namespace for market-local orders keyed by their monotonic order ID.
#[constant]
pub const ORDER_SEED: &[u8] = b"order";

#[constant]
pub const MIN_PRICE: u64 = 1;

#[constant]
pub const MAX_PRICE: u64 = u64::MAX;

#[constant]
pub const MAX_QUANTITY: u64 = u64::MAX;

/// Singleton protocol-governance configuration.
#[constant]
pub const PROTOCOL_CONFIG_SEED: &[u8] = b"protocol_config";

/// Per-wallet administrator record namespace.
#[constant]
pub const ADMIN_SEED: &[u8] = b"admin";

/// Stateless PDA that owns both token vaults belonging to a market.
#[constant]
pub const VAULT_AUTHORITY_SEED: &[u8] = b"vault-authority";

/// Token vault namespace; the remaining seeds are the market and asset mint.
#[constant]
pub const VAULT_SEED: &[u8] = b"vault";

/// Namespace for one active market-side-price queue.
#[constant]
pub const PRICE_LEVEL_SEED: &[u8] = b"price_level";

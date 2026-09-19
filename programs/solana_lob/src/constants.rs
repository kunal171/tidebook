use anchor_lang::prelude::*;

#[constant]
pub const MARKET_SEED: &[u8] = b"market";

#[constant]
pub const ORDER_SEED: &[u8] = b"order";

#[constant]
pub const MIN_PRICE: u64 = 1;

#[constant]
pub const MAX_PRICE: u64 = u64::MAX;

#[constant]
pub const MAX_QUANTITY: u64 = u64::MAX;

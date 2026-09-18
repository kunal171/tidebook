use anchor_lang::prelude::*;

// Market Struct to initialize Market
#[account]
#[derive(InitSpace)]
pub struct Market {
    pub authority: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub status: u8, // 0-Active, 1-Paused
    pub next_order_id: i64,
    pub best_bid: Option<u64>,
    pub best_ask: Option<u64>,
    pub bump: u8,
}

use anchor_lang::prelude::*;

// Market Struct to initialize Market
#[account]
#[derive(InitSpace)]
pub struct Market {
    pub authority: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub status: MarketStatus,
    pub next_order_id: u64,
    pub best_bid: Option<u64>,
    pub best_ask: Option<u64>,
    pub bump: u8,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, InitSpace, Debug)]
pub enum MarketStatus {
    Active,
    Paused,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, InitSpace, Debug)]
pub enum OrderSide {
    Bid,
    Ask,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, InitSpace, Debug)]
pub enum OrderStatus {
    Open,
    Filled,
    Canceled,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, InitSpace, Debug)]
pub enum AdminStatus {
    Active,
    Disabled,
}

#[account]
#[derive(InitSpace)]
pub struct Order {
    pub owner: Pubkey,
    pub market: Pubkey,
    pub order_id: u64,
    pub side: OrderSide,
    pub price: u64,
    pub quantity: u64,
    pub remaining_quantity: u64,
    pub status: OrderStatus,
    pub bump: u8,
}

// Create one super admin on initialization.
#[account]
#[derive(InitSpace)]
pub struct ProtocolConfig {
    pub super_admin: Pubkey,
    pub bump: u8,
}

// Administrator record for market control.
#[account]
#[derive(InitSpace)]
pub struct AdminRecord {
    pub authority: Pubkey,
    pub added_by: Pubkey,
    pub status: AdminStatus,
    pub bump: u8,
}

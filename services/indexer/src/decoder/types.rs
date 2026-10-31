//! Off-chain representations of Tidebook's serialized Anchor events.
//!
//! Field order and field types must exactly match the corresponding on-chain
//! event declarations. Borsh is positional and does not encode field names.

use borsh::BorshDeserialize;
use serde::Serialize;
use solana_pubkey::Pubkey;

/// Off-chain mirror of the market-creation event emitted by the program.
#[derive(Debug, Clone, PartialEq, Eq, BorshDeserialize, Serialize)]
pub struct MarketInitializedEvent {
    pub market: Pubkey,
    pub market_fees: Pubkey,
    pub authority: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub base_vault: Pubkey,
    pub quote_vault: Pubkey,
    pub price_tick_size: u64,
    pub quantity_lot_size: u64,
}

/// On-chain market lifecycle state.
///
/// Borsh stores fieldless enum variants by their zero-based position. Do not
/// reorder these variants: `Active` is encoded as `0` and `Paused` as `1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshDeserialize, Serialize)]
pub enum MarketStatus {
    Active,
    Paused,
}

/// Records an authorized pause or unpause transition.
#[derive(Debug, Clone, PartialEq, Eq, BorshDeserialize, Serialize)]
pub struct MarketStatusChangedEvent {
    pub market: Pubkey,
    pub authority: Pubkey,
    pub status: MarketStatus,
}

/// Records successful closure of the market, fee state, and custody vaults.
#[derive(Debug, Clone, PartialEq, Eq, BorshDeserialize, Serialize)]
pub struct MarketClosedEvent {
    pub market: Pubkey,
    pub market_fees: Pubkey,
    pub authority: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub base_vault: Pubkey,
    pub quote_vault: Pubkey,
}

/// Side of a resting order.
///
/// Borsh encodes `Bid` as `0` and `Ask` as `1`. Variant order must remain
/// identical to the on-chain `OrderSide`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshDeserialize, Serialize)]
pub enum OrderSide {
    Bid,
    Ask,
}

/// Records an order entering a price-level FIFO queue.
#[derive(Debug, Clone, PartialEq, Eq, BorshDeserialize, Serialize)]
pub struct OrderPlacedEvent {
    pub market: Pubkey,
    pub order: Pubkey,
    pub order_id: u64,
    pub owner: Pubkey,
    pub side: OrderSide,
    pub price: u64,
    pub quantity: u64,
    pub locked_collateral: u64,
    pub price_level: Pubkey,
}

/// Records owner-authorized cancellation of an open order.
#[derive(Debug, Clone, PartialEq, Eq, BorshDeserialize, Serialize)]
pub struct OrderCanceledEvent {
    pub market: Pubkey,
    pub order: Pubkey,
    pub order_id: u64,
    pub owner: Pubkey,
    pub side: OrderSide,
    pub price: u64,
    pub canceled_quantity: u64,
    pub released_collateral: u64,
    pub price_level: Pubkey,
    pub price_level_closed: bool,
}

/// Records one maker-price execution between a resting maker and an incoming
/// taker.
///
/// Field order must remain identical to the on-chain `FillEvent`. Borsh uses
/// positional encoding, so reordering fields would silently corrupt decoding.
#[derive(Debug, Clone, PartialEq, Eq, BorshDeserialize, Serialize)]
pub struct FillEvent {
    pub market: Pubkey,
    pub maker_order: Pubkey,
    pub maker_order_id: u64,
    pub maker: Pubkey,
    pub taker: Pubkey,
    pub maker_side: OrderSide,
    pub execution_price: u64,
    pub base_quantity: u64,
    pub quote_quantity: u64,
    pub taker_fee_quote: u64,
    pub maker_remaining_quantity: u64,
    pub taker_remaining_quantity: u64,
}

/// Records creation of a trader's canonical per-market internal ledger.
#[derive(Debug, Clone, PartialEq, Eq, BorshDeserialize, Serialize)]
pub struct TraderBalanceInitializedEvent {
    pub market: Pubkey,
    pub owner: Pubkey,
    pub trader_balance: Pubkey,
}

/// Records tokens entering market custody and the corresponding internal
/// free-balance credit.
///
/// `free_balance` is the selected asset's balance after the deposit.
#[derive(Debug, Clone, PartialEq, Eq, BorshDeserialize, Serialize)]
pub struct DepositEvent {
    pub market: Pubkey,
    pub owner: Pubkey,
    pub trader_balance: Pubkey,
    pub mint: Pubkey,
    pub market_vault: Pubkey,
    pub amount: u64,
    pub free_balance: u64,
}

/// Records an internal free-balance debit and the atomic token transfer out of
/// market custody.
///
/// `free_balance` is the selected asset's balance after the withdrawal.
#[derive(Debug, Clone, PartialEq, Eq, BorshDeserialize, Serialize)]
pub struct WithdrawalEvent {
    pub market: Pubkey,
    pub owner: Pubkey,
    pub trader_balance: Pubkey,
    pub mint: Pubkey,
    pub market_vault: Pubkey,
    pub amount: u64,
    pub free_balance: u64,
}

/// On-chain administrator lifecycle state.
///
/// Borsh encodes `Active` as `0` and `Disabled` as `1`. Variant order must
/// remain identical to the on-chain `AdminStatus` declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshDeserialize, Serialize)]
pub enum AdminStatus {
    Active,
    Disabled,
}

/// Records creation of protocol governance and the deployer's administrator
/// record.
#[derive(Debug, Clone, PartialEq, Eq, BorshDeserialize, Serialize)]
pub struct ProtocolInitializedEvent {
    pub protocol_config: Pubkey,
    pub super_admin: Pubkey,
    pub admin_record: Pubkey,
}

/// Records creation of an independently addressable administrator record.
#[derive(Debug, Clone, PartialEq, Eq, BorshDeserialize, Serialize)]
pub struct AdminAddedEvent {
    pub admin_record: Pubkey,
    pub authority: Pubkey,
    pub added_by: Pubkey,
}

/// Records a reversible administrator enable or disable transition.
#[derive(Debug, Clone, PartialEq, Eq, BorshDeserialize, Serialize)]
pub struct AdminStatusChangedEvent {
    pub admin_record: Pubkey,
    pub authority: Pubkey,
    pub changed_by: Pubkey,
    pub status: AdminStatus,
}

/// Records permanent closure of a disabled administrator record.
#[derive(Debug, Clone, PartialEq, Eq, BorshDeserialize, Serialize)]
pub struct AdminRemovedEvent {
    pub admin_record: Pubkey,
    pub authority: Pubkey,
    pub removed_by: Pubkey,
}

/// Records a global taker-fee policy change.
#[derive(Debug, Clone, PartialEq, Eq, BorshDeserialize, Serialize)]
pub struct TakerFeeUpdatedEvent {
    pub protocol_config: Pubkey,
    pub super_admin: Pubkey,
    pub previous_taker_fee_bps: u16,
    pub new_taker_fee_bps: u16,
}

/// Records quote-denominated protocol revenue leaving market custody.
#[derive(Debug, Clone, PartialEq, Eq, BorshDeserialize, Serialize)]
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

/// Records creation of a market-specific devnet test faucet.
#[derive(Debug, Clone, PartialEq, Eq, BorshDeserialize, Serialize)]
pub struct TestFaucetInitializedEvent {
    pub market: Pubkey,
    pub test_faucet: Pubkey,
    pub faucet_authority: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub base_claim_amount: u64,
    pub quote_claim_amount: u64,
}

/// Records a claimant receiving both configured test assets.
#[derive(Debug, Clone, PartialEq, Eq, BorshDeserialize, Serialize)]
pub struct TestFaucetClaimedEvent {
    pub market: Pubkey,
    pub test_faucet: Pubkey,
    pub claimant: Pubkey,
    pub base_amount: u64,
    pub quote_amount: u64,
}

/// Records closure of a market-specific test faucet.
#[derive(Debug, Clone, PartialEq, Eq, BorshDeserialize, Serialize)]
pub struct TestFaucetClosedEvent {
    pub market: Pubkey,
    pub test_faucet: Pubkey,
    pub authority: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
}

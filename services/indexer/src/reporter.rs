//! Human-readable diagnostics for decoded Tidebook events.

use crate::decoder::{
    BalanceEvent, DecodedEventLog, FaucetEvent, FeeEvent, GovernanceEvent, MarketEvent, OrderEvent,
    TidebookEvent,
};

/// Prints one decoded event without coupling display policy to ingestion.
pub fn report_event(decoded: &DecodedEventLog) {
    println!(
        "  event_log_index={} event_type={} payload={}",
        decoded.log_index,
        decoded.event.event_type(),
        decoded.payload_base64,
    );

    match &decoded.event {
        TidebookEvent::Governance(event) => match event {
            GovernanceEvent::ProtocolInitialized(event) => {
                println!(
                    "    ProtocolInitialized protocol_config={} super_admin={} admin_record={}",
                    event.protocol_config, event.super_admin, event.admin_record,
                );
            }
            GovernanceEvent::AdminAdded(event) => {
                println!(
                    "    AdminAdded admin_record={} authority={} added_by={}",
                    event.admin_record, event.authority, event.added_by,
                );
            }
            GovernanceEvent::AdminStatusChanged(event) => {
                println!(
                    "    AdminStatusChanged admin_record={} authority={} changed_by={} status={:?}",
                    event.admin_record, event.authority, event.changed_by, event.status,
                );
            }
            GovernanceEvent::AdminRemoved(event) => {
                println!(
                    "    AdminRemoved admin_record={} authority={} removed_by={}",
                    event.admin_record, event.authority, event.removed_by,
                );
            }
            GovernanceEvent::TakerFeeUpdated(event) => {
                println!(
                    "    TakerFeeUpdated protocol_config={} super_admin={} previous_taker_fee_bps={} new_taker_fee_bps={}",
                    event.protocol_config,
                    event.super_admin,
                    event.previous_taker_fee_bps,
                    event.new_taker_fee_bps,
                );
            }
        },
        TidebookEvent::Market(event) => match event {
            MarketEvent::Initialized(event) => {
                println!(
                    "    MarketInitialized market={} authority={} base_mint={} quote_mint={} price_tick_size={} quantity_lot_size={}",
                    event.market,
                    event.authority,
                    event.base_mint,
                    event.quote_mint,
                    event.price_tick_size,
                    event.quantity_lot_size,
                );
            }
            MarketEvent::StatusChanged(event) => {
                println!(
                    "    MarketStatusChanged market={} authority={} status={:?}",
                    event.market, event.authority, event.status,
                );
            }
            MarketEvent::Closed(event) => {
                println!(
                    "    MarketClosed market={} authority={} base_mint={} quote_mint={}",
                    event.market, event.authority, event.base_mint, event.quote_mint,
                );
            }
        },
        TidebookEvent::Order(event) => match event {
            OrderEvent::Placed(event) => {
                println!(
                    "    OrderPlaced market={} order={} order_id={} owner={} side={:?} price={} quantity={} locked_collateral={} price_level={}",
                    event.market,
                    event.order,
                    event.order_id,
                    event.owner,
                    event.side,
                    event.price,
                    event.quantity,
                    event.locked_collateral,
                    event.price_level,
                );
            }
            OrderEvent::Canceled(event) => {
                println!(
                    "    OrderCanceled market={} order={} order_id={} owner={} side={:?} price={} canceled_quantity={} released_collateral={} price_level={} price_level_closed={}",
                    event.market,
                    event.order,
                    event.order_id,
                    event.owner,
                    event.side,
                    event.price,
                    event.canceled_quantity,
                    event.released_collateral,
                    event.price_level,
                    event.price_level_closed,
                );
            }
        },
        TidebookEvent::Fill(event) => {
            println!(
                "    Fill market={} maker_order={} maker_order_id={} maker={} taker={} maker_side={:?} execution_price={} base_quantity={} quote_quantity={} taker_fee_quote={} maker_remaining_quantity={} taker_remaining_quantity={}",
                event.market,
                event.maker_order,
                event.maker_order_id,
                event.maker,
                event.taker,
                event.maker_side,
                event.execution_price,
                event.base_quantity,
                event.quote_quantity,
                event.taker_fee_quote,
                event.maker_remaining_quantity,
                event.taker_remaining_quantity,
            );
        }
        TidebookEvent::Balance(event) => match event {
            BalanceEvent::Initialized(event) => {
                println!(
                    "    TraderBalanceInitialized market={} owner={} trader_balance={}",
                    event.market, event.owner, event.trader_balance,
                );
            }
            BalanceEvent::Deposited(event) => {
                println!(
                    "    Deposit market={} owner={} trader_balance={} mint={} market_vault={} amount={} free_balance={}",
                    event.market,
                    event.owner,
                    event.trader_balance,
                    event.mint,
                    event.market_vault,
                    event.amount,
                    event.free_balance,
                );
            }
            BalanceEvent::Withdrawn(event) => {
                println!(
                    "    Withdrawal market={} owner={} trader_balance={} mint={} market_vault={} amount={} free_balance={}",
                    event.market,
                    event.owner,
                    event.trader_balance,
                    event.mint,
                    event.market_vault,
                    event.amount,
                    event.free_balance,
                );
            }
        },
        TidebookEvent::Fee(event) => match event {
            FeeEvent::ProtocolFeesWithdrawn(event) => {
                println!(
                    "    ProtocolFeesWithdrawn market={} market_fees={} super_admin={} quote_mint={} quote_vault={} destination_quote_account={} amount={} remaining_accrued_quote_fees={}",
                    event.market,
                    event.market_fees,
                    event.super_admin,
                    event.quote_mint,
                    event.quote_vault,
                    event.destination_quote_account,
                    event.amount,
                    event.remaining_accrued_quote_fees,
                );
            }
        },
        TidebookEvent::Faucet(event) => match event {
            FaucetEvent::Initialized(event) => {
                println!(
                    "    TestFaucetInitialized market={} test_faucet={} faucet_authority={} base_mint={} quote_mint={} base_claim_amount={} quote_claim_amount={}",
                    event.market,
                    event.test_faucet,
                    event.faucet_authority,
                    event.base_mint,
                    event.quote_mint,
                    event.base_claim_amount,
                    event.quote_claim_amount,
                );
            }
            FaucetEvent::Claimed(event) => {
                println!(
                    "    TestFaucetClaimed market={} test_faucet={} claimant={} base_amount={} quote_amount={}",
                    event.market,
                    event.test_faucet,
                    event.claimant,
                    event.base_amount,
                    event.quote_amount,
                );
            }
            FaucetEvent::Closed(event) => {
                println!(
                    "    TestFaucetClosed market={} test_faucet={} authority={} base_mint={} quote_mint={}",
                    event.market,
                    event.test_faucet,
                    event.authority,
                    event.base_mint,
                    event.quote_mint,
                );
            }
        },
    }
}

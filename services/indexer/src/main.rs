//! Tidebook off-chain indexing service.
//!
//! The foundation milestone fetches a recent finalized transaction batch and
//! extracts raw Tidebook-owned event payloads from its execution logs. Anchor
//! event decoding, paginated backfill, storage, and live subscriptions are
//! intentionally deferred to focused follow-up milestones.

mod config;
mod decoder;
mod events;
mod rpc;

use config::Config;
use decoder::{decode_event, MarketEvent, OrderEvent, TidebookEvent};
use events::extract_program_data;
use rpc::{fetch_recent_successful_signatures, fetch_transaction};
use solana_commitment_config::CommitmentConfig;
use solana_pubkey::Pubkey;
use solana_rpc_client::rpc_client::RpcClient;
use std::{error::Error, str::FromStr};

const INITIAL_SIGNATURE_LIMIT: usize = 20;

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let config = Config::from_env();
    let program_id = Pubkey::from_str(&config.program_id)?;

    println!("Tidebook indexer");
    println!("RPC endpoint: {}", config.rpc_url);
    println!("Program: {program_id}");

    // Reuse one blocking client for the signature page and every transaction
    // fetch. Besides avoiding repeated client construction, this preserves the
    // HTTP connection pool during a backfill batch.
    let client =
        RpcClient::new_with_commitment(config.rpc_url.clone(), CommitmentConfig::finalized());

    let signatures =
        fetch_recent_successful_signatures(&client, &program_id, INITIAL_SIGNATURE_LIMIT)?;

    println!(
        "Found {} successful finalized transactions:",
        signatures.len()
    );

    for signature in signatures {
        let Some(transaction) = fetch_transaction(&client, &signature)? else {
            println!(
                "Skipped transaction without successful metadata: {}",
                signature.signature
            );
            continue;
        };
        let raw_events = extract_program_data(&transaction.logs, &program_id);

        println!(
            "slot={} signature={} block_time={:?} logs={} tidebook_events={}",
            transaction.slot,
            transaction.signature,
            transaction.block_time,
            transaction.logs.len(),
            raw_events.len(),
        );

        for raw_event in raw_events {
            // Decoding is deliberately fail-closed. Once durable checkpoints
            // exist, returning here prevents a checkpoint from moving beyond
            // an unknown or malformed event and creating a partial projection.
            let decoded = decode_event(&raw_event)?;

            println!(
                "  event_log_index={} payload={}",
                decoded.log_index, decoded.payload_base64,
            );

            match &decoded.event {
                TidebookEvent::Market(event) => match event {
                    MarketEvent::Initialized(event) => {
                        println!(
                            "    MarketInitialized market={} authority={} \
                            base_mint={} quote_mint={} \
                            price_tick_size={} quantity_lot_size={}",
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
                            "    MarketClosed market={} authority={} \
                            base_mint={} quote_mint={}",
                            event.market, event.authority, event.base_mint, event.quote_mint,
                        );
                    }
                },
                TidebookEvent::Order(event) => match event {
                    OrderEvent::Placed(event) => {
                        println!(
                            "    OrderPlaced market={} order={} order_id={} \
                            owner={} side={:?} price={} quantity={} \
                            locked_collateral={} price_level={}",
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
                            "    OrderCanceled market={} order={} order_id={} \
                            owner={} side={:?} price={} canceled_quantity={} \
                            released_collateral={} price_level={} \
                            price_level_closed={}",
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
                        "    Fill market={} maker_order={} maker_order_id={} \
                        maker={} taker={} maker_side={:?} execution_price={} \
                        base_quantity={} quote_quantity={} taker_fee_quote={} \
                        maker_remaining_quantity={} taker_remaining_quantity={}",
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
            }
        }
    }

    Ok(())
}

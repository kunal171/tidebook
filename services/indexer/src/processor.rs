//! Finalized transaction ingestion pipeline.

use std::{error::Error, io};

use solana_pubkey::Pubkey;
use solana_rpc_client::rpc_client::RpcClient;
use sqlx::PgPool;

use crate::{
    decoder::decode_event,
    events::extract_program_data,
    reporter::report_event,
    rpc::{fetch_transaction, SignatureRecord},
    storage::persist_transaction,
};

/// Fetches, decodes, persists, and reports one finalized transaction.
///
/// Failures propagate to the page controller so it cannot advance beyond an
/// unavailable transaction, malformed event, or rejected database write.
pub async fn process_signature(
    client: &RpcClient,
    pool: &PgPool,
    program_id: &Pubkey,
    signature: &SignatureRecord,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let transaction = fetch_transaction(client, signature)?.ok_or_else(|| {
        io::Error::other(format!(
            "finalized transaction {} returned unusable metadata",
            signature.signature
        ))
    })?;

    let raw_events = extract_program_data(&transaction.logs, program_id);

    println!(
        "slot={} signature={} block_time={:?} logs={} tidebook_events={}",
        transaction.slot,
        transaction.signature,
        transaction.block_time,
        transaction.logs.len(),
        raw_events.len(),
    );

    // Unknown, malformed, or trailing event bytes stop the page before its
    // checkpoint moves forward, keeping the decoder deliberately fail-closed.
    let decoded_events = raw_events
        .iter()
        .map(decode_event)
        .collect::<Result<Vec<_>, _>>()?;

    persist_transaction(pool, &transaction, &decoded_events).await?;

    for decoded in &decoded_events {
        report_event(decoded);
    }

    Ok(())
}

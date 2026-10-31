//! Historical Solana RPC access for the Tidebook indexer.

use std::error::Error;

use solana_commitment_config::CommitmentConfig;
use solana_pubkey::Pubkey;
use solana_rpc_client::rpc_client::{GetConfirmedSignaturesForAddress2Config, RpcClient};
use solana_rpc_client_api::config::RpcTransactionConfig;
use solana_signature::Signature;
use solana_transaction_status_client_types::{
    option_serializer::OptionSerializer, UiTransactionEncoding,
};

/// Lightweight entry returned by `getSignaturesForAddress`.
///
/// This record is suitable for pagination and checkpoint discovery. Complete
/// execution data must still be loaded through `getTransaction`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureRecord {
    pub signature: String,
    pub slot: u64,
    pub block_time: Option<i64>,
}

/// Immutable source data retained for a successfully finalized transaction.
///
/// Keeping logs in their original order is essential: multiple Anchor events
/// may be emitted by one transaction and their position becomes the stable
/// `event_index` used for idempotent database writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionRecord {
    pub signature: String,
    pub slot: u64,
    pub block_time: Option<i64>,
    pub logs: Vec<String>,
}

/// One deterministic page of finalized transaction signatures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignaturePage {
    /// Successful transactions ordered from oldest to newest.
    pub records: Vec<SignatureRecord>,

    /// Cursor used to request the next, older page.
    pub next_before: Option<Signature>,
}

/// Fetches one page of finalized signatures involving `program_id`.
///
/// Solana returns signatures newest-first. The indexer reverses successful
/// records so state is replayed chronologically.
pub fn fetch_signature_page(
    client: &RpcClient,
    program_id: &Pubkey,
    before: Option<&Signature>,
    limit: usize,
) -> Result<SignaturePage, Box<dyn Error + Send + Sync>> {
    let entries = client.get_signatures_for_address_with_config(
        program_id,
        GetConfirmedSignaturesForAddress2Config {
            before: before.cloned(),
            until: None,
            limit: Some(limit),
            commitment: Some(CommitmentConfig::finalized()),
        },
    )?;

    // Derive this before filtering failed transactions. The cursor describes
    // the RPC page boundary, not merely the successful subset.
    let next_before = entries
        .last()
        .map(|entry| entry.signature.parse::<Signature>())
        .transpose()?;

    let mut records = entries
        .into_iter()
        .filter(|entry| entry.err.is_none())
        .map(|entry| SignatureRecord {
            signature: entry.signature,
            slot: entry.slot,
            block_time: entry.block_time,
        })
        .collect::<Vec<_>>();

    // RPC returns newest-to-oldest. Indexing oldest-to-newest gives
    // deterministic event and projection ordering.
    records.reverse();

    Ok(SignaturePage {
        records,
        next_before,
    })
}

/// Fetches a successful finalized transaction and preserves its raw logs.
///
/// `None` means the RPC response cannot be used as an authoritative source
/// record because metadata is absent or reports a failed execution. Transport,
/// parsing, and RPC failures remain errors so callers can retry rather than
/// silently advancing their durable checkpoint.
pub fn fetch_transaction(
    client: &RpcClient,
    signature_record: &SignatureRecord,
) -> Result<Option<TransactionRecord>, Box<dyn Error + Send + Sync>> {
    let signature: Signature = signature_record.signature.parse()?;

    let transaction = client.get_transaction_with_config(
        &signature,
        RpcTransactionConfig {
            // JSON retains metadata and log messages without requiring binary
            // transaction decoding during this indexing phase.
            encoding: Some(UiTransactionEncoding::Json),
            commitment: Some(CommitmentConfig::finalized()),
            // Accept legacy and v0 transactions. Newer versions must be added
            // deliberately instead of being decoded with v0 assumptions.
            max_supported_transaction_version: Some(0),
        },
    )?;

    let Some(meta) = transaction.transaction.meta else {
        return Ok(None);
    };

    // Solana distinguishes an explicit null (`None`) from an omitted (`Skip`)
    // JSON field. Neither supplies logs, so both normalize to an empty vector.
    let Some(logs) = normalize_logs(meta.err.is_some(), meta.log_messages) else {
        return Ok(None);
    };

    Ok(Some(TransactionRecord {
        signature: signature_record.signature.clone(),
        slot: transaction.slot,
        block_time: transaction.block_time,
        logs,
    }))
}

/// Applies Tidebook's ingestion policy to transaction execution logs.
///
/// A failed execution cannot become an indexed source record because Solana
/// rolled back its state changes. Missing log fields are valid for a successful
/// transaction and normalize to an empty collection.
fn normalize_logs(
    execution_failed: bool,
    log_messages: OptionSerializer<Vec<String>>,
) -> Option<Vec<String>> {
    if execution_failed {
        return None;
    }

    Some(match log_messages {
        OptionSerializer::Some(logs) => logs,
        OptionSerializer::None | OptionSerializer::Skip => Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successful_transaction_preserves_log_order() {
        let logs = vec![
            "Program log: first".to_owned(),
            "Program data: second".to_owned(),
            "Program log: third".to_owned(),
        ];

        let normalized = normalize_logs(false, OptionSerializer::Some(logs.clone()));

        assert_eq!(normalized, Some(logs));
    }

    #[test]
    fn failed_transaction_is_rejected_even_when_logs_exist() {
        let logs = vec!["Program data: rolled-back-event".to_owned()];

        let normalized = normalize_logs(true, OptionSerializer::Some(logs));

        assert_eq!(normalized, None);
    }

    #[test]
    fn explicit_missing_logs_normalize_to_empty_collection() {
        let normalized = normalize_logs(false, OptionSerializer::None);

        assert_eq!(normalized, Some(Vec::new()));
    }

    #[test]
    fn omitted_logs_normalize_to_empty_collection() {
        let normalized = normalize_logs(false, OptionSerializer::Skip);

        assert_eq!(normalized, Some(Vec::new()));
    }

    #[test]
    fn successful_transaction_with_empty_logs_remains_valid() {
        let normalized = normalize_logs(false, OptionSerializer::Some(Vec::new()));

        assert_eq!(normalized, Some(Vec::new()));
    }
}

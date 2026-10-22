//! Historical Solana RPC access for the Tidebook indexer.

use std::error::Error;

use solana_commitment_config::CommitmentConfig;
use solana_pubkey::Pubkey;
use solana_rpc_client::rpc_client::{GetConfirmedSignaturesForAddress2Config, RpcClient};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureRecord {
    pub signature: String,
    pub slot: u64,
    pub block_time: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionRecord {
    pub signature: String,
    pub slot: u64,
    pub block_time: Option<i64>,
    pub logs: Vec<String>,
}

pub fn fetch_recent_successful_signatures(
    client: &RpcClient,
    program_id: &Pubkey,
    limit: usize,
) -> Result<Vec<SignatureRecord>, Box<dyn Error + Send + Sync>> {
    let commitment = CommitmentConfig::finalized();

    let entries = client.get_signatures_for_address_with_config(
        program_id,
        GetConfirmedSignaturesForAddress2Config {
            before: None,
            until: None,
            limit: Some(limit),
            commitment: Some(commitment),
        },
    )?;

    let successful = entries
        .into_iter()
        // Failed transactions can contain logs even though all state changes
        // were rolled back, so they cannot enter our projection pipeline.
        .filter(|entry| entry.err.is_none())
        .map(|entry| SignatureRecord {
            signature: entry.signature,
            slot: entry.slot,
            block_time: entry.block_time,
        })
        .collect();

    Ok(successful)
}

pub fn fetch_transaction(
    client: &RpcClient,
    signature_record: &SignatureRecord,
) -> Result<Option<TransactionRecord, Box<dyn Error + Send + Sync>>> {
    let signature: Signature = signature_record.signature.parse()?;
    let transaction = client.get_transaction_with_config(
        &signature,
        RpcTransactionConfig {
            encoding: Some(UiTransactionEncoding::Json),
            commitment: Some(CommitmentConfig::finalized()),
            max_supported_transaction_version: Some(0),
        },
    )?;

    let Some(meta) = transaction.transaction.meta else {
        return Ok(None);
    };

    // Keep this defensive check even though the earlier signature query
    // already filters failed transactions.
    if meta.err.is_some() {
        return Ok(None);
    }

    let logs = match meta.log_messages {
        OptionSerializer::Some(logs) => logs,
        OptionSerializer::None | OptionSerializer::Skip => Vec::new(),
    };

    Ok(Some(TransactionRecord {
        signature: signature_record.signature.clone(),
        slot: transaction.slot,
        block_time: transaction.block_time,
        logs,
    }))
}
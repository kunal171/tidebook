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

pub fn fetch_recent_successful_signatures(
    rpc_url: &str,
    program_id: &Pubkey,
    limit: usize,
) -> Result<Vec<SignatureRecord>, Box<dyn Error + Send + Sync>> {
    let commitment = CommitmentConfig::finalized();

    let client = RpcClient::new_with_commitment(rpc_url.to_owned(), commitment);

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

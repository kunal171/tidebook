//! Durable, bounded historical ingestion orchestration.

use std::{error::Error, future::Future};

use solana_pubkey::Pubkey;
use solana_rpc_client::rpc_client::RpcClient;
use sqlx::PgPool;

use crate::{
    processor::process_signature,
    rpc::{fetch_signature_page, SignaturePage, SignatureRecord},
    storage::{load_backfill_checkpoint, save_backfill_checkpoint},
};

/// Operational bounds for one historical ingestion run.
#[derive(Debug, Clone, Copy)]
pub struct BackfillConfig {
    /// Maximum signatures requested from one RPC page.
    pub signature_page_limit: usize,

    /// Maximum pages committed before yielding to the next process run.
    pub max_pages_per_run: usize,
}

/// Reason the bounded historical run returned successfully.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackfillOutcome {
    /// The durable checkpoint was already marked complete.
    AlreadyComplete,

    /// This run reached the terminal empty RPC page.
    Completed,

    /// More historical pages remain after reaching the configured cap.
    PageLimitReached,
}

type BoxError = Box<dyn Error + Send + Sync>;

/// Resumes finalized history and advances its cursor after complete pages.
///
/// A transaction, decoding, or storage failure returns before the checkpoint
/// update. The next run therefore retries the incomplete page, relying on the
/// source tables' idempotent writes for already committed transactions.
///
/// Pages are discovered from newest to oldest because Solana's `before` cursor
/// walks backward. Records are oldest-first only inside each page; append-only
/// source storage does not depend on global processing order. Future derived
/// projections must explicitly replay source rows in their required order.
pub async fn run_backfill(
    client: &RpcClient,
    pool: &PgPool,
    program_id: &Pubkey,
    config: BackfillConfig,
) -> Result<BackfillOutcome, Box<dyn Error + Send + Sync>> {
    let checkpoint = load_backfill_checkpoint(pool, program_id).await?;

    if checkpoint.completed {
        println!("Historical backfill is already complete.");
        return Ok(BackfillOutcome::AlreadyComplete);
    }

    run_pages(
        checkpoint.before_signature,
        config,
        |before, limit| fetch_signature_page(client, program_id, before.as_ref(), limit),
        |signature| async move { process_signature(client, pool, program_id, &signature).await },
        |before, completed| async move {
            save_backfill_checkpoint(pool, program_id, before.as_ref(), completed)
                .await
                .map_err(|error| Box::new(error) as BoxError)
        },
    )
    .await
}

/// Runs the checkpoint-sensitive page state machine with injectable effects.
///
/// Production supplies Solana RPC and PostgreSQL operations. Tests supply
/// deterministic in-memory functions, allowing failure and page-cap behavior
/// to be verified without a network endpoint or database.
async fn run_pages<Fetch, Process, ProcessFuture, Save, SaveFuture>(
    mut before_signature: Option<solana_signature::Signature>,
    config: BackfillConfig,
    mut fetch_page: Fetch,
    mut process_signature: Process,
    mut save_checkpoint: Save,
) -> Result<BackfillOutcome, BoxError>
where
    Fetch: FnMut(Option<solana_signature::Signature>, usize) -> Result<SignaturePage, BoxError>,
    Process: FnMut(SignatureRecord) -> ProcessFuture,
    ProcessFuture: Future<Output = Result<(), BoxError>>,
    Save: FnMut(Option<solana_signature::Signature>, bool) -> SaveFuture,
    SaveFuture: Future<Output = Result<(), BoxError>>,
{
    for page_number in 1..=config.max_pages_per_run {
        let page = fetch_page(before_signature, config.signature_page_limit)?;
        let successful_count = page.records.len();
        let next_before = page.next_before;

        println!(
            "Backfill page {page_number}: {successful_count} successful finalized transactions"
        );

        for signature in page.records {
            process_signature(signature).await?;
        }

        let completed = next_before.is_none();

        // A terminal empty page has no new cursor. Preserve the previous raw
        // RPC page boundary so a completed checkpoint remains auditable.
        let checkpoint_before = next_before.or(before_signature);

        // This is deliberately the final mutation for a page. If any record
        // above fails, the stored cursor still points at the previous page.
        save_checkpoint(checkpoint_before, completed).await?;

        if completed {
            println!("Historical backfill reached the end of available history.");
            return Ok(BackfillOutcome::Completed);
        }

        println!("Backfill page {page_number} committed; checkpoint advanced.");
        before_signature = next_before;
    }

    println!(
        "Stopped after {} pages; rerun the indexer to continue.",
        config.max_pages_per_run
    );

    Ok(BackfillOutcome::PageLimitReached)
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        future::ready,
        io,
        rc::Rc,
    };

    use solana_signature::Signature;

    use super::*;

    fn signature(byte: u8) -> Signature {
        Signature::from([byte; 64])
    }

    fn record(byte: u8, slot: u64) -> SignatureRecord {
        SignatureRecord {
            signature: signature(byte).to_string(),
            slot,
            block_time: Some(slot as i64),
            transaction_index: None,
        }
    }

    fn config(max_pages_per_run: usize) -> BackfillConfig {
        BackfillConfig {
            signature_page_limit: 20,
            max_pages_per_run,
        }
    }

    #[tokio::test]
    async fn processing_failure_does_not_advance_checkpoint() {
        let saved = Rc::new(RefCell::new(Vec::new()));
        let saved_by_closure = Rc::clone(&saved);

        let result = run_pages(
            None,
            config(1),
            |_, _| {
                Ok(SignaturePage {
                    records: vec![record(1, 10)],
                    next_before: Some(signature(1)),
                })
            },
            |_| {
                ready(Err(
                    Box::new(io::Error::other("injected processing failure")) as BoxError,
                ))
            },
            move |before, completed| {
                saved_by_closure.borrow_mut().push((before, completed));
                ready(Ok(()))
            },
        )
        .await;

        assert!(result.is_err());
        assert!(saved.borrow().is_empty());
    }

    #[tokio::test]
    async fn successful_page_advances_checkpoint_once() {
        let cursor = signature(2);
        let saved = Rc::new(RefCell::new(Vec::new()));
        let saved_by_closure = Rc::clone(&saved);

        run_pages(
            None,
            config(1),
            |_, _| {
                Ok(SignaturePage {
                    records: vec![record(2, 20)],
                    next_before: Some(cursor),
                })
            },
            |_| ready(Ok(())),
            move |before, completed| {
                saved_by_closure.borrow_mut().push((before, completed));
                ready(Ok(()))
            },
        )
        .await
        .expect("successful page must finish");

        assert_eq!(saved.borrow().as_slice(), &[(Some(cursor), false)]);
    }

    #[tokio::test]
    async fn terminal_page_preserves_previous_cursor() {
        let previous = signature(3);
        let saved = Rc::new(RefCell::new(Vec::new()));
        let saved_by_closure = Rc::clone(&saved);

        run_pages(
            Some(previous),
            config(1),
            |_, _| {
                Ok(SignaturePage {
                    records: Vec::new(),
                    next_before: None,
                })
            },
            |_| ready(Ok(())),
            move |before, completed| {
                saved_by_closure.borrow_mut().push((before, completed));
                ready(Ok(()))
            },
        )
        .await
        .expect("terminal page must finish");

        assert_eq!(saved.borrow().as_slice(), &[(Some(previous), true)]);
    }

    #[tokio::test]
    async fn configured_page_cap_stops_the_run() {
        let fetch_count = Rc::new(Cell::new(0_u8));
        let fetch_count_by_closure = Rc::clone(&fetch_count);
        let saved = Rc::new(RefCell::new(Vec::new()));
        let saved_by_closure = Rc::clone(&saved);

        run_pages(
            None,
            config(2),
            move |_, _| {
                let page_number = fetch_count_by_closure.get() + 1;
                fetch_count_by_closure.set(page_number);

                Ok(SignaturePage {
                    records: Vec::new(),
                    next_before: Some(signature(page_number)),
                })
            },
            |_| ready(Ok(())),
            move |before, completed| {
                saved_by_closure.borrow_mut().push((before, completed));
                ready(Ok(()))
            },
        )
        .await
        .expect("bounded run must stop successfully");

        assert_eq!(fetch_count.get(), 2);
        assert_eq!(saved.borrow().len(), 2);
        assert_eq!(saved.borrow()[1], (Some(signature(2)), false));
    }
}

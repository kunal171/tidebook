//! Finalized live-gap discovery and chronological ordering.

use std::{error::Error, future::Future, io};

use solana_pubkey::Pubkey;
use solana_rpc_client::rpc_client::RpcClient;
use solana_signature::Signature;
use sqlx::PgPool;

use crate::{
    processor::process_signature,
    rpc::{fetch_live_signature_page, SignaturePage, SignatureRecord},
    storage::{save_live_checkpoint, LiveCheckpoint},
};

type BoxError = Box<dyn Error + Send + Sync>;

#[derive(Debug, Clone, Copy)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "runtime live catch-up is the next milestone")
)]
pub struct LiveGapConfig {
    /// Maximum signatures requested from one RPC page.
    pub signature_page_limit: usize,

    /// Maximum pages buffered before failing closed.
    pub max_pages: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "runtime live catch-up is the next milestone")
)]
pub enum LiveGapOutcome {
    /// The checkpoint boundary was reached and records are globally oldest-first.
    CaughtUp(Vec<SignatureRecord>),

    /// The safety cap was reached before the checkpoint boundary was confirmed.
    PageLimitReached,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "runtime wiring is the next milestone")
)]
pub enum LiveCatchupOutcome {
    CaughtUp { processed: usize },
    PageLimitReached,
}

/// Buffers a bounded live gap and returns it in chronological processing order.
///
/// RPC pagination discovers newer page chunks before older ones. Nothing is
/// exposed until an empty terminal page confirms the `until` boundary, which
/// prevents callers from advancing past older transactions that were not yet
/// fetched.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "runtime live catch-up is the next milestone")
)]
fn collect_gap_pages<Fetch>(
    config: LiveGapConfig,
    mut fetch_page: Fetch,
) -> Result<LiveGapOutcome, BoxError>
where
    Fetch: FnMut(Option<Signature>, usize) -> Result<SignaturePage, BoxError>,
{
    if config.signature_page_limit == 0 || config.max_pages == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "live-gap limits must be greater than zero",
        )
        .into());
    }

    let mut before = None;
    let mut pages = Vec::new();

    for _ in 0..config.max_pages {
        let requested_before = before;
        let page = fetch_page(requested_before, config.signature_page_limit)?;
        let next_before = page.next_before;

        pages.push(page.records);

        if next_before.is_none() {
            pages.reverse();

            return Ok(LiveGapOutcome::CaughtUp(
                pages.into_iter().flatten().collect(),
            ));
        }

        if next_before == requested_before {
            return Err(io::Error::other("live-gap RPC cursor did not advance").into());
        }

        before = next_before;
    }

    Ok(LiveGapOutcome::PageLimitReached)
}

/// Persists a complete chronological gap and advances after each transaction.
///
/// Source persistence deliberately precedes checkpoint persistence. A crash
/// between those operations causes an idempotent source replay, whereas the
/// opposite order could permanently skip an unpersisted transaction.
async fn process_gap_records<Process, ProcessFuture, Save, SaveFuture>(
    gap: LiveGapOutcome,
    mut process: Process,
    mut save_checkpoint: Save,
) -> Result<LiveCatchupOutcome, BoxError>
where
    Process: FnMut(SignatureRecord) -> ProcessFuture,
    ProcessFuture: Future<Output = Result<(), BoxError>>,
    Save: FnMut(LiveCheckpoint) -> SaveFuture,
    SaveFuture: Future<Output = Result<(), BoxError>>,
{
    let records = match gap {
        LiveGapOutcome::CaughtUp(records) => records,
        LiveGapOutcome::PageLimitReached => {
            return Ok(LiveCatchupOutcome::PageLimitReached);
        }
    };

    let mut processed = 0;

    for record in records {
        let next_checkpoint = LiveCheckpoint {
            signature: record.signature.parse()?,
            slot: record.slot,
            transaction_index: record.transaction_index,
        };

        process(record).await?;
        save_checkpoint(next_checkpoint).await?;
        processed += 1;
    }

    Ok(LiveCatchupOutcome::CaughtUp { processed })
}

#[expect(dead_code, reason = "runtime wiring is the next milestone")]
pub async fn run_live_catchup(
    client: &RpcClient,
    pool: &PgPool,
    program_id: &Pubkey,
    checkpoint: &LiveCheckpoint,
    config: LiveGapConfig,
) -> Result<LiveCatchupOutcome, BoxError> {
    let gap = collect_gap_pages(config, |before, limit| {
        fetch_live_signature_page(
            client,
            program_id,
            before.as_ref(),
            &checkpoint.signature,
            limit,
        )
    })?;

    process_gap_records(
        gap,
        |record| async move { process_signature(client, pool, program_id, &record).await },
        |next_checkpoint| async move {
            save_live_checkpoint(pool, program_id, &next_checkpoint)
                .await
                .map_err(|error| Box::new(error) as BoxError)
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::VecDeque, future::ready, rc::Rc};

    use super::*;

    fn signature(byte: u8) -> Signature {
        Signature::from([byte; 64])
    }

    fn record(byte: u8, slot: u64, transaction_index: u32) -> SignatureRecord {
        SignatureRecord {
            signature: signature(byte).to_string(),
            slot,
            block_time: Some(slot as i64),
            transaction_index: Some(transaction_index),
        }
    }

    fn config(max_pages: usize) -> LiveGapConfig {
        LiveGapConfig {
            signature_page_limit: 2,
            max_pages,
        }
    }

    #[test]
    fn complete_gap_reverses_page_order_without_reversing_records_inside_pages() {
        let newest_page_boundary = signature(3);
        let oldest_page_boundary = signature(1);
        let mut responses = VecDeque::from([
            (
                None,
                SignaturePage {
                    records: vec![record(3, 11, 0), record(4, 11, 1)],
                    next_before: Some(newest_page_boundary),
                },
            ),
            (
                Some(newest_page_boundary),
                SignaturePage {
                    records: vec![record(1, 10, 0), record(2, 10, 1)],
                    next_before: Some(oldest_page_boundary),
                },
            ),
            (
                Some(oldest_page_boundary),
                SignaturePage {
                    records: Vec::new(),
                    next_before: None,
                },
            ),
        ]);

        let outcome = collect_gap_pages(config(3), |before, limit| {
            assert_eq!(limit, 2);
            let (expected_before, page) = responses
                .pop_front()
                .expect("collector must not request an unexpected page");
            assert_eq!(before, expected_before);
            Ok(page)
        })
        .expect("complete gap must collect successfully");

        let LiveGapOutcome::CaughtUp(records) = outcome else {
            panic!("terminal page must produce a complete gap");
        };

        assert_eq!(
            records
                .iter()
                .map(|record| record.signature.as_str())
                .collect::<Vec<_>>(),
            [signature(1), signature(2), signature(3), signature(4)]
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        );
        assert!(responses.is_empty());
    }

    #[test]
    fn empty_gap_is_caught_up_without_records() {
        let outcome = collect_gap_pages(config(1), |before, limit| {
            assert_eq!(before, None);
            assert_eq!(limit, 2);
            Ok(SignaturePage {
                records: Vec::new(),
                next_before: None,
            })
        })
        .expect("empty gap must be valid");

        assert_eq!(outcome, LiveGapOutcome::CaughtUp(Vec::new()));
    }

    #[test]
    fn page_cap_hides_incomplete_newest_records() {
        let outcome = collect_gap_pages(config(1), |before, _| {
            assert_eq!(before, None);
            Ok(SignaturePage {
                records: vec![record(2, 10, 1)],
                next_before: Some(signature(2)),
            })
        })
        .expect("reaching the configured cap is an explicit outcome");

        assert_eq!(outcome, LiveGapOutcome::PageLimitReached);
    }

    #[test]
    fn zero_limits_are_rejected_before_fetching() {
        for config in [
            LiveGapConfig {
                signature_page_limit: 0,
                max_pages: 1,
            },
            LiveGapConfig {
                signature_page_limit: 1,
                max_pages: 0,
            },
        ] {
            let error = collect_gap_pages(config, |_, _| {
                panic!("invalid configuration must fail before RPC access")
            })
            .expect_err("zero limits must be rejected");

            assert_eq!(
                error
                    .downcast_ref::<io::Error>()
                    .expect("configuration failure must remain an I/O error")
                    .kind(),
                io::ErrorKind::InvalidInput
            );
        }
    }

    #[test]
    fn repeated_cursor_is_rejected() {
        let repeated = signature(1);
        let mut call = 0;

        let error = collect_gap_pages(config(2), |before, _| {
            call += 1;
            Ok(SignaturePage {
                records: Vec::new(),
                next_before: match call {
                    1 => {
                        assert_eq!(before, None);
                        Some(repeated)
                    }
                    2 => {
                        assert_eq!(before, Some(repeated));
                        Some(repeated)
                    }
                    _ => panic!("cursor failure must stop collection"),
                },
            })
        })
        .expect_err("a stalled cursor must fail closed");

        assert_eq!(
            error
                .downcast_ref::<io::Error>()
                .expect("cursor failure must remain an I/O error")
                .kind(),
            io::ErrorKind::Other
        );
    }

    #[test]
    fn fetch_failure_is_propagated() {
        let error = collect_gap_pages(config(1), |_, _| {
            Err(io::Error::other("injected RPC failure").into())
        })
        .expect_err("RPC failure must stop collection");

        assert_eq!(error.to_string(), "injected RPC failure");
    }

    #[tokio::test]
    async fn complete_gap_processes_before_checkpointing_each_record() {
        let operations = Rc::new(RefCell::new(Vec::new()));
        let process_operations = Rc::clone(&operations);
        let save_operations = Rc::clone(&operations);

        let outcome = process_gap_records(
            LiveGapOutcome::CaughtUp(vec![record(1, 10, 0), record(2, 10, 1)]),
            move |record| {
                process_operations
                    .borrow_mut()
                    .push(format!("process:{}", record.signature));
                ready(Ok(()))
            },
            move |checkpoint| {
                save_operations
                    .borrow_mut()
                    .push(format!("checkpoint:{}", checkpoint.signature));
                ready(Ok(()))
            },
        )
        .await
        .expect("complete gap must process successfully");

        assert_eq!(outcome, LiveCatchupOutcome::CaughtUp { processed: 2 });
        assert_eq!(
            *operations.borrow(),
            vec![
                format!("process:{}", signature(1)),
                format!("checkpoint:{}", signature(1)),
                format!("process:{}", signature(2)),
                format!("checkpoint:{}", signature(2)),
            ]
        );
    }

    #[tokio::test]
    async fn page_cap_runs_no_processing_or_checkpoint_effects() {
        let outcome = process_gap_records(
            LiveGapOutcome::PageLimitReached,
            |_| {
                panic!("incomplete newest records must not be processed");
                #[allow(unreachable_code)]
                ready(Ok(()))
            },
            |_| {
                panic!("incomplete newest records must not advance the checkpoint");
                #[allow(unreachable_code)]
                ready(Ok(()))
            },
        )
        .await
        .expect("page cap must remain an explicit non-error outcome");

        assert_eq!(outcome, LiveCatchupOutcome::PageLimitReached);
    }

    #[tokio::test]
    async fn processing_failure_stops_before_its_checkpoint_and_later_records() {
        let operations = Rc::new(RefCell::new(Vec::new()));
        let process_operations = Rc::clone(&operations);
        let save_operations = Rc::clone(&operations);
        let failing_signature = signature(2).to_string();

        let error = process_gap_records(
            LiveGapOutcome::CaughtUp(vec![record(1, 10, 0), record(2, 10, 1), record(3, 11, 0)]),
            move |record| {
                process_operations
                    .borrow_mut()
                    .push(format!("process:{}", record.signature));

                if record.signature == failing_signature {
                    ready(Err(io::Error::other("injected processing failure").into()))
                } else {
                    ready(Ok(()))
                }
            },
            move |checkpoint| {
                save_operations
                    .borrow_mut()
                    .push(format!("checkpoint:{}", checkpoint.signature));
                ready(Ok(()))
            },
        )
        .await
        .expect_err("processing failure must stop the live gap");

        assert_eq!(error.to_string(), "injected processing failure");
        assert_eq!(
            *operations.borrow(),
            vec![
                format!("process:{}", signature(1)),
                format!("checkpoint:{}", signature(1)),
                format!("process:{}", signature(2)),
            ]
        );
    }

    #[tokio::test]
    async fn checkpoint_failure_stops_before_later_records() {
        let operations = Rc::new(RefCell::new(Vec::new()));
        let process_operations = Rc::clone(&operations);
        let save_operations = Rc::clone(&operations);
        let failing_signature = signature(2);

        let error = process_gap_records(
            LiveGapOutcome::CaughtUp(vec![record(1, 10, 0), record(2, 10, 1), record(3, 11, 0)]),
            move |record| {
                process_operations
                    .borrow_mut()
                    .push(format!("process:{}", record.signature));
                ready(Ok(()))
            },
            move |checkpoint| {
                save_operations
                    .borrow_mut()
                    .push(format!("checkpoint:{}", checkpoint.signature));

                if checkpoint.signature == failing_signature {
                    ready(Err(io::Error::other("injected checkpoint failure").into()))
                } else {
                    ready(Ok(()))
                }
            },
        )
        .await
        .expect_err("checkpoint failure must stop the live gap");

        assert_eq!(error.to_string(), "injected checkpoint failure");
        assert_eq!(
            *operations.borrow(),
            vec![
                format!("process:{}", signature(1)),
                format!("checkpoint:{}", signature(1)),
                format!("process:{}", signature(2)),
                format!("checkpoint:{}", signature(2)),
            ]
        );
    }
}

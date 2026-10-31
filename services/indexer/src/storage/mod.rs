//! Durable PostgreSQL storage for finalized source records and sync progress.

mod checkpoint;
mod connection;
mod live_checkpoint;
mod models;
mod writer;

pub use checkpoint::{load_backfill_checkpoint, save_backfill_checkpoint};
pub use connection::connect;
// The write API is intentionally exposed with the read API before the live
// consumer is wired in the next milestone. Remove this narrow allowance once
// runtime ingestion calls `save_live_checkpoint`.
#[allow(
    unused_imports,
    reason = "save_live_checkpoint is consumed by the next live-sync milestone"
)]
pub use live_checkpoint::{load_live_checkpoint, save_live_checkpoint, LiveCheckpoint};
pub use writer::persist_transaction;

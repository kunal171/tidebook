//! Durable PostgreSQL storage for finalized Tidebook source records.

mod connection;
mod models;
mod writer;

pub use connection::connect;
pub use writer::persist_transaction;

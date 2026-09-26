//! Domain-oriented access to Tidebook's stable Anchor error catalog.
//!
//! Anchor 1.2 accepts only one `#[error_code]` enum when generating an IDL.
//! `catalog` therefore owns the ABI, while each public domain module re-exports
//! only the variants relevant to that service.

pub mod admin;
pub mod balance;
pub mod market;
pub mod matching;
pub mod order;

mod catalog;

pub use catalog::TidebookError;

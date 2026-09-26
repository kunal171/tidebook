//! Events emitted by successful Tidebook state transitions.
//!
//! Events are an observation API, not protocol state. Consumers must process
//! them only from successful transactions because failed transactions can
//! retain diagnostic logs even though every account mutation was rolled back.

mod admin;
mod balance;
mod fill;
mod market;
mod order;

pub use admin::*;
pub use balance::*;
pub use fill::*;
pub use market::*;
pub use order::*;

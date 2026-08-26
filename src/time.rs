//! Clock types that work on every target this builds for.
//!
//! `std::time::Instant::now()` panics on `wasm32-unknown-unknown` with "time not
//! implemented on this platform" — the target has no clock of its own. `web-time`
//! is a drop-in backed by `performance.now()`, so the pipeline's many elapsed-time
//! measurements keep working in a browser without any of them growing a `cfg`.
//!
//! `Duration` is pure arithmetic and comes from `std` everywhere.

pub use std::time::Duration;

#[cfg(not(target_arch = "wasm32"))]
pub use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[cfg(target_arch = "wasm32")]
pub use web_time::{Instant, SystemTime, UNIX_EPOCH};

//! Source description and construction for the waterfall binary.
//!
//! A [`ConnectRequest`] fully describes how to bring up a front end; [`connect`]
//! builds, tunes, and starts it. The request is created either from CLI args
//! (auto-connect on launch) or from the in-app connection form, and is the unit
//! we persist as a "recent host".

mod cli;
mod connection;
mod iq_bridge;
pub mod controls_dispatch;
mod device;
mod kinds;
mod settings;

#[cfg(test)]
mod mock_hal_tests;

pub use cli::request_from_args;
pub use connection::{connect, ConnectRequest, SourceKind};
pub use device::Connection;
pub use kinds::{
    is_local_source, source_kind_from_index, source_kind_index, source_kind_label,
    source_kind_labels,
};
pub use settings::{AirspySettings, KiwiSettings, QmxSettings, RtlSdrSettings};

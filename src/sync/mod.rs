//! Cross-thread handoffs that never make one side wait for the other.

mod latest;

pub use latest::{latest_cell, LatestReader, LatestWriter};

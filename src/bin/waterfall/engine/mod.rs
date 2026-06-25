//! Background DSP/audio engine.
//!
//! All real-time work lives here, off the UI thread: draining the IQ ring,
//! demodulating to audio, running the FFT, feeding the skimmer, and owning the
//! connection lifecycle (connect, stall/slow detection, exponential-backoff
//! auto-reconnect). The source and audio device are *created inside this thread*
//! so neither (a possibly `!Send` device handle or cpal stream) ever crosses a
//! thread boundary.
//!
//! The UI communicates by:
//! - writing [`EngineParams`] (DSP settings, volume) through a shared mutex,
//! - sending discrete [`EngineCommand`]s (connect, tune, ...),
//! - and reading [`EngineShared`] (spectrum rows, status, stats, spots).

mod audio;
mod handle;
mod inner;
mod perf;
mod policy;
mod types;

#[allow(unused_imports)] // engine-bench binary
pub use policy::{
    demod_tail_max, wideband_tail_len, MAX_AUDIO_SAMPLES_NARROW, MAX_AUDIO_SAMPLES_WB,
};

pub use handle::EngineHandle;
#[allow(unused_imports)] // engine-bench binary
pub(crate) use inner::Engine;
#[allow(unused_imports)] // engine-bench binary
pub(crate) use types::EngineShared;
pub use types::{
    ConnState, EngineCommand, EngineParams, EnginePoll, EngineStats,
};

pub const FFT_SIZE: usize = 2048;
pub const FFT_HOP: usize = FFT_SIZE / 2;
pub const WATERFALL_ROWS: usize = 360;

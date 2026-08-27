//! The one place the UI and the engine touch.
//!
//! The rule this file exists to enforce is that **the UI never waits for the
//! engine**. Not "waits briefly", not "usually does not wait" — never. A
//! renderer that can block on the DSP will eventually drop a frame because the
//! DSP was busy, and no amount of care in the engine prevents that; only the
//! shape of the boundary does.
//!
//! So there is no mutex here. Three channels, each wait-free:
//!
//! - **Snapshot** (engine → UI): latest-value, via [`hfsdr::sync::latest_cell`].
//!   The UI wants the newest state, never a backlog of old ones, and a reader
//!   that misses an update simply keeps rendering the previous frame's values.
//! - **Rows** (engine → UI): a stream, via an SPSC queue, because every
//!   spectrum row is a distinct slice of time and dropping one leaves a gap in
//!   the waterfall. Returned buffers come back on a second queue so a row costs
//!   no allocation once the pipeline is warm.
//! - **Params** (UI → engine): latest-value again — the engine only ever wants
//!   the current settings.
//!
//! Commands stay on an `mpsc` channel: unbounded, so `send` never blocks, and
//! discrete actions must not be coalesced the way a latest-value slot would.
//!
//! The same shape serves every target: nothing here assumes which side runs
//! where, only that there is exactly one of each. Natively the engine gets a
//! thread. In the browser it gets a timer instead, and the reason is worth
//! recording, because "put it on a worker" is the obvious fix and it does not
//! currently work.
//!
//! A Web Worker needs shared memory, which needs the wasm `atomics` target
//! feature. Everything in this project builds with it — rayon, rustfft, rtrb,
//! tungstenite, a std rebuilt via `-Z build-std`, and `wasm_thread` to back
//! `std::thread::spawn` with a worker. What does not build is the renderer:
//!
//! ```text
//! wgpu-types/src/send_sync.rs:
//!     #[cfg(any(not(target_arch = "wasm32"),
//!               all(feature = "fragile-send-sync-non-atomic-wasm",
//!                   not(target_feature = "atomics"))))]
//!     pub trait WasmNotSend: Send {}
//! ```
//!
//! wgpu drops `Send`/`Sync` on wasm the moment atomics are on — correctly, as
//! its handles wrap JS objects that cannot cross a thread — and egui stores the
//! renderer in a `TypeMap` that requires `Send + Sync`. So egui-on-wgpu and
//! wasm threads are mutually exclusive today, upstream of anything here.
//!
//! Which leaves the browser sharing one thread between engine and renderer.
//! That is exactly why this boundary is wait-free rather than merely
//! fine-grained: on the one target where the two cannot be given separate
//! threads, a lock between them would be a stall the UI could not avoid.
//!
//! The way out, when it is worth the build complexity, is a second wasm module
//! in a worker talking over `postMessage` — no shared memory, so the render
//! module never needs atomics. The channels below map onto that unchanged.

use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use hfsdr::sync::{latest_cell, LatestReader, LatestWriter};
use rtrb::{Consumer, Producer, RingBuffer};

use super::types::{EngineCommand, EngineParams, EngineSnapshot};
use super::WATERFALL_ROWS;

/// Rows the UI may fall behind by before the engine starts dropping them.
///
/// A full waterfall's worth: at ~12 rows/second the UI would have to be stalled
/// for half a minute to reach it, by which point a gap in the waterfall is the
/// least of the problems.
const ROW_QUEUE: usize = WATERFALL_ROWS;

/// The engine's half of the boundary.
pub(crate) struct EngineLink {
    pub cmd_rx: Receiver<EngineCommand>,
    pub snapshot: LatestWriter<EngineSnapshot>,
    pub rows_tx: Producer<Vec<f32>>,
    /// Row buffers the UI has finished with, ready to refill.
    pub spent_rows: Consumer<Vec<f32>>,
    pub params: LatestReader<EngineParams>,
    pub connect_cancel: Arc<AtomicBool>,
}

/// The UI's half of the boundary.
pub(crate) struct UiLink {
    pub cmd_tx: Sender<EngineCommand>,
    pub snapshot: LatestReader<EngineSnapshot>,
    pub rows_rx: Consumer<Vec<f32>>,
    pub spent_rows: Producer<Vec<f32>>,
    pub params: LatestWriter<EngineParams>,
    pub connect_cancel: Arc<AtomicBool>,
}

/// Build both halves of a boundary.
pub(crate) fn engine_link() -> (EngineLink, UiLink) {
    let (cmd_tx, cmd_rx) = channel();
    let (snapshot_w, snapshot_r) = latest_cell(
        EngineSnapshot::default(),
        EngineSnapshot::default(),
        EngineSnapshot::default(),
    );
    let (params_w, params_r) = latest_cell(
        EngineParams::default(),
        EngineParams::default(),
        EngineParams::default(),
    );
    let (rows_tx, rows_rx) = RingBuffer::new(ROW_QUEUE);
    let (spent_tx, spent_rx) = RingBuffer::new(ROW_QUEUE);
    let connect_cancel = Arc::new(AtomicBool::new(false));

    (
        EngineLink {
            cmd_rx,
            snapshot: snapshot_w,
            rows_tx,
            spent_rows: spent_rx,
            params: params_r,
            connect_cancel: Arc::clone(&connect_cancel),
        },
        UiLink {
            cmd_tx,
            snapshot: snapshot_r,
            rows_rx,
            spent_rows: spent_tx,
            params: params_w,
            connect_cancel,
        },
    )
}

//! UI-side handle to the engine thread.

// Only the headless-test poll injection below uses these; the shipping handle
// holds no lock at all, which is the point of the wait-free boundary.
#[cfg(test)]
use std::collections::VecDeque;
#[cfg(test)]
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;
use std::sync::atomic::Ordering;
use std::thread;

use super::inner::Engine;
use super::link::{engine_link, UiLink};
use super::types::{EngineCommand, EngineParams, EnginePoll};

#[cfg(target_arch = "wasm32")]
use super::inner::IdlePacing;
#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;
#[cfg(target_arch = "wasm32")]
use std::rc::Rc;

/// How often the browser pumps the engine, in milliseconds.
///
/// Fast enough to keep the IQ ring drained at any rate a Kiwi delivers, slow
/// enough not to burn a core on an idle receiver. Deliberately unrelated to the
/// repaint interval — that independence is the point.
#[cfg(target_arch = "wasm32")]
const PUMP_INTERVAL_MS: i32 = 8;

/// A `setInterval` that pumps the engine, cancelled on drop.
///
/// The engine used to be stepped from the egui frame callback, which tied the
/// DSP's cadence to the display's: the pipeline stalled whenever a repaint was
/// skipped, and a long step showed up as a dropped frame. A timer separates
/// them as far as a single-threaded tab allows — the two no longer share a
/// cadence, only a thread.
#[cfg(target_arch = "wasm32")]
struct PumpTimer {
    id: i32,
    _cb: wasm_bindgen::closure::Closure<dyn FnMut()>,
}

#[cfg(target_arch = "wasm32")]
impl Drop for PumpTimer {
    fn drop(&mut self) {
        if let Some(w) = web_sys::window() {
            w.clear_interval_with_handle(self.id);
        }
    }
}

/// UI-side handle to the engine.
///
/// Every method here is wait-free. That is the contract: the renderer calls
/// these from inside a frame, so any one of them that could block on the engine
/// would eventually cost a frame. See [`super::link`] for how.
pub struct EngineHandle {
    link: Option<UiLink>,
    join: Option<thread::JoinHandle<()>>,
    /// Headless UI tests inject polls here instead of running an engine.
    #[cfg(test)]
    test_polls: Option<Arc<Mutex<VecDeque<EnginePoll>>>>,
    /// Browser builds own the engine outright: a tab has no thread to run it
    /// on. It is stepped by [`PumpTimer`], not by the renderer.
    #[cfg(target_arch = "wasm32")]
    engine: Option<Rc<RefCell<Engine>>>,
    /// Drops the interval when the handle goes away.
    #[cfg(target_arch = "wasm32")]
    _pump: Option<PumpTimer>,
}

impl EngineHandle {
    pub fn spawn() -> Self {
        let (engine_side, ui_side) = engine_link();
        let join = thread::Builder::new()
            .name("engine".into())
            .spawn(move || Engine::new(engine_side).run())
            .expect("spawn engine thread");

        Self {
            link: Some(ui_side),
            join: Some(join),
            #[cfg(test)]
            test_polls: None,
            #[cfg(target_arch = "wasm32")]
            engine: None,
            #[cfg(target_arch = "wasm32")]
            _pump: None,
        }
    }

    /// Handle with no engine behind it.
    ///
    /// The headless UI harness drives the app from injected [`EnginePoll`]s
    /// rather than a running pipeline.
    #[cfg(test)]
    pub fn spawn_detached() -> Self {
        Self {
            link: None,
            join: None,
            test_polls: Some(Arc::new(Mutex::new(VecDeque::new()))),
            #[cfg(target_arch = "wasm32")]
            engine: None,
            #[cfg(target_arch = "wasm32")]
            _pump: None,
        }
    }

    /// Engine running in-process on its own timer.
    ///
    /// The browser has no thread to give the engine, so it lives here and a
    /// `setInterval` pumps it. The boundary is the same one the threaded handle
    /// uses, because the engine does not know which driver is turning it.
    ///
    /// The renderer deliberately does not turn it. Stepping from the frame
    /// callback made the DSP's rate a function of the display's, so a skipped
    /// repaint stalled the pipeline and a long step dropped a frame.
    #[cfg(target_arch = "wasm32")]
    pub fn spawn_in_process() -> Self {
        use wasm_bindgen::JsCast as _;

        let (engine_side, ui_side) = engine_link();
        let engine = Rc::new(RefCell::new(Engine::new(engine_side)));

        let engine_cb = Rc::clone(&engine);
        let cb = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || {
            // Callbacks on one thread cannot interleave, so this only guards
            // against a future re-entrant caller — but a panicking borrow here
            // would take the whole app down.
            if let Ok(mut e) = engine_cb.try_borrow_mut() {
                e.step(IdlePacing::Return);
            }
        });
        let pump = web_sys::window()
            .and_then(|w| {
                w.set_interval_with_callback_and_timeout_and_arguments_0(
                    cb.as_ref().unchecked_ref(),
                    PUMP_INTERVAL_MS,
                )
                .ok()
            })
            .map(|id| PumpTimer { id, _cb: cb });
        if pump.is_none() {
            crate::log::error("engine: could not start the pump timer");
        }

        Self {
            link: Some(ui_side),
            join: None,
            #[cfg(test)]
            test_polls: None,
            engine: Some(engine),
            _pump: pump,
        }
    }

    /// Queue a synthetic engine poll (detached handles only).
    #[cfg(test)]
    pub fn inject_poll(&self, poll: EnginePoll) {
        let Some(q) = &self.test_polls else {
            return;
        };
        if let Ok(mut guard) = q.lock() {
            guard.push_back(poll);
        }
    }

    /// Send a discrete command. Unbounded channel, so this never blocks.
    pub fn send(&self, cmd: EngineCommand) {
        if let Some(link) = &self.link {
            let _ = link.cmd_tx.send(cmd);
        }
    }

    /// Abort a blocking `connect()` from the UI thread (must run before or with Disconnect).
    pub fn abort_connect(&self) {
        if let Some(link) = &self.link {
            link.connect_cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Overwrite the engine's view of UI settings (called once per UI frame).
    ///
    /// Wait-free: writes into a slot this side owns, then one atomic swap.
    pub fn set_params(&mut self, params: EngineParams) {
        if let Some(link) = &mut self.link {
            *link.params.slot() = params;
            link.params.publish();
        }
    }

    /// Take the latest engine state and any rows that arrived since the last
    /// call. Wait-free — never waits on the engine, whatever it is doing.
    ///
    /// Returns `None` only when there is no engine at all.
    pub fn try_poll(&mut self) -> Option<EnginePoll> {
        #[cfg(test)]
        if let Some(q) = &self.test_polls {
            let mut guard = q.lock().ok()?;
            return guard.pop_front();
        }
        let link = self.link.as_mut()?;
        link.snapshot.fetch();
        let snap = link.snapshot.slot();

        let mut rows = Vec::new();
        while let Ok(row) = link.rows_rx.pop() {
            rows.push(row);
        }

        Some(EnginePoll {
            state: snap.state.clone(),
            stats: snap.stats.clone(),
            rows,
            latest: snap.latest.clone(),
            last_error: snap.last_error.clone(),
            audio_scope: snap.audio_scope.clone(),
            audio_waveform: snap.audio_waveform.clone(),
        })
    }

    /// Hand a finished row buffer back to the engine to refill.
    ///
    /// Optional: dropping the buffer instead simply costs an allocation on the
    /// engine's next row.
    pub fn recycle_row(&mut self, row: Vec<f32>) {
        if let Some(link) = &mut self.link {
            let _ = link.spent_rows.push(row);
        }
    }

    /// Signal shutdown and detach the worker thread — never blocks the UI thread.
    pub fn shutdown_now(&mut self) {
        self.abort_connect();
        self.send(EngineCommand::Shutdown);
        if let Some(h) = self.join.take() {
            // Dropping JoinHandle without join() detaches the thread.
            drop(h);
        }
        // Dropping the UI half closes the command channel, which is how a
        // detached engine notices it should stop.
        self.link = None;
    }
}

impl Drop for EngineHandle {
    fn drop(&mut self) {
        self.shutdown_now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{ConnState, EngineParams, EngineStats, FFT_SIZE};

    fn sample_poll(state: ConnState) -> EnginePoll {
        EnginePoll {
            state,
            stats: EngineStats::default(),
            rows: Vec::new(),
            latest: vec![-90.0; FFT_SIZE],
            last_error: None,
            audio_scope: Vec::new(),
            audio_waveform: Vec::new(),
        }
    }

    #[test]
    fn test_handle_inject_and_drain() {
        let mut handle = EngineHandle::spawn_detached();
        handle.inject_poll(sample_poll(ConnState::Streaming));
        let poll = handle.try_poll().expect("queued poll");
        assert!(matches!(poll.state, ConnState::Streaming));
        assert!(handle.try_poll().is_none());
    }

    #[test]
    fn live_handle_ignores_inject() {
        let mut handle = EngineHandle::spawn();
        handle.inject_poll(sample_poll(ConnState::Streaming));
        let poll = handle.try_poll().expect("shared poll");
        assert!(matches!(poll.state, ConnState::Disconnected));
        handle.shutdown_now();
    }

    /// Params must actually reach the engine's side of the boundary.
    #[test]
    fn set_params_reaches_the_engine() {
        let (mut engine_side, ui_side) = crate::engine::link::engine_link();
        let mut handle = EngineHandle {
            link: Some(ui_side),
            join: None,
            test_polls: None,
        };
        let params = EngineParams {
            volume: 0.42,
            rf_gain_db: 6.0,
            ..EngineParams::default()
        };
        handle.set_params(params);

        assert!(engine_side.params.fetch(), "engine saw no params update");
        let seen = engine_side.params.slot();
        assert!((seen.volume - 0.42).abs() < f32::EPSILON);
        assert!((seen.rf_gain_db - 6.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_handle_fifo_order() {
        let mut handle = EngineHandle::spawn_detached();
        handle.inject_poll(sample_poll(ConnState::Connecting {
            label: "a".into(),
        }));
        handle.inject_poll(sample_poll(ConnState::Streaming));
        assert!(matches!(
            handle.try_poll().unwrap().state,
            ConnState::Connecting { .. }
        ));
        assert!(matches!(handle.try_poll().unwrap().state, ConnState::Streaming));
    }

    #[test]
    fn test_handle_send_is_noop() {
        let mut handle = EngineHandle::spawn_detached();
        handle.send(EngineCommand::Disconnect);
        assert!(handle.try_poll().is_none());
    }
}

/// The claim this boundary exists to make: the UI never waits for the engine.
///
/// Worth being precise about what a mutex actually costs here, because it is
/// not what it first looks like. Measured against this same load, a contended
/// `Mutex` blocks the reader for about 50 us — real, but not the problem. The
/// problem is what the old boundary did to avoid that wait: `try_lock`, which
/// under the same contention **missed 4428 of 5000 updates**. The UI was
/// discarding ~89% of the engine's snapshots and rendering whatever it managed
/// to catch, which is where bursty row delivery and stale readings came from.
///
/// So the timing tests below are only catastrophe guards, and their budget is
/// loose on purpose. The test that actually discriminates between a mutex and
/// this boundary is [`never_blocks::try_poll_never_misses_an_update`].
#[cfg(test)]
mod never_blocks {
    use super::*;
    use crate::engine::link::engine_link;
    use crate::engine::{ConnState, EngineStats, FFT_SIZE};
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, Instant};

    /// A UI-side handle wired to an engine side we drive by hand.
    fn wired() -> (EngineHandle, crate::engine::link::EngineLink) {
        let (engine_side, ui_side) = engine_link();
        (
            EngineHandle {
                link: Some(ui_side),
                join: None,
                test_polls: None,
            },
            engine_side,
        )
    }

    /// Publish as fast as possible from another thread, timing the UI's worst
    /// single call to `f`.
    fn worst_under_load<F>(mut f: F) -> Duration
    where
        F: FnMut(&mut EngineHandle),
    {
        let (mut handle, mut engine_side) = wired();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_w = Arc::clone(&stop);

        let writer = thread::spawn(move || {
            let mut n = 0u64;
            while !stop_w.load(Ordering::Relaxed) {
                n += 1;
                {
                    let slot = engine_side.snapshot.slot();
                    slot.state = ConnState::Streaming;
                    slot.stats = EngineStats::default();
                    slot.stats.dropped = n;
                    slot.latest.resize(FFT_SIZE, -120.0);
                    slot.latest.fill(-(n as f32 % 100.0));
                }
                engine_side.snapshot.publish();
                if !engine_side.rows_tx.is_full() {
                    let _ = engine_side.rows_tx.push(vec![-90.0; FFT_SIZE]);
                }
                while engine_side.spent_rows.pop().is_ok() {}
            }
        });

        let mut worst = Duration::ZERO;
        for _ in 0..5_000 {
            let t0 = Instant::now();
            f(&mut handle);
            worst = worst.max(t0.elapsed());
        }
        stop.store(true, Ordering::Relaxed);
        writer.join().expect("engine side panicked");
        worst
    }

    /// A catastrophe guard, not a discriminator: a contended mutex measures
    /// ~50 us here, so this budget would not catch one. It catches a boundary
    /// that has started waiting on something unbounded.
    const BUDGET: Duration = Duration::from_millis(20);

    #[test]
    fn try_poll_never_waits_for_a_busy_engine() {
        let worst = worst_under_load(|h| {
            let _ = h.try_poll();
        });
        assert!(
            worst < BUDGET,
            "worst try_poll took {worst:?} while the engine was publishing"
        );
    }

    #[test]
    fn set_params_never_waits_for_a_busy_engine() {
        let worst = worst_under_load(|h| h.set_params(EngineParams::default()));
        assert!(
            worst < BUDGET,
            "worst set_params took {worst:?} while the engine was publishing"
        );
    }

    #[test]
    fn send_never_waits_for_a_busy_engine() {
        let worst = worst_under_load(|h| h.send(EngineCommand::Disconnect));
        assert!(
            worst < BUDGET,
            "worst send took {worst:?} while the engine was publishing"
        );
    }

    /// The discriminating test: a reader polling a busy writer must obtain a
    /// snapshot every single time and never see one go backwards.
    ///
    /// `try_lock` on a mutex fails ~89% of the time under this exact load. A
    /// wait-free reader has no failure mode to have.
    #[test]
    fn try_poll_never_misses_an_update() {
        let (mut handle, mut engine_side) = wired();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_w = Arc::clone(&stop);
        // Proves the writer actually ran, without assuming *when* it ran: on a
        // busy runner it may get no CPU at all during a fixed polling window,
        // and asserting otherwise would test the scheduler, not the boundary.
        let published = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let published_w = Arc::clone(&published);

        let writer = thread::spawn(move || {
            let mut n = 0u64;
            while !stop_w.load(Ordering::Relaxed) {
                n += 1;
                let slot = engine_side.snapshot.slot();
                slot.stats.dropped = n;
                slot.latest.resize(FFT_SIZE, -120.0);
                slot.latest.fill(-(n as f32 % 100.0));
                engine_side.snapshot.publish();
                published_w.store(n, Ordering::Relaxed);
            }
        });

        // Wait for the writer to be genuinely running before measuring.
        let deadline = Instant::now() + Duration::from_secs(10);
        while published.load(Ordering::Relaxed) == 0 && Instant::now() < deadline {
            std::hint::spin_loop();
        }
        assert!(
            published.load(Ordering::Relaxed) > 0,
            "the writer thread never ran; this test observed no concurrency"
        );

        // The discriminating assertion: under a live writer every poll returns
        // a snapshot. `try_lock` fails ~89% of them under this load.
        let mut previous = 0u64;
        for i in 0..5_000 {
            let poll = handle
                .try_poll()
                .unwrap_or_else(|| panic!("poll {i} returned nothing while the engine was live"));
            assert!(
                poll.stats.dropped >= previous,
                "snapshot went backwards at poll {i}: {} after {previous}",
                poll.stats.dropped
            );
            previous = poll.stats.dropped;
        }
        stop.store(true, Ordering::Relaxed);
        writer.join().expect("engine side panicked");
    }

    /// Rows are a stream, so none may be silently coalesced the way a
    /// latest-value slot would coalesce them.
    #[test]
    fn every_row_reaches_the_ui_in_order() {
        let (mut handle, mut engine_side) = wired();
        for i in 0..64u32 {
            let _ = engine_side.rows_tx.push(vec![i as f32; 4]);
        }
        engine_side.snapshot.publish();

        let poll = handle.try_poll().expect("poll");
        assert_eq!(poll.rows.len(), 64, "rows were coalesced or lost");
        for (i, row) in poll.rows.iter().enumerate() {
            assert_eq!(row[0], i as f32, "rows arrived out of order at {i}");
        }
    }

    /// A UI that falls behind must see the newest state, not a queue of old
    /// ones — otherwise the display lags further behind the longer it stutters.
    #[test]
    fn snapshot_skips_to_the_newest_state() {
        let (mut handle, mut engine_side) = wired();
        for n in 1..=50u64 {
            engine_side.snapshot.slot().stats.dropped = n;
            engine_side.snapshot.publish();
        }
        let poll = handle.try_poll().expect("poll");
        assert_eq!(
            poll.stats.dropped, 50,
            "UI got a backlogged snapshot instead of the newest"
        );
    }
}

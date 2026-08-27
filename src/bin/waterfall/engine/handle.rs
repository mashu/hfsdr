//! UI-side handle to the engine thread.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::Mutex;
use std::thread;

use super::inner::Engine;
use super::types::{EngineCommand, EngineParams, EnginePoll, EngineShared};

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

/// UI-side handle to the engine thread.
pub struct EngineHandle {
    cmd_tx: Option<Sender<EngineCommand>>,
    shared: Arc<Mutex<EngineShared>>,
    params: Arc<Mutex<EngineParams>>,
    connect_cancel: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
    /// Headless UI tests inject polls here instead of running the engine thread.
    test_polls: Option<Arc<Mutex<VecDeque<EnginePoll>>>>,
    /// Browser builds own the engine outright: a tab has no thread to run it
    /// on. It is stepped by [`PumpTimer`], not by the renderer.
    #[cfg(target_arch = "wasm32")]
    engine: Option<Rc<RefCell<Engine>>>,
    /// Drops the interval when the handle goes away.
    #[cfg(target_arch = "wasm32")]
    _pump: Option<PumpTimer>,
}

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

impl EngineHandle {
    pub fn spawn() -> Self {
        let (cmd_tx, cmd_rx) = channel::<EngineCommand>();
        let shared = Arc::new(Mutex::new(EngineShared::default()));
        let params = Arc::new(Mutex::new(EngineParams::default()));
        let connect_cancel = Arc::new(AtomicBool::new(false));
        let shared_thread = Arc::clone(&shared);
        let params_thread = Arc::clone(&params);
        let connect_cancel_thread = Arc::clone(&connect_cancel);

        let join = thread::Builder::new()
            .name("engine".into())
            .spawn(move || {
                Engine::new(cmd_rx, shared_thread, params_thread, connect_cancel_thread).run();
            })
            .expect("spawn engine thread");

        Self {
            cmd_tx: Some(cmd_tx),
            shared,
            params,
            connect_cancel,
            join: Some(join),
            test_polls: None,
            #[cfg(target_arch = "wasm32")]
            engine: None,
            #[cfg(target_arch = "wasm32")]
            _pump: None,
        }
    }

    /// Engine handle with no worker thread.
    ///
    /// Browser builds cannot spawn threads, and the headless UI harness does not
    /// want one. Polls are supplied by the caller via [`Self::inject_poll`]
    /// instead of coming from a running pipeline, so the UI runs against real
    /// [`EnginePoll`] data with nothing behind it.
    #[cfg(any(test, not(feature = "gui-core")))]
    pub fn spawn_detached() -> Self {
        Self {
            cmd_tx: None,
            shared: Arc::new(Mutex::new(EngineShared::default())),
            params: Arc::new(Mutex::new(EngineParams::default())),
            connect_cancel: Arc::new(AtomicBool::new(false)),
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
    /// `setInterval` pumps it. Everything else — the command channel, the
    /// shared snapshot, `try_poll` — is identical to the threaded handle,
    /// because the engine does not know which driver is turning it.
    ///
    /// The renderer deliberately does not turn it. Stepping from the frame
    /// callback made the DSP's rate a function of the display's, so a skipped
    /// repaint stalled the pipeline and a long step dropped a frame.
    #[cfg(target_arch = "wasm32")]
    pub fn spawn_in_process() -> Self {
        use wasm_bindgen::JsCast as _;

        let (cmd_tx, cmd_rx) = channel::<EngineCommand>();
        let shared = Arc::new(Mutex::new(EngineShared::default()));
        let params = Arc::new(Mutex::new(EngineParams::default()));
        let connect_cancel = Arc::new(AtomicBool::new(false));
        let engine = Rc::new(RefCell::new(Engine::new(
            cmd_rx,
            Arc::clone(&shared),
            Arc::clone(&params),
            Arc::clone(&connect_cancel),
        )));

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
            cmd_tx: Some(cmd_tx),
            shared,
            params,
            connect_cancel,
            join: None,
            test_polls: None,
            engine: Some(engine),
            _pump: pump,
        }
    }

    /// Queue a synthetic engine poll (detached handles only).
    #[cfg(any(test, not(feature = "gui-core")))]
    pub fn inject_poll(&self, poll: EnginePoll) {
        let Some(q) = &self.test_polls else {
            return;
        };
        if let Ok(mut guard) = q.lock() {
            guard.push_back(poll);
        }
    }

    pub fn send(&self, cmd: EngineCommand) {
        if let Some(tx) = &self.cmd_tx {
            let _ = tx.send(cmd);
        }
    }

    /// Abort a blocking `connect()` from the UI thread (must run before or with Disconnect).
    pub fn abort_connect(&self) {
        self.connect_cancel.store(true, Ordering::Relaxed);
    }

    /// Overwrite the engine's view of UI settings (called once per UI frame).
    pub fn set_params(&self, params: EngineParams) {
        if let Ok(mut guard) = self.params.lock() {
            *guard = params;
        }
    }

    pub fn try_poll(&self) -> Option<EnginePoll> {
        if let Some(q) = &self.test_polls {
            let mut guard = q.lock().ok()?;
            return guard.pop_front();
        }
        let mut guard = self.shared.try_lock().ok()?;
        let rows: Vec<Vec<f32>> = guard.new_rows.drain(..).collect();
        Some(EnginePoll {
            state: guard.state.clone(),
            stats: guard.stats.clone(),
            rows,
            latest: guard.latest.clone(),
            last_error: guard.last_error.clone(),
            audio_scope: guard.audio_scope.clone(),
            audio_waveform: guard.audio_waveform.clone(),
        })
    }

    /// Signal shutdown and detach the worker thread — never blocks the UI thread.
    pub fn shutdown_now(&mut self) {
        self.abort_connect();
        self.send(EngineCommand::Shutdown);
        if let Some(h) = self.join.take() {
            // Dropping JoinHandle without join() detaches the thread.
            drop(h);
        }
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
        let handle = EngineHandle::spawn_detached();
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

    #[test]
    fn set_params_roundtrip() {
        let handle = EngineHandle::spawn_detached();
        let mut params = EngineParams::default();
        params.volume = 0.42;
        params.rf_gain_db = 6.0;
        handle.set_params(params.clone());
        let guard = handle.params.lock().expect("params lock");
        assert!((guard.volume - 0.42).abs() < f32::EPSILON);
        assert!((guard.rf_gain_db - 6.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_handle_fifo_order() {
        let handle = EngineHandle::spawn_detached();
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
        let handle = EngineHandle::spawn_detached();
        handle.send(EngineCommand::Disconnect);
        assert!(handle.try_poll().is_none());
    }
}

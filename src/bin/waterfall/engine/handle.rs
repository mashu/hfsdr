//! UI-side handle to the engine thread.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::Mutex;
use std::thread;

use super::inner::Engine;
use super::types::{EngineCommand, EngineParams, EnginePoll, EngineShared};

/// UI-side handle to the engine thread.
pub struct EngineHandle {
    cmd_tx: Option<Sender<EngineCommand>>,
    shared: Arc<Mutex<EngineShared>>,
    params: Arc<Mutex<EngineParams>>,
    connect_cancel: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
    /// Headless UI tests inject polls here instead of running the engine thread.
    test_polls: Option<Arc<Mutex<VecDeque<EnginePoll>>>>,
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
        }
    }

    /// Headless UI harness: no engine thread; push [`EnginePoll`] snapshots via [`Self::inject_poll`].
    #[cfg(test)]
    pub fn spawn_for_test() -> Self {
        Self {
            cmd_tx: None,
            shared: Arc::new(Mutex::new(EngineShared::default())),
            params: Arc::new(Mutex::new(EngineParams::default())),
            connect_cancel: Arc::new(AtomicBool::new(false)),
            join: None,
            test_polls: Some(Arc::new(Mutex::new(VecDeque::new()))),
        }
    }

    /// Queue a synthetic engine poll (test handles only).
    #[cfg(test)]
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
            spots: guard.spots.clone(),
            decode_channels: guard.skimmer_decode_channels.clone(),
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
            spots: Vec::new(),
            decode_channels: Vec::new(),
            rows: Vec::new(),
            latest: vec![-90.0; FFT_SIZE],
            last_error: None,
            audio_scope: Vec::new(),
            audio_waveform: Vec::new(),
        }
    }

    #[test]
    fn test_handle_inject_and_drain() {
        let handle = EngineHandle::spawn_for_test();
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
        let handle = EngineHandle::spawn_for_test();
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
        let handle = EngineHandle::spawn_for_test();
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
        let handle = EngineHandle::spawn_for_test();
        handle.send(EngineCommand::Disconnect);
        assert!(handle.try_poll().is_none());
    }
}

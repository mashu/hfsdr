//! UI-side handle to the engine thread.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::Mutex;
use std::thread;

use super::inner::Engine;
use super::types::{ConnState, EngineCommand, EngineParams, EnginePoll, EngineShared};

/// UI-side handle to the engine thread.
pub struct EngineHandle {
    cmd_tx: Sender<EngineCommand>,
    shared: Arc<Mutex<EngineShared>>,
    params: Arc<Mutex<EngineParams>>,
    connect_cancel: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
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
            cmd_tx,
            shared,
            params,
            connect_cancel,
            join: Some(join),
        }
    }

    pub fn send(&self, cmd: EngineCommand) {
        let _ = self.cmd_tx.send(cmd);
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
        let mut guard = self.shared.try_lock().ok()?;
        let rows: Vec<Vec<f32>> = guard.new_rows.drain(..).collect();
        Some(EnginePoll {
            state: guard.state.clone(),
            stats: guard.stats.clone(),
            spots: guard.spots.clone(),
            rows,
            latest: guard.latest.clone(),
            last_error: guard.last_error.clone(),
            audio_scope: guard.audio_scope.clone(),
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

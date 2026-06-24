//! Background ingress decimation — runs anti-alias FIR on a dedicated core.

use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use crate::source::Complex32;

use super::fir_decim::FirDecimator;

struct WorkerCmd {
    raw: Arc<Vec<Complex32>>,
    device_rate: f32,
    factor: usize,
}

struct WorkerDone {
    decimated: Vec<Complex32>,
}

/// Single-threaded ingress worker (one job in flight).
pub struct IngressWorker {
    cmd_tx: SyncSender<WorkerCmd>,
    done_rx: Receiver<WorkerDone>,
    join: Option<JoinHandle<()>>,
}

impl IngressWorker {
    pub fn spawn() -> Self {
        let (cmd_tx, cmd_rx) = mpsc::sync_channel(1);
        let (done_tx, done_rx) = mpsc::sync_channel(1);
        let join = thread::Builder::new()
            .name("hfsdr-ingress".into())
            .spawn(move || worker_loop(cmd_rx, done_tx))
            .expect("spawn ingress worker");
        Self {
            cmd_tx,
            done_rx,
            join: Some(join),
        }
    }

    /// Start decimation on `raw` (shared with the caller for parallel demod).
    pub fn start(&self, raw: Arc<Vec<Complex32>>, device_rate: f32, factor: usize) -> bool {
        self.cmd_tx
            .try_send(WorkerCmd {
                raw,
                device_rate,
                factor,
            })
            .is_ok()
    }

    /// Block until the in-flight job finishes.
    pub fn finish(&self) -> Option<Vec<Complex32>> {
        self.done_rx.recv().ok().map(|d| d.decimated)
    }

    /// Non-blocking take when already complete.
    pub fn try_take(&self) -> Option<Vec<Complex32>> {
        match self.done_rx.try_recv() {
            Ok(done) => Some(done.decimated),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => None,
        }
    }
}

impl Drop for IngressWorker {
    fn drop(&mut self) {
        if let Some(h) = self.join.take() {
            let _ = h.join();
        }
    }
}

fn worker_loop(cmd_rx: Receiver<WorkerCmd>, done_tx: SyncSender<WorkerDone>) {
    let mut decim = FirDecimator::with_factor(384_000.0, 1, true);
    let mut last_rate = 0.0f32;
    let mut last_factor = 0usize;

    while let Ok(cmd) = cmd_rx.recv() {
        if cmd.factor != last_factor || (cmd.device_rate - last_rate).abs() > 1.0 {
            decim = FirDecimator::with_factor(cmd.device_rate, cmd.factor, true);
            last_rate = cmd.device_rate;
            last_factor = cmd.factor;
        }
        let mut decimated = Vec::new();
        decim.decimate_block(cmd.raw.as_slice(), &mut decimated);
        let _ = done_tx.send(WorkerDone { decimated });
    }
}

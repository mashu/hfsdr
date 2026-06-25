//! Background ingress decimation — runs anti-alias FIR on a dedicated core.

use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use crate::source::Complex32;

use super::super::cw::DecimFilterKind;
use super::fir_decim::FirDecimator;

struct WorkerCmd {
    raw: Arc<Vec<Complex32>>,
    device_rate: f32,
    factor: usize,
    filter_kind: DecimFilterKind,
}

struct WorkerDone {
    decimated: Vec<Complex32>,
}

/// Single-threaded ingress worker (one job in flight).
pub struct IngressWorker {
    cmd_tx: Option<SyncSender<WorkerCmd>>,
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
            cmd_tx: Some(cmd_tx),
            done_rx,
            join: Some(join),
        }
    }

    /// Start decimation on `raw` (shared with the caller for parallel demod).
    pub fn start(
        &self,
        raw: Arc<Vec<Complex32>>,
        device_rate: f32,
        factor: usize,
        filter_kind: DecimFilterKind,
    ) -> bool {
        self.cmd_tx
            .as_ref()
            .and_then(|tx| {
                tx.try_send(WorkerCmd {
                    raw,
                    device_rate,
                    factor,
                    filter_kind,
                })
                .ok()
            })
            .is_some()
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
        // Close the command channel so worker_loop exits its blocking recv.
        self.cmd_tx = None;
        if let Some(h) = self.join.take() {
            let _ = h.join();
        }
    }
}

fn worker_loop(cmd_rx: Receiver<WorkerCmd>, done_tx: SyncSender<WorkerDone>) {
    let mut decim = FirDecimator::with_factor(384_000.0, 1, true, DecimFilterKind::LinearFir);
    let mut last_rate = 0.0f32;
    let mut last_factor = 0usize;
    let mut last_filter = DecimFilterKind::LinearFir;

    while let Ok(cmd) = cmd_rx.recv() {
        if cmd.factor != last_factor
            || (cmd.device_rate - last_rate).abs() > 1.0
            || cmd.filter_kind != last_filter
        {
            decim = FirDecimator::with_factor(
                cmd.device_rate,
                cmd.factor,
                true,
                cmd.filter_kind,
            );
            last_rate = cmd.device_rate;
            last_factor = cmd.factor;
            last_filter = cmd.filter_kind;
        }
        let mut decimated = Vec::new();
        decim.decimate_block(cmd.raw.as_slice(), &mut decimated, false);
        let _ = done_tx.send(WorkerDone { decimated });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::source::Complex32;

    #[test]
    fn decimates_in_background_thread() {
        let worker = IngressWorker::spawn();
        let raw: Vec<Complex32> = (0..64)
            .map(|i| Complex32::new((i as f32 * 0.1).cos(), 0.0))
            .collect();
        assert!(worker.start(
            Arc::new(raw),
            48_000.0,
            4,
            DecimFilterKind::LinearFir,
        ));
        let out = worker.finish().expect("decimated output");
        assert!(!out.is_empty());
        assert!(out.len() < 64);
    }

    #[test]
    fn start_rejects_second_job_while_busy() {
        let worker = IngressWorker::spawn();
        let raw = Arc::new(vec![Complex32::default(); 32]);
        assert!(worker.start(
            Arc::clone(&raw),
            48_000.0,
            2,
            DecimFilterKind::LinearFir,
        ));
        assert!(!worker.start(raw, 48_000.0, 2, DecimFilterKind::LinearFir));
        worker.finish();
    }

    #[test]
    fn try_take_before_finish_is_empty() {
        let worker = IngressWorker::spawn();
        let raw = Arc::new(vec![Complex32::new(1.0, 0.0); 32]);
        assert!(worker.start(raw, 48_000.0, 2, DecimFilterKind::LinearFir));
        assert!(worker.try_take().is_none());
        assert!(worker.finish().is_some());
    }

    #[test]
    fn resyncs_filter_on_rate_change() {
        let worker = IngressWorker::spawn();
        let raw = Arc::new(vec![Complex32::new(1.0, 0.0); 32]);
        assert!(worker.start(
            Arc::clone(&raw),
            48_000.0,
            2,
            DecimFilterKind::LinearFir,
        ));
        worker.finish();
        assert!(worker.start(raw, 96_000.0, 4, DecimFilterKind::Iir2Pole));
        assert!(worker.finish().is_some());
    }
}

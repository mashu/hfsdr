//! Background ingress decimation — runs anti-alias FIR on a dedicated core.
//!
//! Native-only: this spawns an OS thread, which wasm32 targets without threads
//! cannot do. The pump already has a non-threaded fallback that decimates
//! inline, so a wasm frontend simply never constructs this.

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
    /// Recycled output buffer from a previous job (avoids a per-job Vec).
    reuse: Vec<Complex32>,
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
    ///
    /// `reuse` is an output buffer from a previous [`Self::finish`] (or empty)
    /// that the worker recycles instead of allocating per job.
    pub fn start(
        &self,
        raw: Arc<Vec<Complex32>>,
        device_rate: f32,
        factor: usize,
        filter_kind: DecimFilterKind,
        reuse: Vec<Complex32>,
    ) -> bool {
        self.cmd_tx
            .as_ref()
            .and_then(|tx| {
                tx.try_send(WorkerCmd {
                    raw,
                    device_rate,
                    factor,
                    filter_kind,
                    reuse,
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
        let WorkerCmd { raw, reuse, .. } = cmd;
        let mut decimated = reuse;
        decimated.clear();
        decim.decimate_block(raw.as_slice(), &mut decimated, false);
        // Release the shared batch before signaling completion so the caller
        // can reclaim its Arc as soon as finish() returns.
        drop(raw);
        let _ = done_tx.send(WorkerDone { decimated });
    }
}

// Every test here constructs an IngressWorker, which spawns an OS thread —
// unavailable on single-threaded wasm targets.
#[cfg(all(test, not(target_family = "wasm")))]
mod tests {
    use std::time::{Duration, Instant};

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
        Vec::new()));
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
        Vec::new()));
        assert!(!worker.start(raw, 48_000.0, 2, DecimFilterKind::LinearFir, Vec::new()));
        worker.finish();
    }

    /// `try_take` must not block, and the job's output must not be lost whether
    /// or not the worker happened to finish first.
    ///
    /// The previous version asserted `try_take().is_none()` immediately after
    /// `start`, which is a statement about thread scheduling rather than about
    /// the contract: on a loaded or emulated CPU the worker can finish first,
    /// and the test then failed for a reason that was never a defect.
    #[test]
    fn try_take_is_non_blocking_and_never_loses_the_job() {
        let worker = IngressWorker::spawn();
        let raw = Arc::new(vec![Complex32::new(1.0, 0.0); 32]);
        assert!(worker.start(raw, 48_000.0, 2, DecimFilterKind::LinearFir, Vec::new()));

        let t0 = Instant::now();
        let early = worker.try_take();
        assert!(
            t0.elapsed() < Duration::from_millis(200),
            "try_take must not block on the worker"
        );

        // Either it was ready and we have it, or finish() must still deliver it.
        let decimated = match early {
            Some(d) => d,
            None => worker.finish().expect("worker must deliver the started job"),
        };
        assert!(!decimated.is_empty(), "decimation produced no output");
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
        Vec::new()));
        worker.finish();
        assert!(worker.start(raw, 96_000.0, 4, DecimFilterKind::Iir2Pole, Vec::new()));
        assert!(worker.finish().is_some());
    }
}

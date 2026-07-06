//! Dual IQ rings: raw (demod + record) and decimated (spectrum + skimmer).
//!
//! A bridge thread drains the device ring and fans out to both rings so the
//! engine pump no longer runs ingress FIR decimation on the hot path.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use hfsdr::{Complex32, DecimFilterKind, FirDecimator};
use rtrb::{Consumer, RingBuffer};

fn filter_from_atomic(v: u8) -> DecimFilterKind {
    if v == 1 {
        DecimFilterKind::Iir2Pole
    } else {
        DecimFilterKind::LinearFir
    }
}

fn filter_to_atomic(kind: DecimFilterKind) -> u8 {
    match kind {
        DecimFilterKind::Iir2Pole => 1,
        DecimFilterKind::LinearFir => 0,
    }
}

const BRIDGE_CHUNK: usize = 4096;

/// Background tap: device IQ → raw ring + decimated ring.
pub struct IqDualRingBridge {
    stop: Arc<AtomicBool>,
    filter_ctl: Arc<AtomicU8>,
    join: Option<JoinHandle<()>>,
    raw_dropped: Arc<AtomicU64>,
    decim_dropped: Arc<AtomicU64>,
}

impl IqDualRingBridge {
    /// Spawn the bridge. Returns `(bridge, raw_consumer, decim_consumer)`.
    pub fn spawn(
        mut device: Consumer<Complex32>,
        device_rate: f32,
        factor: usize,
        filter_kind: DecimFilterKind,
        raw_cap: usize,
        decim_cap: usize,
    ) -> (Self, Consumer<Complex32>, Consumer<Complex32>) {
        let (mut raw_prod, raw_cons) = RingBuffer::new(raw_cap);
        let (mut decim_prod, decim_cons) = RingBuffer::new(decim_cap);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_t = Arc::clone(&stop);
        let filter_ctl = Arc::new(AtomicU8::new(filter_to_atomic(filter_kind)));
        let filter_t = Arc::clone(&filter_ctl);
        let raw_dropped = Arc::new(AtomicU64::new(0));
        let decim_dropped = Arc::new(AtomicU64::new(0));
        let raw_dropped_t = Arc::clone(&raw_dropped);
        let decim_dropped_t = Arc::clone(&decim_dropped);

        let join = thread::Builder::new()
            .name("hfsdr-iq-bridge".into())
            .spawn(move || {
                let mut decim =
                    FirDecimator::with_factor(device_rate, factor, true, filter_kind);
                let mut active_filter = filter_kind;
                let mut chunk = Vec::with_capacity(BRIDGE_CHUNK);
                let mut decim_out = Vec::new();
                loop {
                    if stop_t.load(Ordering::Relaxed) {
                        break;
                    }
                    let wanted = filter_from_atomic(filter_t.load(Ordering::Relaxed));
                    if wanted != active_filter {
                        decim.sync_filter(device_rate, wanted);
                        active_filter = wanted;
                    }

                    chunk.clear();
                    while chunk.len() < BRIDGE_CHUNK {
                        match device.pop() {
                            Ok(sample) => chunk.push(sample),
                            Err(rtrb::PopError::Empty) => break,
                        }
                    }
                    if chunk.is_empty() {
                        thread::yield_now();
                        continue;
                    }

                    for sample in &chunk {
                        if raw_prod.push(*sample).is_err() {
                            raw_dropped_t.fetch_add(1, Ordering::Relaxed);
                        }
                    }

                    decim.decimate_block(&chunk, &mut decim_out, false);
                    for sample in &decim_out {
                        if decim_prod.push(*sample).is_err() {
                            decim_dropped_t.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            })
            .expect("spawn iq bridge");

        (
            Self {
                stop,
                filter_ctl,
                join: Some(join),
                raw_dropped,
                decim_dropped,
            },
            raw_cons,
            decim_cons,
        )
    }

    pub fn set_decim_filter(&self, kind: DecimFilterKind) {
        self.filter_ctl
            .store(filter_to_atomic(kind), Ordering::Relaxed);
    }

    pub fn raw_dropped(&self) -> u64 {
        self.raw_dropped.load(Ordering::Relaxed)
    }

    pub fn decim_dropped(&self) -> u64 {
        self.decim_dropped.load(Ordering::Relaxed)
    }
}

impl Drop for IqDualRingBridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/// Wire dual rings when `ingress_decim > 1`; otherwise pass the device consumer through.
pub fn attach_dual_ring(
    device_iq: Consumer<Complex32>,
    ingress_decim: usize,
    device_rate: f32,
    ring_cap: usize,
    filter_kind: DecimFilterKind,
) -> (
    Consumer<Complex32>,
    Option<Consumer<Complex32>>,
    Option<IqDualRingBridge>,
    usize,
) {
    if ingress_decim <= 1 {
        return (device_iq, None, None, 0);
    }
    let decim_cap = (ring_cap / ingress_decim).max(4096);
    let (bridge, raw, decim) = IqDualRingBridge::spawn(
        device_iq,
        device_rate,
        ingress_decim,
        filter_kind,
        ring_cap,
        decim_cap,
    );
    (raw, Some(decim), Some(bridge), decim_cap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtrb::RingBuffer;
    use std::time::Duration;

    #[test]
    fn bridge_decimates_into_second_ring() {
        let (mut dev_prod, dev_cons) = RingBuffer::<Complex32>::new(256);
        for i in 0..32i32 {
            let t = i as f32 / 48_000.0;
            let _ = dev_prod.push(Complex32::new((t * 1000.0).cos(), 0.0));
        }
        drop(dev_prod);

        let (_bridge, mut raw, mut decim) =
            IqDualRingBridge::spawn(dev_cons, 48_000.0, 4, DecimFilterKind::LinearFir, 256, 64);

        std::thread::sleep(Duration::from_millis(50));

        let mut raw_n = 0usize;
        while raw.pop().is_ok() {
            raw_n += 1;
        }
        let mut decim_n = 0usize;
        while decim.pop().is_ok() {
            decim_n += 1;
        }
        assert_eq!(raw_n, 32);
        assert!(decim_n > 0 && decim_n < raw_n);
    }
}

//! Background skimmer thread wiring.
//!
//! The GUI already owns the single ring consumer, so rather than refactor the
//! ring into multi-consumer we forward a copy of each drained IQ block plus the
//! latest spectrum row to a worker thread. The worker runs the [`Skimmer`]
//! decoder bank off the UI thread and publishes a sorted spot snapshot back.

use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;

use hfsdr::{Complex32, Skimmer, SkimmerConfig, Spot, SpotSort};

struct SkimmerInput {
    iq: Vec<Complex32>,
    spectrum: Vec<f32>,
    iq_rate: f32,
    center_hz: f64,
}

/// Handle to the background skimmer worker.
pub struct SkimmerHandle {
    tx: SyncSender<SkimmerInput>,
    spots: Arc<Mutex<Vec<Spot>>>,
    channels: Arc<Mutex<usize>>,
    enabled: bool,
}

impl SkimmerHandle {
    pub fn spawn(label: String) -> Self {
        let (tx, rx) = sync_channel::<SkimmerInput>(4);
        let spots = Arc::new(Mutex::new(Vec::new()));
        let channels = Arc::new(Mutex::new(0usize));
        let spots_thread = Arc::clone(&spots);
        let channels_thread = Arc::clone(&channels);

        thread::Builder::new()
            .name("skimmer".into())
            .spawn(move || {
                let mut skimmer = Skimmer::new(SkimmerConfig {
                    source_label: label,
                    ..SkimmerConfig::default()
                });
                while let Ok(input) = rx.recv() {
                    skimmer.process(&input.iq, input.iq_rate, &input.spectrum, input.center_hz);
                    if let Ok(mut guard) = spots_thread.lock() {
                        *guard = skimmer.store().sorted(SpotSort::SnrDesc);
                    }
                    if let Ok(mut guard) = channels_thread.lock() {
                        *guard = skimmer.active_channels();
                    }
                }
            })
            .expect("spawn skimmer thread");

        Self {
            tx,
            spots,
            channels,
            enabled: false,
        }
    }

    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
    }

    /// Forward a block to the worker; drops silently if the worker is busy.
    pub fn submit(&self, iq: &[Complex32], spectrum: &[f32], iq_rate: f32, center_hz: f64) {
        if !self.enabled || iq.is_empty() {
            return;
        }
        let _ = self.tx.try_send(SkimmerInput {
            iq: iq.to_vec(),
            spectrum: spectrum.to_vec(),
            iq_rate,
            center_hz,
        });
    }

    pub fn spots(&self) -> Vec<Spot> {
        self.spots.lock().map(|g| g.clone()).unwrap_or_default()
    }

    pub fn active_channels(&self) -> usize {
        self.channels.lock().map(|g| *g).unwrap_or(0)
    }
}

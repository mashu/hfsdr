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
    spectrum_rate: f32,
    spectrum_pan_hz: f32,
    center_hz: f64,
}

enum SkimmerMsg {
    Frame(SkimmerInput),
    Clear,
    ReloadScp,
    ReloadScpPath(std::path::PathBuf),
}

/// MASTER.SCP load status for the UI.
#[derive(Clone, Debug, Default)]
pub struct ScpStatus {
    pub loaded: bool,
    pub calls: usize,
    pub version: Option<String>,
    pub path: Option<String>,
}

/// Handle to the background skimmer worker.
pub struct SkimmerHandle {
    tx: SyncSender<SkimmerMsg>,
    spots: Arc<Mutex<Vec<Spot>>>,
    channels: Arc<Mutex<usize>>,
    scp_status: Arc<Mutex<ScpStatus>>,
    config: Arc<Mutex<SkimmerConfig>>,
    enabled: bool,
}

fn publish_scp(skimmer: &Skimmer, status: &Arc<Mutex<ScpStatus>>) {
    let scp = skimmer.scp();
    if let Ok(mut guard) = status.lock() {
        *guard = ScpStatus {
            loaded: scp.is_loaded(),
            calls: scp.len(),
            version: scp.version().map(str::to_string),
            path: scp.path().map(|p| p.to_string_lossy().into_owned()),
        };
    }
}

impl SkimmerHandle {
    pub fn spawn(label: String) -> Self {
        let (tx, rx) = sync_channel::<SkimmerMsg>(8);
        let spots = Arc::new(Mutex::new(Vec::new()));
        let channels = Arc::new(Mutex::new(0usize));
        let scp_status = Arc::new(Mutex::new(ScpStatus::default()));
        let config = Arc::new(Mutex::new(SkimmerConfig {
            source_label: label,
            ..SkimmerConfig::default()
        }));
        let spots_thread = Arc::clone(&spots);
        let channels_thread = Arc::clone(&channels);
        let scp_thread = Arc::clone(&scp_status);
        let config_thread = Arc::clone(&config);

        thread::Builder::new()
            .name("skimmer".into())
            .spawn(move || {
                let mut skimmer =
                    Skimmer::new(config_thread.lock().map(|g| g.clone()).unwrap_or_default());
                publish_scp(&skimmer, &scp_thread);
                while let Ok(msg) = rx.recv() {
                    match msg {
                        SkimmerMsg::Clear => {
                            skimmer.clear();
                            if let Ok(mut guard) = spots_thread.lock() {
                                guard.clear();
                            }
                            if let Ok(mut guard) = channels_thread.lock() {
                                *guard = 0;
                            }
                        }
                        SkimmerMsg::ReloadScp => {
                            skimmer.reload_scp_discover();
                            publish_scp(&skimmer, &scp_thread);
                        }
                        SkimmerMsg::ReloadScpPath(path) => {
                            skimmer.reload_scp_from(&path);
                            publish_scp(&skimmer, &scp_thread);
                        }
                        SkimmerMsg::Frame(input) => {
                            if let Ok(cfg) = config_thread.lock() {
                                skimmer.set_config(cfg.clone());
                            }
                            skimmer.process(
                                &input.iq,
                                input.iq_rate,
                                &input.spectrum,
                                input.spectrum_rate,
                                input.spectrum_pan_hz,
                                input.center_hz,
                            );
                            if let Ok(mut guard) = spots_thread.lock() {
                                *guard = skimmer.store().sorted(SpotSort::SnrDesc);
                            }
                            if let Ok(mut guard) = channels_thread.lock() {
                                *guard = skimmer.active_channels();
                            }
                            publish_scp(&skimmer, &scp_thread);
                        }
                    }
                }
            })
            .expect("spawn skimmer thread");

        Self {
            tx,
            spots,
            channels,
            scp_status,
            config,
            enabled: false,
        }
    }

    pub fn set_config(&self, config: SkimmerConfig) {
        if let Ok(mut guard) = self.config.lock() {
            *guard = config;
        }
    }

    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
    }

    /// Drop all decoded spots and reset decoder channels.
    pub fn clear(&self) {
        let _ = self.tx.try_send(SkimmerMsg::Clear);
        if let Ok(mut guard) = self.spots.lock() {
            guard.clear();
        }
        if let Ok(mut guard) = self.channels.lock() {
            *guard = 0;
        }
    }

    /// Re-scan known MASTER.SCP locations and reload (blocks until the worker runs it).
    pub fn reload_scp(&self) {
        let _ = self.tx.send(SkimmerMsg::ReloadScp);
    }

    /// Reload from an explicit path (e.g. after download).
    pub fn reload_scp_from(&self, path: std::path::PathBuf) {
        let _ = self.tx.send(SkimmerMsg::ReloadScpPath(path));
    }

    /// Forward a block to the worker; drops silently if the worker is busy.
    pub fn submit(
        &self,
        iq: &[Complex32],
        spectrum: &[f32],
        iq_rate: f32,
        spectrum_rate: f32,
        spectrum_pan_hz: f32,
        center_hz: f64,
    ) {
        if !self.enabled || iq.is_empty() {
            return;
        }
        let _ = self.tx.try_send(SkimmerMsg::Frame(SkimmerInput {
            iq: iq.to_vec(),
            spectrum: spectrum.to_vec(),
            iq_rate,
            spectrum_rate,
            spectrum_pan_hz,
            center_hz,
        }));
    }

    pub fn spots(&self) -> Vec<Spot> {
        self.spots.lock().map(|g| g.clone()).unwrap_or_default()
    }

    pub fn active_channels(&self) -> usize {
        self.channels.lock().map(|g| *g).unwrap_or(0)
    }

    pub fn scp_status(&self) -> ScpStatus {
        self.scp_status
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }
}

//! KiwiSDR front end: connects over WebSocket, requests IQ mode, and delivers
//! ~12 kHz baseband IQ through the same [`IqSource`] interface as the Airspy.
//! Wire format reverse-checked against the reference client (jks-prv/kiwiclient).

pub mod protocol;
mod reader;

use crate::source::{Complex32, Consumer, IqSource, Result, SourceError};
use protocol::{kiwi_iq_half_hz, KIWI_IQ_RATE, KiwiRxSetup, mod_iq_command};
use reader::{READ_TIMEOUT, reader_loop};
use rtrb::RingBuffer;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tungstenite::client::IntoClientRequest;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(12);

/// A KiwiSDR IQ front end.
pub struct KiwiSource {
    host: String,
    port: u16,
    freq_hz: f64,
    low_cut: i32,
    high_cut: i32,
    freq_offset_khz: f64,
    ar_out_hz: u32,
    agc_on: bool,
    man_gain: u8,
    compression: bool,
    streaming: bool,
    stop: Arc<AtomicBool>,
    dropped: Arc<AtomicU64>,
    rssi_cdbm: Arc<AtomicI32>,
    iq_streaming: Arc<AtomicBool>,
    link_error: Arc<Mutex<Option<String>>>,
    cmd_tx: Option<Sender<String>>,
    handle: Option<JoinHandle<()>>,
}

impl KiwiSource {
    /// IQ stream is configured and SND frames are arriving.
    pub fn iq_ready(&self) -> bool {
        self.iq_streaming.load(Ordering::Relaxed)
    }

    /// Reader thread is still running.
    pub fn link_alive(&self) -> bool {
        self.handle.as_ref().is_some_and(|h| !h.is_finished())
    }

    pub fn link_error(&self) -> Option<String> {
        self.link_error.lock().ok().and_then(|e| e.clone())
    }

    /// Create a source for `ws://host:port` (the standard Kiwi port is 8073).
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        let half = kiwi_iq_half_hz(KIWI_IQ_RATE);
        Self {
            host: host.into(),
            port,
            freq_hz: 0.0,
            low_cut: -half,
            high_cut: half,
            freq_offset_khz: 0.0,
            ar_out_hz: 44_100,
            agc_on: true,
            man_gain: 50,
            compression: false,
            streaming: false,
            stop: Arc::new(AtomicBool::new(false)),
            dropped: Arc::new(AtomicU64::new(0)),
            rssi_cdbm: Arc::new(AtomicI32::new(0)),
            iq_streaming: Arc::new(AtomicBool::new(false)),
            link_error: Arc::new(Mutex::new(None)),
            cmd_tx: None,
            handle: None,
        }
    }

    /// Set the IQ passband (Hz) sent to the Kiwi; default is +/-5 kHz.
    pub fn with_passband(mut self, low_cut: i32, high_cut: i32) -> Self {
        self.low_cut = low_cut;
        self.high_cut = high_cut;
        self
    }

    /// Enable or disable Kiwi AGC (default on).
    pub fn with_agc(mut self, on: bool) -> Self {
        self.agc_on = on;
        self
    }

    /// Manual RF gain 0..=100 (used when AGC is off; still sent with AGC on).
    pub fn with_man_gain(mut self, gain: u8) -> Self {
        self.man_gain = gain.clamp(0, 100);
        self
    }

    /// Transverter / LNB offset in kHz subtracted from the tune frequency (kiwiclient `-o`).
    pub fn with_freq_offset_khz(mut self, khz: f64) -> Self {
        self.freq_offset_khz = khz;
        self
    }

    /// `SET AR OK out=` rate (default 44100).
    pub fn with_ar_out_hz(mut self, hz: u32) -> Self {
        self.ar_out_hz = hz.clamp(8_000, 192_000);
        self
    }

    /// Kiwi center frequency in kHz after transverter offset.
    fn kiwi_freq_khz(&self) -> f64 {
        self.freq_hz / 1000.0 - self.freq_offset_khz
    }

    /// Latest S-meter reading in dBm.
    pub fn meter_dbm(&self) -> f32 {
        self.rssi_cdbm.load(Ordering::Relaxed) as f32 / 100.0
    }

    fn mod_cmd(&self) -> String {
        mod_iq_command(
            self.low_cut,
            self.high_cut,
            self.kiwi_freq_khz() * 1000.0,
        )
    }

    fn rx_setup(&self) -> KiwiRxSetup {
        KiwiRxSetup {
            low_cut: self.low_cut,
            high_cut: self.high_cut,
            freq_hz: self.kiwi_freq_khz() * 1000.0,
            agc_on: self.agc_on,
            man_gain: self.man_gain,
            compression: self.compression,
            ar_out_hz: self.ar_out_hz,
        }
    }

    fn connect_ws(&self, cancel: &AtomicBool) -> Result<WebSocket<MaybeTlsStream<TcpStream>>> {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let url = format!("ws://{}:{}/{}/SND", self.host, self.port, ts);
        let addr = (self.host.as_str(), self.port)
            .to_socket_addrs()
            .map_err(|_| SourceError::Backend {
                op: "kiwi resolve",
                code: -3,
            })?
            .next()
            .ok_or(SourceError::Backend {
                op: "kiwi resolve",
                code: -3,
            })?;
        let request = url.into_client_request().map_err(|_| SourceError::Backend {
            op: "kiwi ws request",
            code: -4,
        })?;
        let deadline = Instant::now() + CONNECT_TIMEOUT;
        while Instant::now() < deadline {
            if cancel.load(Ordering::Relaxed) {
                return Err(SourceError::Backend {
                    op: "kiwi connect cancelled",
                    code: -6,
                });
            }
            let Ok(tcp) = TcpStream::connect_timeout(&addr, Duration::from_millis(400)) else {
                continue;
            };
            let Ok((ws, _resp)) =
                tungstenite::client::client(request.clone(), MaybeTlsStream::Plain(tcp))
            else {
                continue;
            };
            return Ok(ws);
        }
        Err(SourceError::Backend {
            op: "kiwi connect",
            code: -1,
        })
    }

    pub fn start_cancellable(&mut self, cancel: &AtomicBool) -> Result<Consumer<Complex32>> {
        if self.streaming {
            return Err(SourceError::InvalidState("already streaming"));
        }

        let mut ws = self.connect_ws(cancel)?;

        if let MaybeTlsStream::Plain(tcp) = ws.get_ref() {
            let _ = tcp.set_read_timeout(Some(READ_TIMEOUT));
        }

        for line in [
            "SET auth t=kiwi p=",
            "SET ident_user=hfsdr",
            &self.mod_cmd(),
            &format!("SET AR OK in={} out={}", KIWI_IQ_RATE, self.ar_out_hz),
            &protocol::agc_command(self.agc_on, self.man_gain),
            "SET squelch=0 max=0",
            "SET keepalive",
        ] {
            if cancel.load(Ordering::Relaxed) {
                let _ = ws.close(None);
                return Err(SourceError::Backend {
                    op: "kiwi connect cancelled",
                    code: -6,
                });
            }
            ws.send(Message::Text(line.into()))
                .map_err(|_| SourceError::Backend {
                    op: "kiwi auth",
                    code: -5,
                })?;
        }

        let (prod, cons) = RingBuffer::<Complex32>::new(1 << 16);
        let (cmd_tx, cmd_rx) = mpsc::channel::<String>();
        let stop = Arc::new(AtomicBool::new(false));

        let dropped = Arc::clone(&self.dropped);
        let rssi = Arc::clone(&self.rssi_cdbm);
        let iq_streaming = Arc::clone(&self.iq_streaming);
        let link_error = Arc::clone(&self.link_error);
        let stop_thread = Arc::clone(&stop);
        let rx_setup = self.rx_setup();
        let handle = thread::spawn(move || {
            reader_loop(
                ws,
                prod,
                cmd_rx,
                stop_thread,
                dropped,
                rssi,
                iq_streaming,
                link_error,
                rx_setup,
            );
        });

        self.stop = stop;
        self.cmd_tx = Some(cmd_tx);
        self.handle = Some(handle);
        self.streaming = true;
        Ok(cons)
    }
}

impl IqSource for KiwiSource {
    fn sample_rates(&self) -> Vec<u32> {
        vec![KIWI_IQ_RATE]
    }

    fn sample_rate(&self) -> u32 {
        KIWI_IQ_RATE
    }

    fn set_sample_rate(&mut self, sr: u32) -> Result<()> {
        if sr == KIWI_IQ_RATE {
            Ok(())
        } else {
            Err(SourceError::Unsupported(format!(
                "KiwiSDR IQ rate is fixed at {KIWI_IQ_RATE} S/s"
            )))
        }
    }

    fn tune(&mut self, hz: f64) -> Result<()> {
        self.freq_hz = hz;
        if let Some(tx) = &self.cmd_tx {
            let _ = tx.send(self.mod_cmd());
        }
        Ok(())
    }

    fn frequency(&self) -> f64 {
        self.freq_hz
    }

    fn start(&mut self) -> Result<Consumer<Complex32>> {
        static NEVER: AtomicBool = AtomicBool::new(false);
        self.start_cancellable(&NEVER)
    }

    fn stop(&mut self) -> Result<()> {
        if !self.streaming {
            return Ok(());
        }
        self.stop.store(true, Ordering::Relaxed);
        self.cmd_tx = None;
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        self.streaming = false;
        Ok(())
    }

    fn dropped_samples(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    fn is_streaming(&self) -> bool {
        self.streaming
    }

    fn rssi_dbm(&self) -> Option<f32> {
        Some(self.meter_dbm())
    }

    fn supports_passband(&self) -> bool {
        true
    }

    fn set_passband(&mut self, low_hz: i32, high_hz: i32) -> Result<()> {
        self.low_cut = low_hz;
        self.high_cut = high_hz;
        if let Some(tx) = &self.cmd_tx {
            let _ = tx.send(self.mod_cmd());
        }
        Ok(())
    }

    fn set_agc(&mut self, on: bool) -> Result<()> {
        self.agc_on = on;
        if let Some(tx) = &self.cmd_tx {
            let cmd = protocol::agc_command(on, self.man_gain);
            let _ = tx.send(cmd);
        }
        Ok(())
    }

    fn set_man_gain(&mut self, gain: u8) -> Result<()> {
        self.man_gain = gain.clamp(0, 100);
        if let Some(tx) = &self.cmd_tx {
            let cmd = protocol::agc_command(self.agc_on, self.man_gain);
            let _ = tx.send(cmd);
        }
        Ok(())
    }

    fn link_ready(&self) -> bool {
        self.iq_ready()
    }

    fn link_alive(&self) -> bool {
        KiwiSource::link_alive(self)
    }

    fn link_error(&self) -> Option<String> {
        KiwiSource::link_error(self)
    }
}

impl Drop for KiwiSource {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::IqSource;

    #[test]
    fn builder_sets_passband_and_agc() {
        let src = KiwiSource::new("kiwi.test", 8073)
            .with_passband(-3_000, 3_000)
            .with_agc(false)
            .with_freq_offset_khz(5.0)
            .with_ar_out_hz(48_000);
        assert!(!src.agc_on);
        assert_eq!(src.low_cut, -3_000);
        assert_eq!(src.high_cut, 3_000);
        assert_eq!(src.ar_out_hz, 48_000);
    }

    #[test]
    fn iq_source_trait_before_streaming() {
        let mut src = KiwiSource::new("kiwi.test", 8073);
        assert_eq!(src.sample_rate(), KIWI_IQ_RATE);
        assert!(src.set_sample_rate(KIWI_IQ_RATE).is_ok());
        assert!(src.set_sample_rate(48_000).is_err());
        src.tune(14_030_000.0).unwrap();
        assert_eq!(src.frequency(), 14_030_000.0);
        assert!(!src.is_streaming());
        assert_eq!(src.dropped_samples(), 0);
        assert!(src.supports_passband());
        src.set_passband(-4_000, 4_000).unwrap();
        src.set_agc(true).unwrap();
        src.set_man_gain(60).unwrap();
        assert!(src.rssi_dbm().is_some());
        assert!(!src.link_ready());
        assert!(!src.link_alive());
        assert!(src.link_error().is_none());
        src.stop().unwrap();
    }

    #[test]
    fn meter_dbm_reads_atomic() {
        let src = KiwiSource::new("kiwi.test", 8073);
        src.rssi_cdbm.store(1_234, Ordering::Relaxed);
        assert!((src.meter_dbm() - 12.34).abs() < 1e-3);
    }
}

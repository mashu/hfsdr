//! KiwiSDR front end: connects over WebSocket, requests IQ mode, and delivers
//! ~12 kHz baseband IQ through the same [`IqSource`] interface as the Airspy.
//! Wire format reverse-checked against the reference client (jks-prv/kiwiclient).

pub mod protocol;
#[cfg(not(target_arch = "wasm32"))]
mod reader;
pub mod session;
#[cfg(target_arch = "wasm32")]
pub mod web;

use crate::source::controls::KiwiControls;
use crate::source::{Complex32, Consumer, IqSource, Result, SourceError};
use protocol::{kiwi_iq_half_hz, KIWI_IQ_RATE, KiwiRxSetup, KIWI_MAN_GAIN_DEFAULT, mod_iq_command};
#[cfg(not(target_arch = "wasm32"))]
use reader::{READ_TIMEOUT, reader_loop};
use session::{KiwiLinkState, KiwiSession};
use rtrb::RingBuffer;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use crate::time::{SystemTime, UNIX_EPOCH};

#[cfg(not(target_arch = "wasm32"))]
use std::net::{TcpStream, ToSocketAddrs};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::{self, Sender};
#[cfg(not(target_arch = "wasm32"))]
use std::thread::{self, JoinHandle};
#[cfg(not(target_arch = "wasm32"))]
use crate::time::{Duration, Instant};
#[cfg(not(target_arch = "wasm32"))]
use tungstenite::client::IntoClientRequest;
#[cfg(not(target_arch = "wasm32"))]
use tungstenite::stream::MaybeTlsStream;
#[cfg(not(target_arch = "wasm32"))]
use tungstenite::{Message, WebSocket};

#[cfg(not(target_arch = "wasm32"))]
const CONNECT_TIMEOUT: Duration = Duration::from_secs(12);

/// The live transport, whichever this target has.
///
/// Both variants answer the same three questions the rest of the code asks:
/// is the link alive, take this command, and (on drop) shut down.
#[cfg(not(target_arch = "wasm32"))]
struct Link {
    /// `None` for the mock source, which accepts no commands.
    cmd_tx: Option<Sender<String>>,
    handle: Option<JoinHandle<()>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Link {
    fn alive(&self) -> bool {
        self.handle.as_ref().is_some_and(|h| !h.is_finished())
    }

    fn send_cmd(&self, cmd: String) {
        if let Some(tx) = &self.cmd_tx {
            let _ = tx.send(cmd);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Link {
    fn drop(&mut self) {
        // Dropping the sender is what lets the reader loop notice the stop flag
        // and return, so join only after it is gone.
        self.cmd_tx = None;
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

#[cfg(target_arch = "wasm32")]
struct Link {
    socket: web::WebKiwiLink,
}

#[cfg(target_arch = "wasm32")]
impl Link {
    fn alive(&self) -> bool {
        self.socket.alive()
    }

    fn send_cmd(&self, cmd: String) {
        self.socket.send_command(cmd);
    }
}

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
    gen_attn: u8,
    rf_attn_db: f32,
    compression: bool,
    streaming: bool,
    has_rf_attn: Arc<AtomicBool>,
    rf_attn_cdb: Arc<AtomicI32>,
    stop: Arc<AtomicBool>,
    dropped: Arc<AtomicU64>,
    rssi_cdbm: Arc<AtomicI32>,
    iq_streaming: Arc<AtomicBool>,
    link_error: Arc<Mutex<Option<String>>>,
    link: Option<Link>,
}

impl KiwiSource {
    /// IQ stream is configured and SND frames are arriving.
    pub fn iq_ready(&self) -> bool {
        self.iq_streaming.load(Ordering::Relaxed)
    }

    /// The transport is still running: the reader thread natively, the socket
    /// in the browser.
    pub fn link_alive(&self) -> bool {
        self.link.as_ref().is_some_and(Link::alive)
    }

    /// Forward a control command to the transport, if one is connected.
    fn send_cmd(&self, cmd: String) {
        if let Some(link) = &self.link {
            link.send_cmd(cmd);
        }
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
            man_gain: KIWI_MAN_GAIN_DEFAULT,
            gen_attn: 0,
            rf_attn_db: 0.0,
            compression: false,
            streaming: false,
            has_rf_attn: Arc::new(AtomicBool::new(false)),
            rf_attn_cdb: Arc::new(AtomicI32::new(-1)),
            stop: Arc::new(AtomicBool::new(false)),
            dropped: Arc::new(AtomicU64::new(0)),
            rssi_cdbm: Arc::new(AtomicI32::new(0)),
            iq_streaming: Arc::new(AtomicBool::new(false)),
            link_error: Arc::new(Mutex::new(None)),
            link: None,
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

    /// RF gain 0..=100 (`manGain`); manual IQ gain when Kiwi RF AGC is off (firmware ignores it when AGC on).
    pub fn with_man_gain(mut self, gain: u8) -> Self {
        self.man_gain = gain.clamp(0, 100);
        self
    }

    /// Test generator attenuation for the IQ handshake (`SET genattn=`).
    pub fn with_gen_attn(mut self, attn: u8) -> Self {
        self.gen_attn = attn;
        self
    }

    /// Hardware RF attenuator in dB (KiwiSDR 2, when `has_attn=1`).
    pub fn with_rf_attn_db(mut self, db: f32) -> Self {
        self.rf_attn_db = db.clamp(0.0, protocol::KIWI_RF_ATTN_MAX_DB);
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

    /// Opening handshake, sent as soon as the socket is writable.
    ///
    /// Order follows kiwiclient: authenticate, identify, then request the mode
    /// and rates. The Kiwi answers with `sample_rate=…`, which is what drives
    /// [`session::KiwiSession`] into IQ setup.
    fn auth_lines(&self) -> Vec<String> {
        vec![
            "SET auth t=kiwi p=".to_string(),
            "SET ident_user=hfsdr".to_string(),
            self.mod_cmd(),
            format!("SET AR OK in={} out={}", KIWI_IQ_RATE, self.ar_out_hz),
            protocol::agc_command(self.agc_on, self.man_gain),
            "SET squelch=0 max=0".to_string(),
            "SET keepalive".to_string(),
        ]
    }

    /// Unix seconds, which the Kiwi uses to tell reconnects from duplicates.
    fn stream_timestamp(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    /// Browser transport: open the socket and return at once.
    ///
    /// There is nothing to cancel yet and nothing to wait for — `WebSocket` is
    /// asynchronous by construction, so `cancel` is only checked up front. The
    /// caller already polls [`Self::link_error`] and [`Self::iq_ready`] while
    /// the handshake completes, which is exactly what the native path needs
    /// too once it has spawned its reader.
    #[cfg(target_arch = "wasm32")]
    pub fn start_cancellable(&mut self, cancel: &AtomicBool) -> Result<Consumer<Complex32>> {
        if self.streaming {
            return Err(SourceError::InvalidState("already streaming"));
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(SourceError::Backend {
                op: "kiwi connect cancelled",
                code: -6,
            });
        }

        let (prod, cons) = RingBuffer::<Complex32>::new(1 << 16);
        let session = KiwiSession::new(self.rx_setup(), self.link_state());
        let socket = web::WebKiwiLink::open(
            &self.host,
            self.port,
            self.stream_timestamp(),
            session,
            prod,
            self.auth_lines(),
        )
        .map_err(|detail| {
            if let Ok(mut slot) = self.link_error.lock() {
                *slot = Some(detail);
            }
            SourceError::Backend {
                op: "kiwi connect",
                code: -2,
            }
        })?;

        self.link = Some(Link { socket });
        self.streaming = true;
        Ok(cons)
    }

    /// Shared handles the reader reports link status through.
    fn link_state(&self) -> KiwiLinkState {
        KiwiLinkState {
            dropped: Arc::clone(&self.dropped),
            rssi_cdbm: Arc::clone(&self.rssi_cdbm),
            iq_streaming: Arc::clone(&self.iq_streaming),
            link_error: Arc::clone(&self.link_error),
            has_rf_attn: Arc::clone(&self.has_rf_attn),
            rf_attn_cdb: Arc::clone(&self.rf_attn_cdb),
        }
    }

    fn rx_setup(&self) -> KiwiRxSetup {
        KiwiRxSetup {
            low_cut: self.low_cut,
            high_cut: self.high_cut,
            freq_hz: self.kiwi_freq_khz() * 1000.0,
            agc_on: self.agc_on,
            man_gain: self.man_gain,
            gen_attn: self.gen_attn,
            rf_attn_db: self.rf_attn_db,
            compression: self.compression,
            ar_out_hz: self.ar_out_hz,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn connect_ws(&self, cancel: &AtomicBool) -> Result<WebSocket<MaybeTlsStream<TcpStream>>> {
        let url = protocol::stream_url(false, &self.host, self.port, self.stream_timestamp());
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

    #[cfg(not(target_arch = "wasm32"))]
    pub fn start_cancellable(&mut self, cancel: &AtomicBool) -> Result<Consumer<Complex32>> {
        if self.streaming {
            return Err(SourceError::InvalidState("already streaming"));
        }

        #[cfg(any(test, coverage, mock_hal))]
        if crate::mock_hal::enabled() {
            return self.start_mock(cancel);
        }

        let mut ws = self.connect_ws(cancel)?;

        if let MaybeTlsStream::Plain(tcp) = ws.get_ref() {
            let _ = tcp.set_read_timeout(Some(READ_TIMEOUT));
        }

        for line in self.auth_lines() {
            if cancel.load(Ordering::Relaxed) {
                let _ = ws.close(None);
                return Err(SourceError::Backend {
                    op: "kiwi connect cancelled",
                    code: -6,
                });
            }
            ws.send(Message::Text(line.as_str().into()))
                .map_err(|_| SourceError::Backend {
                    op: "kiwi auth",
                    code: -5,
                })?;
        }

        let (prod, cons) = RingBuffer::<Complex32>::new(1 << 16);
        let (cmd_tx, cmd_rx) = mpsc::channel::<String>();
        let stop = Arc::new(AtomicBool::new(false));

        let stop_thread = Arc::clone(&stop);
        let session = KiwiSession::new(self.rx_setup(), self.link_state());
        let handle = thread::spawn(move || {
            reader_loop(ws, prod, cmd_rx, stop_thread, session);
        });

        self.stop = stop;
        self.link = Some(Link { cmd_tx: Some(cmd_tx), handle: Some(handle) });
        self.streaming = true;
        Ok(cons)
    }

    #[cfg(any(test, coverage, mock_hal))]
    fn start_mock(&mut self, cancel: &AtomicBool) -> Result<Consumer<Complex32>> {
        if cancel.load(Ordering::Relaxed) {
            return Err(SourceError::Backend {
                op: "kiwi connect cancelled",
                code: -6,
            });
        }
        let (mut prod, cons) = RingBuffer::<Complex32>::new(1 << 16);
        let stop_flag = Arc::clone(&self.stop);
        let stop_loop = Arc::clone(&stop_flag);
        let dropped = Arc::clone(&self.dropped);
        let iq_streaming = Arc::clone(&self.iq_streaming);
        let rssi = Arc::clone(&self.rssi_cdbm);
        let has_attn = Arc::clone(&self.has_rf_attn);
        has_attn.store(true, Ordering::Relaxed);
        let handle = thread::spawn(move || {
            let mut phase = 0.0f32;
            while !stop_loop.load(Ordering::Relaxed) {
                let sample = Complex32::new(phase.cos() * 0.2, phase.sin() * 0.2);
                if prod.push(sample).is_err() {
                    dropped.fetch_add(1, Ordering::Relaxed);
                }
                phase += std::f32::consts::TAU * 700.0 / KIWI_IQ_RATE as f32;
                rssi.store(-7300, Ordering::Relaxed);
                iq_streaming.store(true, Ordering::Relaxed);
                thread::sleep(Duration::from_millis(2));
            }
        });
        self.stop = stop_flag;
        self.link = Some(Link { cmd_tx: None, handle: Some(handle) });
        self.streaming = true;
        self.iq_streaming.store(true, Ordering::Relaxed);
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
        self.send_cmd(self.mod_cmd());
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
        self.link = None;
        self.streaming = false;
        Ok(())
    }

    fn dropped_samples(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    fn is_streaming(&self) -> bool {
        self.streaming
    }
}

impl KiwiSource {
    pub fn set_passband(&mut self, low_hz: i32, high_hz: i32) -> Result<()> {
        self.low_cut = low_hz;
        self.high_cut = high_hz;
        self.send_cmd(self.mod_cmd());
        Ok(())
    }

    pub fn set_agc(&mut self, on: bool) -> Result<()> {
        self.agc_on = on;
        self.send_cmd(protocol::agc_command(on, self.man_gain));
        Ok(())
    }

    pub fn set_man_gain(&mut self, gain: u8) -> Result<()> {
        self.man_gain = gain.clamp(0, 100);
        self.send_cmd(protocol::agc_command(self.agc_on, self.man_gain));
        Ok(())
    }

    pub fn set_rf_attn_db(&mut self, db: f32) -> Result<()> {
        let db = db.clamp(0.0, protocol::KIWI_RF_ATTN_MAX_DB);
        self.rf_attn_db = db;
        self.rf_attn_cdb
            .store((db * 10.0).round() as i32, Ordering::Relaxed);
        self.send_cmd(protocol::rf_attn_command(db));
        Ok(())
    }
}

impl KiwiControls for KiwiSource {
    fn supports_passband(&self) -> bool {
        true
    }

    fn set_passband(&mut self, low_hz: i32, high_hz: i32) -> Result<()> {
        KiwiSource::set_passband(self, low_hz, high_hz)
    }

    fn set_agc(&mut self, on: bool) -> Result<()> {
        KiwiSource::set_agc(self, on)
    }

    fn rf_agc_on(&self) -> bool {
        self.agc_on
    }

    fn set_man_gain(&mut self, gain: u8) -> Result<()> {
        KiwiSource::set_man_gain(self, gain)
    }

    fn set_rf_attn_db(&mut self, db: f32) -> Result<()> {
        KiwiSource::set_rf_attn_db(self, db)
    }

    fn has_rf_attn(&self) -> bool {
        self.has_rf_attn.load(Ordering::Relaxed)
    }

    fn rf_attn_db(&self) -> Option<f32> {
        let cdb = self.rf_attn_cdb.load(Ordering::Relaxed);
        if cdb < 0 {
            None
        } else {
            Some(cdb as f32 / 10.0)
        }
    }

    fn rssi_dbm(&self) -> Option<f32> {
        Some(self.meter_dbm())
    }

    fn hw_rf_gain(&self) -> Option<u8> {
        Some(self.man_gain)
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

// The Kiwi client uses TcpStream and a reader thread: native-only.
// A wasm frontend would drive the same protocol over the browser WebSocket API.
#[cfg(all(test, not(target_family = "wasm")))]
mod tests {
    use super::*;
    use crate::source::controls::KiwiControls;
    use crate::source::IqSource;
    use std::sync::atomic::Ordering;

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
        assert!(KiwiControls::supports_passband(&src));
        KiwiControls::set_passband(&mut src, -4_000, 4_000).unwrap();
        KiwiControls::set_agc(&mut src, true).unwrap();
        KiwiControls::set_man_gain(&mut src, 60).unwrap();
        assert!(!KiwiControls::has_rf_attn(&src));
        assert!(KiwiControls::rf_attn_db(&src).is_none());
        KiwiControls::set_rf_attn_db(&mut src, 6.0).unwrap();
        assert!(KiwiControls::rssi_dbm(&src).is_some());
        assert!(!KiwiControls::link_ready(&src));
        assert!(!KiwiControls::link_alive(&src));
        assert!(KiwiControls::link_error(&src).is_none());
        src.stop().unwrap();
    }

    #[test]
    fn meter_dbm_reads_atomic() {
        let src = KiwiSource::new("kiwi.test", 8073);
        src.rssi_cdbm.store(1_234, Ordering::Relaxed);
        assert!((src.meter_dbm() - 12.34).abs() < 1e-3);
    }

    #[test]
    fn builder_clamps_gain_and_rf_attn() {
        let src = KiwiSource::new("kiwi.test", 8073)
            .with_man_gain(150)
            .with_rf_attn_db(99.0)
            .with_ar_out_hz(1_000);
        assert_eq!(src.man_gain, 100);
        assert_eq!(src.rf_attn_db, protocol::KIWI_RF_ATTN_MAX_DB);
        assert_eq!(src.ar_out_hz, 8_000);
    }

    #[test]
    fn set_rf_attn_db_updates_stored_cdb() {
        let mut src = KiwiSource::new("kiwi.test", 8073);
        src.set_rf_attn_db(12.5).unwrap();
        assert_eq!(src.rf_attn_db, 12.5);
        assert_eq!(src.rf_attn_cdb.load(Ordering::Relaxed), 125);
        assert_eq!(KiwiControls::rf_attn_db(&src), Some(12.5));
    }

    #[test]
    fn default_passband_matches_protocol_half_width() {
        let src = KiwiSource::new("kiwi.test", 8073);
        let half = kiwi_iq_half_hz(KIWI_IQ_RATE);
        assert_eq!(src.low_cut, -half);
        assert_eq!(src.high_cut, half);
    }

    #[test]
    fn tune_before_streaming_does_not_panic() {
        let mut src = KiwiSource::new("kiwi.test", 8073)
            .with_freq_offset_khz(5.0);
        src.tune(14_030_000.0).unwrap();
        assert_eq!(src.frequency(), 14_030_000.0);
    }

    #[test]
    fn iq_ready_and_link_state_before_connect() {
        let src = KiwiSource::new("kiwi.test", 8073);
        assert!(!src.iq_ready());
        assert!(!src.link_alive());
        assert!(src.link_error().is_none());
    }

    #[test]
    fn has_rf_attn_reflects_atomic() {
        let src = KiwiSource::new("kiwi.test", 8073);
        assert!(!src.has_rf_attn.load(Ordering::Relaxed));
        src.has_rf_attn.store(true, Ordering::Relaxed);
        assert!(KiwiControls::has_rf_attn(&src));
    }

    #[test]
    fn start_while_streaming_is_invalid_state() {
        let mut src = KiwiSource::new("127.0.0.1", 1);
        src.streaming = true;
        assert!(matches!(src.start(), Err(SourceError::InvalidState(_))));
    }

    #[test]
    fn builder_gen_attn_and_compression_defaults() {
        let src = KiwiSource::new("kiwi.test", 8073).with_gen_attn(12);
        assert_eq!(src.gen_attn, 12);
        assert!(!src.compression);
    }

    #[test]
    fn stop_is_idempotent_before_streaming() {
        let mut src = KiwiSource::new("kiwi.test", 8073);
        src.stop().unwrap();
        src.stop().unwrap();
        assert!(!src.is_streaming());
    }
}

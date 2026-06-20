//! KiwiSDR front end: connects to a KiwiSDR over its WebSocket protocol,
//! requests IQ mode, and delivers ~12 kHz baseband IQ through the same
//! [`IqSource`] interface as the Airspy. Wire format reverse-checked against
//! the reference client (jks-prv/kiwiclient, `kiwi/client.py`).
//!
//! A SND frame is: `b"SND"` tag, 1 flags byte, u32-LE sequence, u16-BE S-meter,
//! then the payload. In IQ ("stereo") mode the payload starts with a 10-byte
//! GPS timestamp, followed by big-endian interleaved int16 I,Q pairs.

use crate::source::{Complex32, Consumer, IqSource, Result, SourceError};
use rtrb::{Producer, RingBuffer};
use std::io::ErrorKind;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

const KIWI_IQ_RATE: u32 = 12_000; // nominal; the server reports ~12001 Hz
const READ_TIMEOUT: Duration = Duration::from_millis(150);
const KEEPALIVE: Duration = Duration::from_secs(5);

type Ws = WebSocket<MaybeTlsStream<TcpStream>>;

/// A KiwiSDR IQ front end.
pub struct KiwiSource {
    host: String,
    port: u16,
    freq_hz: f64,
    low_cut: i32,
    high_cut: i32,
    streaming: bool,
    stop: Arc<AtomicBool>,
    dropped: Arc<AtomicU64>,
    rssi_cdbm: Arc<AtomicI32>, // S-meter in centi-dBm (rssi * 100)
    cmd_tx: Option<Sender<String>>,
    handle: Option<JoinHandle<()>>,
}

impl KiwiSource {
    /// Create a source for `ws://host:port` (the standard Kiwi port is 8073).
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            freq_hz: 0.0,
            low_cut: -5000,
            high_cut: 5000,
            streaming: false,
            stop: Arc::new(AtomicBool::new(false)),
            dropped: Arc::new(AtomicU64::new(0)),
            rssi_cdbm: Arc::new(AtomicI32::new(0)),
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

    /// Latest S-meter reading in dBm.
    pub fn rssi_dbm(&self) -> f32 {
        self.rssi_cdbm.load(Ordering::Relaxed) as f32 / 100.0
    }

    fn set_mod_cmd(&self) -> String {
        format!(
            "SET mod=iq low_cut={} high_cut={} freq={:.3}",
            self.low_cut,
            self.high_cut,
            self.freq_hz / 1000.0
        )
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
            // Ignore send errors: the reader thread may already be gone.
            let _ = tx.send(self.set_mod_cmd());
        }
        Ok(())
    }

    fn frequency(&self) -> f64 {
        self.freq_hz
    }

    fn start(&mut self) -> Result<Consumer<Complex32>> {
        if self.streaming {
            return Err(SourceError::InvalidState("already streaming"));
        }

        // Connect + handshake synchronously so errors surface here.
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let url = format!("ws://{}:{}/{}/SND", self.host, self.port, ts);
        let (mut ws, _resp) = tungstenite::connect(&url)
            .map_err(|_| SourceError::Backend { op: "kiwi connect", code: -1 })?;

        if let MaybeTlsStream::Plain(tcp) = ws.get_ref() {
            let _ = tcp.set_read_timeout(Some(READ_TIMEOUT));
        }

        // Opening handshake. The server replies with audio_rate, to which we
        // answer "SET AR OK ..." (handled in the reader loop) to start the flow.
        let setup = [
            "SET auth t=kiwi p=".to_string(),
            "SET ident_user=hfsdr".to_string(),
            self.set_mod_cmd(),
            "SET agc=1 hang=0 thresh=-100 slope=6 decay=1000 manGain=50".to_string(),
            "SET squelch=0 max=0".to_string(),
        ];
        for line in setup {
            ws.send(Message::Text(line.into()))
                .map_err(|_| SourceError::Backend { op: "kiwi handshake", code: -2 })?;
        }

        let (prod, cons) = RingBuffer::<Complex32>::new(1 << 16);
        let (cmd_tx, cmd_rx) = mpsc::channel::<String>();
        let stop = Arc::new(AtomicBool::new(false));

        let dropped = Arc::clone(&self.dropped);
        let rssi = Arc::clone(&self.rssi_cdbm);
        let stop_thread = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            reader_loop(ws, prod, cmd_rx, stop_thread, dropped, rssi);
        });

        self.stop = stop;
        self.cmd_tx = Some(cmd_tx);
        self.handle = Some(handle);
        self.streaming = true;
        Ok(cons)
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
}

impl Drop for KiwiSource {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// Owns the socket for the life of the stream: pumps outgoing commands, sends
/// keepalives, and parses inbound SND frames into the ring.
fn reader_loop(
    mut ws: Ws,
    mut prod: Producer<Complex32>,
    cmd_rx: Receiver<String>,
    stop: Arc<AtomicBool>,
    dropped: Arc<AtomicU64>,
    rssi: Arc<AtomicI32>,
) {
    let mut last_keepalive = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        // Forward any queued retune/control commands.
        while let Ok(cmd) = cmd_rx.try_recv() {
            if ws.send(Message::Text(cmd.into())).is_err() {
                return;
            }
        }
        if last_keepalive.elapsed() >= KEEPALIVE {
            if ws.send(Message::Text("SET keepalive".to_string().into())).is_err() {
                return;
            }
            last_keepalive = Instant::now();
        }

        match ws.read() {
            Ok(Message::Binary(buf)) => parse_snd(&buf, &mut prod, &dropped, &rssi),
            Ok(Message::Text(text)) => {
                if let Some(rate) = audio_rate(text.as_str()) {
                    let _ = ws.send(Message::Text(
                        format!("SET AR OK in={rate} out=44100").into(),
                    ));
                }
            }
            Ok(Message::Close(_)) => return,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {}
            Err(_) => return,
        }
    }
    let _ = ws.close(None);
}

/// Pull `audio_rate=<n>` out of a server MSG line, if present.
fn audio_rate(text: &str) -> Option<u32> {
    text.split_whitespace()
        .find_map(|tok| tok.strip_prefix("audio_rate="))
        .and_then(|v| v.parse::<u32>().ok())
}

/// Parse one SND frame and push its IQ samples into the ring.
fn parse_snd(buf: &[u8], prod: &mut Producer<Complex32>, dropped: &AtomicU64, rssi: &AtomicI32) {
    // tag(3) + flags(1) + seq(4) + smeter(2) = 10-byte header.
    if buf.len() < 10 || &buf[0..3] != b"SND" {
        return;
    }
    let smeter = u16::from_be_bytes([buf[8], buf[9]]);
    let rssi_dbm = 0.1 * smeter as f32 - 127.0;
    rssi.store((rssi_dbm * 100.0) as i32, Ordering::Relaxed);

    // IQ (stereo) payload: 10-byte GPS header, then big-endian int16 I,Q pairs.
    let payload = &buf[10..];
    if payload.len() < 10 {
        return;
    }
    let iq = &payload[10..];
    let pairs = iq.len() / 4; // 2 int16 (I,Q) = 4 bytes per complex sample
    let mut dropped_now = 0u64;
    for k in 0..pairs {
        let base = k * 4;
        let i = i16::from_be_bytes([iq[base], iq[base + 1]]) as f32 / 32768.0;
        let q = i16::from_be_bytes([iq[base + 2], iq[base + 3]]) as f32 / 32768.0;
        if prod.push(Complex32::new(i, q)).is_err() {
            dropped_now += 1;
        }
    }
    if dropped_now > 0 {
        dropped.fetch_add(dropped_now, Ordering::Relaxed);
    }
}

//! KiwiSDR front end: connects over WebSocket, requests IQ mode, and delivers
//! ~12 kHz baseband IQ through the same [`IqSource`] interface as the Airspy.
//! Wire format reverse-checked against the reference client (jks-prv/kiwiclient).

mod protocol;
mod reader;

use crate::source::{Complex32, Consumer, IqSource, Result, SourceError};
use protocol::{KIWI_IQ_RATE, KiwiRxSetup, mod_iq_command};
use reader::{READ_TIMEOUT, reader_loop};
use rtrb::RingBuffer;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
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
    agc_on: bool,
    compression: bool,
    streaming: bool,
    stop: Arc<AtomicBool>,
    dropped: Arc<AtomicU64>,
    rssi_cdbm: Arc<AtomicI32>,
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
            agc_on: true,
            compression: false,
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

    /// Enable or disable Kiwi AGC (default on).
    pub fn with_agc(mut self, on: bool) -> Self {
        self.agc_on = on;
        self
    }

    /// Latest S-meter reading in dBm.
    pub fn meter_dbm(&self) -> f32 {
        self.rssi_cdbm.load(Ordering::Relaxed) as f32 / 100.0
    }

    fn mod_cmd(&self) -> String {
        mod_iq_command(self.low_cut, self.high_cut, self.freq_hz)
    }

    fn rx_setup(&self) -> KiwiRxSetup {
        KiwiRxSetup {
            low_cut: self.low_cut,
            high_cut: self.high_cut,
            freq_hz: self.freq_hz,
            agc_on: self.agc_on,
            compression: self.compression,
        }
    }

    fn connect_ws(&self) -> Result<WebSocket<MaybeTlsStream<TcpStream>>> {
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
        let tcp = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).map_err(|_| {
            SourceError::Backend {
                op: "kiwi connect",
                code: -1,
            }
        })?;
        let request = url.into_client_request().map_err(|_| SourceError::Backend {
            op: "kiwi ws request",
            code: -4,
        })?;
        let (ws, _resp) = tungstenite::client::client(
            request,
            MaybeTlsStream::Plain(tcp),
        )
        .map_err(|_| SourceError::Backend {
            op: "kiwi handshake",
            code: -2,
        })?;
        Ok(ws)
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
        if self.streaming {
            return Err(SourceError::InvalidState("already streaming"));
        }

        let mut ws = self.connect_ws()?;

        if let MaybeTlsStream::Plain(tcp) = ws.get_ref() {
            let _ = tcp.set_read_timeout(Some(READ_TIMEOUT));
        }

        // Auth only — IQ mode is configured after the server sends sample_rate=…
        for line in ["SET auth t=kiwi p=", "SET ident_user=hfsdr"] {
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
        let stop_thread = Arc::clone(&stop);
        let rx_setup = self.rx_setup();
        let handle = thread::spawn(move || {
            reader_loop(ws, prod, cmd_rx, stop_thread, dropped, rssi, rx_setup);
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
            let cmd = format!(
                "SET agc={} hang=0 thresh=-100 slope=6 decay=1000 manGain=50",
                on as u8
            );
            let _ = tx.send(cmd);
        }
        Ok(())
    }
}

impl Drop for KiwiSource {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

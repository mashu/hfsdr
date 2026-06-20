//! WebSocket reader thread for KiwiSDR: command forwarding, keepalives, SND parsing.

use crate::kiwi::protocol::{audio_rate, msg_body_text, parse_snd, KiwiRxSetup};
use crate::source::Complex32;
use rtrb::Producer;
use std::io::ErrorKind;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

pub const READ_TIMEOUT: Duration = Duration::from_millis(150);
pub const KEEPALIVE: Duration = Duration::from_secs(5);

type Ws = WebSocket<MaybeTlsStream<TcpStream>>;

fn handle_msg_text(ws: &mut Ws, text: &str, rx_setup: &KiwiRxSetup, iq_configured: &mut bool) {
    if text.starts_with("sample_rate=") && !*iq_configured {
        for cmd in rx_setup.setup_commands() {
            if ws.send(Message::Text(cmd.into())).is_err() {
                return;
            }
        }
        *iq_configured = true;
    }
    if let Some(rate) = audio_rate(text) {
        let _ = ws.send(Message::Text(
            format!("SET AR OK in={rate} out=44100").into(),
        ));
    }
}

/// Owns the socket for the life of the stream: pumps outgoing commands, sends
/// keepalives, and parses inbound SND frames into the ring.
pub fn reader_loop(
    mut ws: Ws,
    mut prod: Producer<Complex32>,
    cmd_rx: Receiver<String>,
    stop: Arc<AtomicBool>,
    dropped: Arc<AtomicU64>,
    rssi: Arc<AtomicI32>,
    rx_setup: KiwiRxSetup,
) {
    let mut last_keepalive = Instant::now();
    let mut iq_configured = false;
    while !stop.load(Ordering::Relaxed) {
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
            Ok(Message::Binary(buf)) => {
                if buf.len() >= 3 && &buf[0..3] == b"SND" {
                    parse_snd(&buf, &mut prod, &dropped, &rssi);
                } else if let Some(text) = msg_body_text(&buf) {
                    handle_msg_text(&mut ws, text, &rx_setup, &mut iq_configured);
                }
            }
            Ok(Message::Text(text)) => {
                handle_msg_text(&mut ws, text.as_str(), &rx_setup, &mut iq_configured);
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

//! WebSocket reader thread for KiwiSDR: command forwarding, keepalives, SND parsing.

use crate::kiwi::protocol::{
    audio_rate, has_sample_rate, msg_body_text, parse_snd, KiwiRxSetup,
};
use crate::source::Complex32;
use rtrb::Producer;
use std::io::ErrorKind;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

pub const READ_TIMEOUT: Duration = Duration::from_millis(150);
pub const KEEPALIVE: Duration = Duration::from_secs(5);

type Ws = WebSocket<MaybeTlsStream<TcpStream>>;

fn send_text(ws: &mut Ws, cmd: &str) -> bool {
    ws.send(Message::Text(cmd.into())).is_ok()
}

fn configure_iq(ws: &mut Ws, rx_setup: &KiwiRxSetup) -> bool {
    for cmd in rx_setup.setup_commands() {
        if !send_text(ws, &cmd) {
            eprintln!("kiwi: failed to send {cmd}");
            return false;
        }
    }
    true
}

fn record_badp(link_error: &Mutex<Option<String>>, value: &str) {
    // badp=0 means password OK; only non-zero values are errors (kiwiclient).
    if value == "0" {
        return;
    }
    let msg = match value {
        "1" => {
            "All Kiwi public slots are busy, or the password is wrong. Try again in a few minutes or pick another receiver."
                .to_string()
        }
        "2" => "Kiwi is still determining your network address. Try again shortly.".to_string(),
        "3" => "Admin connection not allowed from your IP address.".to_string(),
        "4" => "No admin password set on this Kiwi (local network only).".to_string(),
        "5" => "This Kiwi does not allow multiple connections from your IP.".to_string(),
        "6" => "Kiwi database update in progress. Try again in a minute.".to_string(),
        "7" => "Another admin connection is already open on this Kiwi.".to_string(),
        other => format!("Kiwi refused connection (badp={other})"),
    };
    if let Ok(mut slot) = link_error.lock() {
        *slot = Some(msg);
    }
}

fn handle_msg_text(
    ws: &mut Ws,
    text: &str,
    rx_setup: &KiwiRxSetup,
    iq_configured: &mut bool,
    link_error: &Mutex<Option<String>>,
) {
    let params = crate::kiwi::protocol::kiwi_msg_params(text);
    if params.contains("badp=") {
        for tok in params.split_whitespace() {
            if let Some(v) = tok.strip_prefix("badp=") {
                record_badp(link_error, v);
            }
        }
    }

    if has_sample_rate(text) && !*iq_configured {
        if configure_iq(ws, rx_setup) {
            *iq_configured = true;
        }
    }
    if let Some(rate) = audio_rate(text) {
        let _ = send_text(ws, &format!("SET AR OK in={rate} out=44100"));
    }
}

fn flush_pending(ws: &mut Ws, pending: &mut Vec<String>) {
    for cmd in pending.drain(..) {
        if !send_text(ws, &cmd) {
            break;
        }
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
    iq_streaming: Arc<AtomicBool>,
    link_error: Arc<Mutex<Option<String>>>,
    rx_setup: KiwiRxSetup,
) {
    let mut last_keepalive = Instant::now();
    let mut iq_configured = false;
    let mut pending_cmds: Vec<String> = Vec::new();

    while !stop.load(Ordering::Relaxed) {
        while let Ok(cmd) = cmd_rx.try_recv() {
            if iq_configured {
                if !send_text(&mut ws, &cmd) {
                    return;
                }
            } else {
                pending_cmds.push(cmd);
            }
        }
        if last_keepalive.elapsed() >= KEEPALIVE {
            if !send_text(&mut ws, "SET keepalive") {
                return;
            }
            last_keepalive = Instant::now();
        }

        match ws.read() {
            Ok(Message::Binary(buf)) => {
                if buf.len() >= 3 && &buf[0..3] == b"SND" {
                    parse_snd(&buf, &mut prod, &dropped, &rssi);
                    iq_streaming.store(true, Ordering::Relaxed);
                } else if let Some(text) = msg_body_text(&buf) {
                    handle_msg_text(&mut ws, text, &rx_setup, &mut iq_configured, &link_error);
                    if iq_configured {
                        flush_pending(&mut ws, &mut pending_cmds);
                    }
                }
            }
            Ok(Message::Text(text)) => {
                handle_msg_text(&mut ws, text.as_str(), &rx_setup, &mut iq_configured, &link_error);
                if iq_configured {
                    flush_pending(&mut ws, &mut pending_cmds);
                }
            }
            Ok(Message::Close(_)) => return,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {}
            Err(e) => {
                eprintln!("kiwi reader: websocket error: {e}");
                return;
            }
        }

        if link_error.lock().is_ok_and(|e| e.is_some()) {
            return;
        }
    }
    let _ = ws.close(None);
}

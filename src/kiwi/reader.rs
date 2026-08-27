//! Native WebSocket transport for KiwiSDR: a thread with blocking reads.
//!
//! The protocol lives in [`super::session`]; this file only moves bytes. The
//! browser transport is the same session driven by JS callbacks instead.

use crate::source::Complex32;
use crate::time::{Duration, Instant};
use rtrb::Producer;
use std::io::ErrorKind;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use super::session::KiwiSession;

pub const READ_TIMEOUT: Duration = Duration::from_millis(150);
pub const KEEPALIVE: Duration = Duration::from_secs(5);

type Ws = WebSocket<MaybeTlsStream<TcpStream>>;

/// Send every command in order; `false` once the socket refuses one.
fn send_all(ws: &mut Ws, cmds: &[String]) -> bool {
    for cmd in cmds {
        if ws.send(Message::Text(cmd.as_str().into())).is_err() {
            crate::log::warn(format!("kiwi: failed to send {cmd}"));
            return false;
        }
    }
    true
}

/// Owns the socket for the life of the stream: pumps outgoing commands, sends
/// keepalives, and feeds inbound frames to the session.
pub fn reader_loop(
    mut ws: Ws,
    mut prod: Producer<Complex32>,
    cmd_rx: Receiver<String>,
    stop: Arc<AtomicBool>,
    mut session: KiwiSession,
) {
    let mut last_keepalive = Instant::now();

    while !stop.load(Ordering::Relaxed) {
        while let Ok(cmd) = cmd_rx.try_recv() {
            if let Some(cmd) = session.queue_command(cmd) {
                if !send_all(&mut ws, std::slice::from_ref(&cmd)) {
                    return;
                }
            }
        }
        if last_keepalive.elapsed() >= KEEPALIVE {
            if !send_all(&mut ws, &["SET keepalive".to_string()]) {
                return;
            }
            last_keepalive = Instant::now();
        }

        let replies = match ws.read() {
            Ok(Message::Binary(buf)) => session.on_binary(&buf, &mut prod),
            Ok(Message::Text(text)) => session.on_text(text.as_str()),
            Ok(Message::Close(_)) => {
                session
                    .link_state()
                    .record_first_error("Kiwi closed the connection");
                return;
            }
            Ok(_) => Vec::new(),
            Err(tungstenite::Error::Io(e))
                if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut =>
            {
                Vec::new()
            }
            Err(e) => {
                crate::log::warn(format!("kiwi reader: websocket error: {e}"));
                session
                    .link_state()
                    .record_first_error(&format!("Kiwi connection lost: {e}"));
                return;
            }
        };
        if !send_all(&mut ws, &replies) {
            return;
        }

        if session.failed() {
            return;
        }
    }
    let _ = ws.close(None);
}

//! WebSocket reader thread for KiwiSDR: command forwarding, keepalives, SND parsing.

use crate::kiwi::protocol::{
    audio_rate, has_rf_attn, has_sample_rate, msg_body_text, parse_snd, rf_attn_db, KiwiRxSetup,
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
            crate::log::warn(format!("kiwi: failed to send {cmd}"));
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

fn record_too_busy(link_error: &Mutex<Option<String>>, value: &str) {
    let slots = value.parse::<u32>().unwrap_or(0);
    let msg = if slots > 0 {
        format!(
            "KiwiSDR all {slots} client slots are busy — pick another receiver or retry in a minute"
        )
    } else {
        "KiwiSDR is busy (all client slots taken)".to_string()
    };
    if let Ok(mut slot) = link_error.lock() {
        *slot = Some(msg);
    }
}

fn record_link_lost(link_error: &Mutex<Option<String>>, detail: &str) {
    if let Ok(mut slot) = link_error.lock() {
        if slot.is_none() {
            *slot = Some(detail.to_string());
        }
    }
}

fn handle_msg_text(
    ws: &mut Ws,
    text: &str,
    rx_setup: &KiwiRxSetup,
    iq_configured: &mut bool,
    rf_attn_applied: &mut bool,
    has_attn_atomic: &AtomicBool,
    rf_attn_cdb: &AtomicI32,
    link_error: &Mutex<Option<String>>,
) {
    let params = crate::kiwi::protocol::kiwi_msg_params(text);
    if params.contains("badp=") && !*iq_configured {
        for tok in params.split_whitespace() {
            if let Some(v) = tok.strip_prefix("badp=") {
                record_badp(link_error, v);
            }
        }
    }
    if params.contains("too_busy=") && !*iq_configured {
        for tok in params.split_whitespace() {
            if let Some(v) = tok.strip_prefix("too_busy=") {
                record_too_busy(link_error, v);
            }
        }
    }

    if has_sample_rate(text) && !*iq_configured && configure_iq(ws, rx_setup) {
        *iq_configured = true;
    }
    if let Some(true) = has_rf_attn(text) {
        has_attn_atomic.store(true, Ordering::Relaxed);
    }
    if let Some(db) = rf_attn_db(text) {
        rf_attn_cdb.store((db * 10.0).round() as i32, Ordering::Relaxed);
    }
    if *iq_configured
        && has_attn_atomic.load(Ordering::Relaxed)
        && !*rf_attn_applied
    {
        if send_text(ws, &crate::kiwi::protocol::rf_attn_command(rx_setup.rf_attn_db)) {
            *rf_attn_applied = true;
            rf_attn_cdb.store(
                (rx_setup.rf_attn_db * 10.0).round() as i32,
                Ordering::Relaxed,
            );
        }
    }
    if let Some(rate) = audio_rate(text) {
        let _ = send_text(
            ws,
            &format!("SET AR OK in={rate} out={}", rx_setup.ar_out_hz),
        );
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
#[allow(clippy::too_many_arguments)]
pub fn reader_loop(
    mut ws: Ws,
    mut prod: Producer<Complex32>,
    cmd_rx: Receiver<String>,
    stop: Arc<AtomicBool>,
    dropped: Arc<AtomicU64>,
    rssi: Arc<AtomicI32>,
    iq_streaming: Arc<AtomicBool>,
    link_error: Arc<Mutex<Option<String>>>,
    has_attn_atomic: Arc<AtomicBool>,
    rf_attn_cdb: Arc<AtomicI32>,
    rx_setup: KiwiRxSetup,
) {
    let mut last_keepalive = Instant::now();
    let mut iq_configured = false;
    let mut rf_attn_applied = false;
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
                    handle_msg_text(
                        &mut ws,
                        text,
                        &rx_setup,
                        &mut iq_configured,
                        &mut rf_attn_applied,
                        &has_attn_atomic,
                        &rf_attn_cdb,
                        &link_error,
                    );
                    if iq_configured {
                        flush_pending(&mut ws, &mut pending_cmds);
                    }
                }
            }
            Ok(Message::Text(text)) => {
                handle_msg_text(
                    &mut ws,
                    text.as_str(),
                    &rx_setup,
                    &mut iq_configured,
                    &mut rf_attn_applied,
                    &has_attn_atomic,
                    &rf_attn_cdb,
                    &link_error,
                );
                if iq_configured {
                    flush_pending(&mut ws, &mut pending_cmds);
                }
            }
            Ok(Message::Close(_)) => {
                record_link_lost(&link_error, "Kiwi closed the connection");
                return;
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {}
            Err(e) => {
                crate::log::warn(format!("kiwi reader: websocket error: {e}"));
                record_link_lost(&link_error, &format!("Kiwi connection lost: {e}"));
                return;
            }
        }

        if link_error.lock().is_ok_and(|e| e.is_some()) {
            return;
        }
    }
    let _ = ws.close(None);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link_err() -> Arc<Mutex<Option<String>>> {
        Arc::new(Mutex::new(None))
    }

    #[test]
    fn record_badp_ignores_success() {
        let slot = link_err();
        record_badp(&slot, "0");
        assert!(slot.lock().unwrap().is_none());
    }

    #[test]
    fn record_badp_maps_known_codes() {
        let cases = [
            ("1", "public slots are busy"),
            ("2", "determining your network address"),
            ("3", "Admin connection not allowed"),
            ("4", "No admin password"),
            ("5", "does not allow multiple connections"),
            ("6", "database update in progress"),
            ("7", "Another admin connection"),
        ];
        for (code, fragment) in cases {
            let slot = link_err();
            record_badp(&slot, code);
            let msg = slot.lock().unwrap().clone().expect("message");
            assert!(
                msg.to_ascii_lowercase().contains(&fragment.to_ascii_lowercase()),
                "badp={code}: {msg}"
            );
        }
    }

    #[test]
    fn record_badp_unknown_code_includes_value() {
        let slot = link_err();
        record_badp(&slot, "99");
        assert_eq!(
            slot.lock().unwrap().as_deref(),
            Some("Kiwi refused connection (badp=99)")
        );
    }

    #[test]
    fn record_too_busy_with_slot_count() {
        let slot = link_err();
        record_too_busy(&slot, "4");
        let msg = slot.lock().unwrap().clone().unwrap();
        assert!(msg.contains("4"));
        assert!(msg.contains("busy"));
    }

    #[test]
    fn record_too_busy_without_count() {
        let slot = link_err();
        record_too_busy(&slot, "nope");
        assert!(slot
            .lock()
            .unwrap()
            .as_deref()
            .unwrap()
            .contains("busy"));
    }

    #[test]
    fn record_link_lost_only_sets_first_error() {
        let slot = link_err();
        record_link_lost(&slot, "first");
        record_link_lost(&slot, "second");
        assert_eq!(slot.lock().unwrap().as_deref(), Some("first"));
    }
}

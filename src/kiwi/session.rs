//! Transport-agnostic KiwiSDR protocol state machine.
//!
//! The Kiwi link is a WebSocket in every build, but the socket itself differs:
//! natively it is [`tungstenite`] over a `TcpStream` read from a thread, and in
//! the browser it is `WebSocket` driven by JS callbacks. Only the *transport*
//! differs — the handshake, the IQ-mode setup, the error reporting and the SND
//! unpacking are identical.
//!
//! So they live here, once. A caller feeds frames in and sends whatever
//! commands come back out. Two copies of this logic would drift apart, and the
//! drift would only show up against a real receiver.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use rtrb::Producer;

use crate::source::Complex32;

use super::protocol::{
    audio_rate, has_rf_attn, has_sample_rate, kiwi_msg_params, parse_snd, rf_attn_command,
    rf_attn_db, KiwiRxSetup,
};

/// Shared state a [`KiwiSession`] reports back to the owning `KiwiSource`.
///
/// Grouped into one struct because both transports need to hand over the same
/// six handles, and a six-argument constructor invites mismatched call sites.
#[derive(Clone)]
pub struct KiwiLinkState {
    pub dropped: Arc<AtomicU64>,
    pub rssi_cdbm: Arc<AtomicI32>,
    pub iq_streaming: Arc<AtomicBool>,
    pub link_error: Arc<Mutex<Option<String>>>,
    pub has_rf_attn: Arc<AtomicBool>,
    pub rf_attn_cdb: Arc<AtomicI32>,
}

impl KiwiLinkState {
    fn record_error(&self, msg: String) {
        if let Ok(mut slot) = self.link_error.lock() {
            *slot = Some(msg);
        }
    }

    /// Record a reason only if none was recorded yet: the first failure is the
    /// informative one, later ones are usually its consequences.
    pub fn record_first_error(&self, detail: &str) {
        if let Ok(mut slot) = self.link_error.lock() {
            if slot.is_none() {
                *slot = Some(detail.to_string());
            }
        }
    }

    pub fn failed(&self) -> bool {
        self.link_error.lock().is_ok_and(|e| e.is_some())
    }
}

/// Human-readable reason for a Kiwi `badp=` rejection.
///
/// `badp=0` means the password was accepted, so it is not an error.
pub fn badp_reason(value: &str) -> Option<String> {
    if value == "0" {
        return None;
    }
    Some(match value {
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
    })
}

/// Human-readable reason for a Kiwi `too_busy=` rejection.
pub fn too_busy_reason(value: &str) -> String {
    match value.parse::<u32>().unwrap_or(0) {
        0 => "KiwiSDR is busy (all client slots taken)".to_string(),
        slots => format!(
            "KiwiSDR all {slots} client slots are busy — pick another receiver or retry in a minute"
        ),
    }
}

/// Protocol state for one Kiwi connection.
///
/// Every method returns the commands the caller must write to the socket, in
/// order. The session never touches the socket itself — that is the whole point
/// of the split.
pub struct KiwiSession {
    rx_setup: KiwiRxSetup,
    state: KiwiLinkState,
    /// IQ mode has been requested; until then UI commands are held back because
    /// the Kiwi ignores them before setup.
    iq_configured: bool,
    rf_attn_applied: bool,
    pending: Vec<String>,
}

impl KiwiSession {
    pub fn new(rx_setup: KiwiRxSetup, state: KiwiLinkState) -> Self {
        Self {
            rx_setup,
            state,
            iq_configured: false,
            rf_attn_applied: false,
            pending: Vec::new(),
        }
    }

    pub fn link_state(&self) -> &KiwiLinkState {
        &self.state
    }

    /// True once IQ mode has been requested and UI commands flow straight through.
    pub fn iq_configured(&self) -> bool {
        self.iq_configured
    }

    pub fn failed(&self) -> bool {
        self.state.failed()
    }

    /// A UI command to forward, or `None` if it was queued until setup completes.
    pub fn queue_command(&mut self, cmd: String) -> Option<String> {
        if self.iq_configured {
            Some(cmd)
        } else {
            self.pending.push(cmd);
            None
        }
    }

    /// Handle one binary frame: either SND payload or a text message in binary
    /// clothing. Returns commands to send.
    pub fn on_binary(&mut self, buf: &[u8], prod: &mut Producer<Complex32>) -> Vec<String> {
        if buf.len() >= 3 && &buf[0..3] == b"SND" {
            parse_snd(buf, prod, &self.state.dropped, &self.state.rssi_cdbm);
            self.state.iq_streaming.store(true, Ordering::Relaxed);
            return Vec::new();
        }
        match super::protocol::msg_body_text(buf) {
            Some(text) => {
                // `text` borrows `buf`, so copy before taking `&mut self`.
                let owned = text.to_string();
                self.on_text(&owned)
            }
            None => Vec::new(),
        }
    }

    /// Handle one text message. Returns commands to send, in order.
    pub fn on_text(&mut self, text: &str) -> Vec<String> {
        let mut out = Vec::new();
        let params = kiwi_msg_params(text);

        // Rejections only matter before setup; afterwards the slot is ours.
        if !self.iq_configured {
            for tok in params.split_whitespace() {
                if let Some(v) = tok.strip_prefix("badp=") {
                    if let Some(msg) = badp_reason(v) {
                        self.state.record_error(msg);
                    }
                } else if let Some(v) = tok.strip_prefix("too_busy=") {
                    self.state.record_error(too_busy_reason(v));
                }
            }
        }

        if has_sample_rate(text) && !self.iq_configured {
            out.extend(self.rx_setup.setup_commands());
            self.iq_configured = true;
            out.append(&mut self.pending);
        }

        if let Some(true) = has_rf_attn(text) {
            self.state.has_rf_attn.store(true, Ordering::Relaxed);
        }
        if let Some(db) = rf_attn_db(text) {
            self.state
                .rf_attn_cdb
                .store((db * 10.0).round() as i32, Ordering::Relaxed);
        }
        if self.iq_configured
            && self.state.has_rf_attn.load(Ordering::Relaxed)
            && !self.rf_attn_applied
        {
            out.push(rf_attn_command(self.rx_setup.rf_attn_db));
            self.rf_attn_applied = true;
            self.state.rf_attn_cdb.store(
                (self.rx_setup.rf_attn_db * 10.0).round() as i32,
                Ordering::Relaxed,
            );
        }

        if let Some(rate) = audio_rate(text) {
            out.push(format!(
                "SET AR OK in={rate} out={}",
                self.rx_setup.ar_out_hz
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link_state() -> KiwiLinkState {
        KiwiLinkState {
            dropped: Arc::new(AtomicU64::new(0)),
            rssi_cdbm: Arc::new(AtomicI32::new(0)),
            iq_streaming: Arc::new(AtomicBool::new(false)),
            link_error: Arc::new(Mutex::new(None)),
            has_rf_attn: Arc::new(AtomicBool::new(false)),
            rf_attn_cdb: Arc::new(AtomicI32::new(0)),
        }
    }

    fn rx_setup() -> KiwiRxSetup {
        KiwiRxSetup {
            low_cut: -5_980,
            high_cut: 5_980,
            freq_hz: 14_050_000.0,
            agc_on: true,
            man_gain: 100,
            gen_attn: 0,
            rf_attn_db: 12.0,
            compression: false,
            ar_out_hz: 44_100,
        }
    }

    fn session() -> KiwiSession {
        KiwiSession::new(rx_setup(), link_state())
    }

    #[test]
    fn badp_success_is_not_an_error() {
        assert!(badp_reason("0").is_none());
    }

    #[test]
    fn badp_maps_known_codes() {
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
            let msg = badp_reason(code).expect("message");
            assert!(
                msg.to_ascii_lowercase()
                    .contains(&fragment.to_ascii_lowercase()),
                "badp={code}: {msg}"
            );
        }
    }

    #[test]
    fn badp_unknown_code_includes_value() {
        assert_eq!(
            badp_reason("99").as_deref(),
            Some("Kiwi refused connection (badp=99)")
        );
    }

    #[test]
    fn too_busy_reports_slot_count_when_given() {
        let msg = too_busy_reason("4");
        assert!(msg.contains('4'), "{msg}");
        assert!(msg.contains("busy"), "{msg}");
        assert!(too_busy_reason("nope").contains("busy"));
    }

    #[test]
    fn first_error_wins() {
        let state = link_state();
        state.record_first_error("first");
        state.record_first_error("second");
        assert_eq!(state.link_error.lock().unwrap().as_deref(), Some("first"));
    }

    /// A rejection arriving before setup must be surfaced, not swallowed.
    #[test]
    fn rejection_before_setup_is_recorded() {
        let mut s = session();
        let replies = s.on_text("MSG badp=1");
        assert!(replies.is_empty(), "a rejection must not trigger setup");
        assert!(s.failed());
        assert!(
            s.link_state()
                .link_error
                .lock()
                .unwrap()
                .as_deref()
                .unwrap()
                .contains("busy")
        );
    }

    /// `badp=0` is the success case and must not poison the link.
    #[test]
    fn accepted_password_does_not_fail_the_link() {
        let mut s = session();
        s.on_text("MSG badp=0");
        assert!(!s.failed());
    }

    /// The sample-rate message is what triggers IQ setup, exactly once.
    #[test]
    fn sample_rate_triggers_setup_once() {
        let mut s = session();
        assert!(!s.iq_configured());
        let first = s.on_text("MSG audio_init=0 audio_rate=12000 sample_rate=12000.123");
        assert!(!first.is_empty(), "setup commands must be emitted");
        assert!(s.iq_configured());

        let second = s.on_text("MSG audio_init=0 audio_rate=12000 sample_rate=12000.123");
        assert!(
            !second.iter().any(|c| c.starts_with("SET mod=iq")),
            "setup must not be repeated: {second:?}"
        );
    }

    /// Commands issued before setup are held and replayed in order, otherwise
    /// the Kiwi silently ignores them and the UI's tuning is lost.
    #[test]
    fn commands_before_setup_are_queued_then_flushed_in_order() {
        let mut s = session();
        assert_eq!(s.queue_command("SET first".into()), None);
        assert_eq!(s.queue_command("SET second".into()), None);

        let replies = s.on_text("MSG sample_rate=12000.0");
        let first = replies.iter().position(|c| c == "SET first");
        let second = replies.iter().position(|c| c == "SET second");
        assert!(first.is_some() && second.is_some(), "{replies:?}");
        assert!(first < second, "queued order not preserved: {replies:?}");

        // After setup they pass straight through.
        assert_eq!(
            s.queue_command("SET third".into()),
            Some("SET third".to_string())
        );
    }

    /// RF attenuation is only meaningful on Kiwis that report the capability,
    /// and must be applied once rather than on every message.
    #[test]
    fn rf_attn_applied_once_and_only_when_supported() {
        let mut s = session();
        s.on_text("MSG sample_rate=12000.0");

        let none = s.on_text("MSG some_other_field=1");
        assert!(
            !none.iter().any(|c| c.contains("SET rf_attn")),
            "attenuation sent without capability: {none:?}"
        );

        let first = s.on_text("MSG has_attn=1");
        assert!(
            first.iter().any(|c| c.contains("rf_attn")),
            "attenuation not applied once supported: {first:?}"
        );
        let second = s.on_text("MSG has_attn=1");
        assert!(
            !second.iter().any(|c| c.contains("SET rf_attn")),
            "attenuation re-applied: {second:?}"
        );
    }

    /// SND frames must reach the ring and raise the streaming flag; a short or
    /// non-SND buffer must not.
    #[test]
    fn snd_frames_feed_the_ring_and_raise_streaming() {
        let mut s = session();
        let (mut prod, cons) = rtrb::RingBuffer::<Complex32>::new(4096);

        let replies = s.on_binary(b"XX", &mut prod);
        assert!(replies.is_empty());
        assert!(
            !s.link_state().iq_streaming.load(Ordering::Relaxed),
            "a truncated frame must not count as streaming"
        );

        // SND + 3-byte header fields the parser skips, then IQ payload.
        let mut frame = b"SND".to_vec();
        frame.extend_from_slice(&[0u8; 7]);
        frame.extend(std::iter::repeat_n(0x10u8, 256));
        s.on_binary(&frame, &mut prod);
        assert!(
            s.link_state().iq_streaming.load(Ordering::Relaxed),
            "SND frame did not mark the link as streaming"
        );
        assert!(cons.slots() > 0, "SND payload produced no IQ samples");
    }
}

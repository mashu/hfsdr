//! Handshake, stall detection, auto-reconnect.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use crate::source::SourceKind;

use super::Engine;
use crate::engine::policy::{handshake_timeout, reconnect_retry_secs, stall_timeout, RECONNECT_BUSY_DELAY_SECS};
use crate::engine::types::ConnState;


impl Engine {
pub(super) fn poll_handshake(&mut self) {
        if self.first_iq_received {
            return;
        }
        let link_error = self
            .conn
            .as_ref()
            .and_then(|c| c.device.link_error());
        let alive = self
            .conn
            .as_ref()
            .is_some_and(|c| c.device.link_alive());
        if let Some(err) = link_error {
            self.fail_connection(err);
            return;
        }
        if !alive {
            self.fail_connection("Kiwi disconnected during handshake".into());
            return;
        }
        if self.connected_at.elapsed() > self.handshake_timeout() {
            self.fail_connection("Kiwi handshake timed out (no IQ data)".into());
        }
    }

    pub(super) fn fail_connection(&mut self, reason: String) {
        self.teardown();
        if self.request.is_none() || self.connect_cancel.load(Ordering::Relaxed) {
            self.set_error(None);
            self.set_state(ConnState::Disconnected);
            return;
        }
        self.set_error(Some(reason));
        self.schedule_reconnect();
        self.set_state(ConnState::Reconnecting {
            attempt: self.reconnect_attempt,
            retry_in_s: self.retry_secs(),
        });
    }

    pub(super) fn maybe_reconnect_on_stall(&mut self) {
        let link_error = self.conn.as_ref().and_then(|c| c.device.link_error());
        let reader_dead = self.conn.as_ref().is_some_and(|c| {
            c.is_kiwi && c.device.is_streaming() && !c.device.link_alive()
        });
        let stalled = if self.first_iq_received {
            self.last_data.elapsed() > self.stall_timeout()
        } else {
            self.connected_at.elapsed() > self.handshake_timeout()
        };
        if link_error.is_some() || reader_dead || stalled {
            let reason = link_error.unwrap_or_else(|| {
                if reader_dead {
                    "Kiwi reader stopped unexpectedly".to_string()
                } else if self.first_iq_received {
                    "connection stalled (no data)".to_string()
                } else {
                    "Kiwi handshake timed out (no IQ data)".to_string()
                }
            });
            self.fail_connection(reason);
        }
    }

    pub(super) fn handshake_timeout(&self) -> Duration {
        handshake_timeout(self.conn.as_ref().is_some_and(|c| c.is_kiwi))
    }

    pub(super) fn stall_timeout(&self) -> Duration {
        stall_timeout(self.conn.as_ref().is_some_and(|c| c.is_kiwi))
    }

    pub(super) fn is_kiwi_request(&self) -> bool {
        self.request
            .as_ref()
            .is_some_and(|r| r.kind == SourceKind::Kiwi)
    }

    pub(super) fn maybe_retry_reconnect(&mut self) {
        let Some(req) = self.request.clone() else {
            return;
        };
        let Some(at) = self.retry_at else {
            return;
        };
        let remaining = at.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            self.start_connect(&req);
        } else {
            self.set_state(ConnState::Reconnecting {
                attempt: self.reconnect_attempt,
                retry_in_s: remaining.as_secs_f32(),
            });
        }
    }

    pub(super) fn schedule_reconnect(&mut self) {
        self.reconnect_attempt = self.reconnect_attempt.saturating_add(1);
        let busy = self
            .shared
            .lock()
            .ok()
            .and_then(|g| g.last_error.clone())
            .is_some_and(|e| e.to_ascii_lowercase().contains("busy"));
        let secs = if busy {
            RECONNECT_BUSY_DELAY_SECS
        } else {
            reconnect_retry_secs(self.is_kiwi_request(), self.reconnect_attempt)
        };
        self.retry_at = Some(Instant::now() + Duration::from_secs_f32(secs));
    }

    pub(super) fn retry_secs(&self) -> f32 {
        reconnect_retry_secs(self.is_kiwi_request(), self.reconnect_attempt)
    }

    pub(super) fn set_state(&self, state: ConnState) {
        if let Ok(mut guard) = self.shared.lock() {
            guard.state = state;
        }
    }

    pub(super) fn set_error(&self, error: Option<String>) {
        if let Ok(mut guard) = self.shared.lock() {
            guard.last_error = error;
        }
    }
}

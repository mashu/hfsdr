//! Browser WebSocket transport for KiwiSDR.
//!
//! The native transport blocks on reads from its own thread. A tab has neither
//! blocking sockets nor (without cross-origin isolation) threads, so the same
//! [`KiwiSession`] is driven from JS callbacks instead: `onmessage` feeds frames
//! in and writes the replies straight back to the socket.
//!
//! ## Why connecting can fail for reasons that are not the receiver's fault
//!
//! A page served over `https:` cannot open a `ws://` socket — browsers class it
//! as mixed content and block it outright, with no user override. Most KiwiSDRs
//! serve plain HTTP on port 8073. So [`stream_url`] follows the page's own
//! scheme, and a plain-HTTP receiver is reachable only from a page that is
//! itself served over HTTP (including `http://localhost`).

use std::cell::RefCell;
use std::rc::Rc;

use rtrb::Producer;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CloseEvent, ErrorEvent, MessageEvent, WebSocket};

use crate::source::Complex32;

use super::protocol::stream_url;
use super::session::{KiwiLinkState, KiwiSession};

/// True when the document itself was served over TLS, so sockets must be `wss:`.
///
/// Defaults to secure when the location is unreadable: guessing `ws://` there
/// produces a mixed-content block whose console error looks nothing like the
/// real problem.
fn page_is_secure() -> bool {
    web_sys::window()
        .and_then(|w| w.location().protocol().ok())
        .map(|p| p.eq_ignore_ascii_case("https:"))
        .unwrap_or(true)
}

/// Everything the JS callbacks mutate. Single-threaded, so `RefCell` is enough.
struct LinkInner {
    session: KiwiSession,
    prod: Producer<Complex32>,
}

/// A live browser WebSocket to one KiwiSDR.
pub struct WebKiwiLink {
    ws: WebSocket,
    inner: Rc<RefCell<LinkInner>>,
    state: KiwiLinkState,
    /// Kept alive for the socket's lifetime: dropping a closure detaches it.
    _callbacks: Vec<Closure<dyn FnMut(JsValue)>>,
    /// Commands issued before the socket opened, replayed on `onopen`.
    preopen: Rc<RefCell<Vec<String>>>,
}

/// Send `cmds` on `ws`, reporting the first failure into `state`.
fn send_all(ws: &WebSocket, state: &KiwiLinkState, cmds: &[String]) {
    for cmd in cmds {
        if ws.send_with_str(cmd).is_err() {
            state.record_first_error("Kiwi connection lost while sending");
            return;
        }
    }
}

impl WebKiwiLink {
    /// Open the socket and wire up the session. Returns immediately: the
    /// handshake completes later, and the caller watches [`Self::link_error`]
    /// and the session's `iq_streaming` flag the same way the native path does.
    pub fn open(
        host: &str,
        port: u16,
        timestamp_secs: u64,
        session: KiwiSession,
        prod: Producer<Complex32>,
        auth_lines: Vec<String>,
    ) -> Result<Self, String> {
        let url = stream_url(page_is_secure(), host, port, timestamp_secs);
        let ws = WebSocket::new(&url).map_err(|e| {
            format!(
                "could not open {url}: {}",
                e.as_string().unwrap_or_else(|| "blocked by the browser".into())
            )
        })?;
        // SND frames are binary; without this they arrive as Blobs and would
        // need an async read before they could be parsed.
        ws.set_binary_type(web_sys::BinaryType::Arraybuffer);

        let state = session.link_state().clone();
        let inner = Rc::new(RefCell::new(LinkInner { session, prod }));
        let preopen: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let mut callbacks: Vec<Closure<dyn FnMut(JsValue)>> = Vec::new();

        {
            let ws_cb = ws.clone();
            let state_cb = state.clone();
            let preopen_cb = Rc::clone(&preopen);
            let cb = Closure::<dyn FnMut(JsValue)>::new(move |_: JsValue| {
                send_all(&ws_cb, &state_cb, &auth_lines);
                let queued: Vec<String> = preopen_cb.borrow_mut().drain(..).collect();
                send_all(&ws_cb, &state_cb, &queued);
            });
            ws.set_onopen(Some(cb.as_ref().unchecked_ref()));
            callbacks.push(cb);
        }

        {
            let ws_cb = ws.clone();
            let state_cb = state.clone();
            let inner_cb = Rc::clone(&inner);
            let cb = Closure::<dyn FnMut(JsValue)>::new(move |ev: JsValue| {
                let Ok(ev) = ev.dyn_into::<MessageEvent>() else {
                    return;
                };
                let data = ev.data();
                let replies = if let Some(buf) = data.dyn_ref::<js_sys::ArrayBuffer>() {
                    let bytes = js_sys::Uint8Array::new(buf).to_vec();
                    let mut inner = inner_cb.borrow_mut();
                    let LinkInner { session, prod } = &mut *inner;
                    session.on_binary(&bytes, prod)
                } else if let Some(text) = data.as_string() {
                    inner_cb.borrow_mut().session.on_text(&text)
                } else {
                    return;
                };
                if !replies.is_empty() {
                    send_all(&ws_cb, &state_cb, &replies);
                }
            });
            ws.set_onmessage(Some(cb.as_ref().unchecked_ref()));
            callbacks.push(cb);
        }

        {
            // The browser deliberately withholds the reason for a failed
            // WebSocket handshake, so name the causes the user can act on.
            let state_cb = state.clone();
            let url_cb = url.clone();
            let cb = Closure::<dyn FnMut(JsValue)>::new(move |ev: JsValue| {
                let detail = ev
                    .dyn_ref::<ErrorEvent>()
                    .map(|e| e.message())
                    .filter(|m| !m.is_empty())
                    .unwrap_or_else(|| {
                        "the receiver is unreachable, refused the connection, or the page's \
                         security policy blocked it"
                            .to_string()
                    });
                state_cb.record_first_error(&format!("Could not connect to {url_cb}: {detail}"));
            });
            ws.set_onerror(Some(cb.as_ref().unchecked_ref()));
            callbacks.push(cb);
        }

        {
            let state_cb = state.clone();
            let cb = Closure::<dyn FnMut(JsValue)>::new(move |ev: JsValue| {
                let detail = ev
                    .dyn_ref::<CloseEvent>()
                    .map(|e| {
                        let reason = e.reason();
                        if reason.is_empty() {
                            format!("code {}", e.code())
                        } else {
                            reason
                        }
                    })
                    .unwrap_or_else(|| "connection closed".to_string());
                state_cb.record_first_error(&format!("Kiwi closed the connection ({detail})"));
            });
            ws.set_onclose(Some(cb.as_ref().unchecked_ref()));
            callbacks.push(cb);
        }

        Ok(Self {
            ws,
            inner,
            state,
            _callbacks: callbacks,
            preopen,
        })
    }

    /// Forward a UI command, holding it back until the link can accept it.
    ///
    /// Two separate gates: the socket may not be open yet, and the Kiwi ignores
    /// commands sent before IQ mode is configured.
    pub fn send_command(&self, cmd: String) {
        if self.ws.ready_state() != WebSocket::OPEN {
            self.preopen.borrow_mut().push(cmd);
            return;
        }
        if let Some(cmd) = self.inner.borrow_mut().session.queue_command(cmd) {
            send_all(&self.ws, &self.state, &[cmd]);
        }
    }

    /// Keepalive; the native transport sends this from its read loop timer.
    pub fn keepalive(&self) {
        if self.ws.ready_state() == WebSocket::OPEN {
            let _ = self.ws.send_with_str("SET keepalive");
        }
    }

    /// Whether the socket is still connecting or open.
    pub fn alive(&self) -> bool {
        matches!(
            self.ws.ready_state(),
            WebSocket::CONNECTING | WebSocket::OPEN
        )
    }

    pub fn close(&self) {
        // Detach first: onclose would otherwise record a link error for a
        // shutdown the caller asked for.
        self.ws.set_onclose(None);
        self.ws.set_onerror(None);
        self.ws.set_onmessage(None);
        let _ = self.ws.close();
    }
}

impl Drop for WebKiwiLink {
    fn drop(&mut self) {
        self.close();
    }
}

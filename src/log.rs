//! Unified stderr logging for the library and GUI.
//!
//! All messages are written to stderr. The waterfall app may install a [`set_sink`]
//! callback so the in-app console panel mirrors the same lines.

use std::fmt::Display;
use std::sync::{Mutex, OnceLock};

type LogSink = Box<dyn Fn(&str) + Send + Sync>;

static SINK: OnceLock<Mutex<Option<LogSink>>> = OnceLock::new();

fn sink_slot() -> &'static Mutex<Option<LogSink>> {
    SINK.get_or_init(|| Mutex::new(None))
}

/// Optional hook for the GUI log ring buffer (or other collectors).
pub fn set_sink(sink: Option<LogSink>) {
    if let Ok(mut slot) = sink_slot().lock() {
        *slot = sink;
    }
}

fn emit(level: &str, msg: impl Display) {
    let line = format!("[{level}] {msg}");
    if let Ok(slot) = sink_slot().lock() {
        if let Some(ref sink) = *slot {
            sink(&line);
        }
    }
    eprintln!("{line}");
}

pub fn info(msg: impl Display) {
    emit("INFO", msg);
}

pub fn warn(msg: impl Display) {
    emit("WARN", msg);
}

pub fn error(msg: impl Display) {
    emit("ERROR", msg);
}

pub fn debug(msg: impl Display) {
    #[cfg(debug_assertions)]
    emit("DEBUG", msg);
    #[cfg(not(debug_assertions))]
    let _ = &msg;
}

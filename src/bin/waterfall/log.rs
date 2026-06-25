//! In-app log ring buffer (hidden console panel). Also mirrors to stderr.

use std::collections::VecDeque;
use std::fmt::Display;
use std::sync::{Mutex, OnceLock};

const CAPACITY: usize = 400;

static LOG: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();

fn buffer() -> &'static Mutex<VecDeque<String>> {
    LOG.get_or_init(|| Mutex::new(VecDeque::with_capacity(CAPACITY)))
}

pub fn init() {
    let _ = buffer();
}

fn push(level: &str, msg: impl Display) {
    let line = format!("[{level}] {msg}");
    if let Ok(mut q) = buffer().lock() {
        if q.len() >= CAPACITY {
            q.pop_front();
        }
        q.push_back(line.clone());
    }
    eprintln!("{line}");
}

pub fn info(msg: impl Display) {
    push("INFO", msg);
}

pub fn warn(msg: impl Display) {
    push("WARN", msg);
}

pub fn error(msg: impl Display) {
    push("ERROR", msg);
}

pub fn entries() -> Vec<String> {
    buffer()
        .lock()
        .map(|q| q.iter().cloned().collect())
        .unwrap_or_default()
}

pub fn clear() {
    if let Ok(mut q) = buffer().lock() {
        q.clear();
    }
}

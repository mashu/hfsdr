//! In-app log ring buffer (hidden console panel). Mirrors [`hfsdr::log`] to stderr.

use std::collections::VecDeque;
use std::fmt::Display;
use std::sync::{Mutex, OnceLock};

const CAPACITY: usize = 400;

static LOG: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();

fn buffer() -> &'static Mutex<VecDeque<String>> {
    LOG.get_or_init(|| Mutex::new(VecDeque::with_capacity(CAPACITY)))
}

fn push_line(line: &str) {
    if let Ok(mut q) = buffer().lock() {
        if q.len() >= CAPACITY {
            q.pop_front();
        }
        q.push_back(line.to_string());
    }
}

pub fn init() {
    let _ = buffer();
    hfsdr::log::set_sink(Some(Box::new(push_line)));
}

pub fn info(msg: impl Display) {
    hfsdr::log::info(msg);
}

pub fn warn(msg: impl Display) {
    hfsdr::log::warn(msg);
}

pub fn error(msg: impl Display) {
    hfsdr::log::error(msg);
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

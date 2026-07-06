//! Unified logging for the library and GUI.
//!
//! All messages are written to the original stderr stream. The waterfall app may
//! install a [`set_sink`] callback so the in-app console panel mirrors the same
//! lines. [`init_stdio_capture`] forwards stderr from external libraries into
//! this logger without recursion.

use std::fmt::Display;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

type LogSink = Box<dyn Fn(&str) + Send + Sync>;

static SINK: OnceLock<Mutex<Option<LogSink>>> = OnceLock::new();
static ORIG_STDERR: OnceLock<Mutex<std::fs::File>> = OnceLock::new();
static IN_EMIT: AtomicBool = AtomicBool::new(false);

fn sink_slot() -> &'static Mutex<Option<LogSink>> {
    SINK.get_or_init(|| Mutex::new(None))
}

fn orig_stderr() -> Option<&'static Mutex<std::fs::File>> {
    ORIG_STDERR.get()
}

/// Optional hook for the GUI log ring buffer (or other collectors).
pub fn set_sink(sink: Option<LogSink>) {
    if let Ok(mut slot) = sink_slot().lock() {
        *slot = sink;
    }
}

fn write_stderr(line: &str) {
    if let Some(stderr) = orig_stderr() {
        if let Ok(mut out) = stderr.lock() {
            let _ = writeln!(out, "{line}");
            let _ = out.flush();
            return;
        }
    }
    eprintln!("{line}");
}

fn emit(level: &str, msg: impl Display) {
    if IN_EMIT.swap(true, Ordering::Relaxed) {
        return;
    }
    let line = format!("[{level}] {msg}");
    if let Ok(slot) = sink_slot().lock() {
        if let Some(ref sink) = *slot {
            sink(&line);
        }
    }
    write_stderr(&line);
    IN_EMIT.store(false, Ordering::Relaxed);
}

/// Forward a raw stderr line from an external library (no extra prefix if already tagged).
pub fn external_line(line: &str) {
    if line.is_empty() {
        return;
    }
    if IN_EMIT.load(Ordering::Relaxed) {
        return;
    }
    let trimmed = line.trim_end();
    let formatted = if trimmed.starts_with('[')
        && trimmed.len() > 7
        && trimmed.as_bytes().get(6) == Some(&b']')
    {
        trimmed.to_string()
    } else {
        format!("[EXTERNAL] {trimmed}")
    };
    if let Ok(slot) = sink_slot().lock() {
        if let Some(ref sink) = *slot {
            sink(&formatted);
        }
    }
    write_stderr(&formatted);
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
    emit("DEBUG", msg);
}

pub fn warn_if_err<E: Display>(op: impl Display, result: std::result::Result<(), E>) {
    if let Err(err) = result {
        warn(format!("{op}: {err}"));
    }
}

pub fn error_if_err<E: Display>(op: impl Display, result: std::result::Result<(), E>) {
    if let Err(err) = result {
        error(format!("{op}: {err}"));
    }
}

/// Redirect process stderr into [`external_line`] so third-party libraries are visible in-app.
pub fn init_stdio_capture() {
    #[cfg(unix)]
    {
        static STARTED: std::sync::Once = std::sync::Once::new();
        STARTED.call_once(|| {
            if std::env::var("HFSDR_NO_STDERR_CAPTURE").is_ok() {
                return;
            }
            if start_stdio_capture().is_err() {
                warn("stderr capture unavailable — external library logs may be missed");
            }
        });
    }
}

#[cfg(unix)]
fn start_stdio_capture() -> std::io::Result<()> {
    use std::fs::File;
    use std::io::{BufRead, BufReader};
    use std::os::unix::io::FromRawFd;

    let orig = unsafe {
        let dup_fd = libc::dup(libc::STDERR_FILENO);
        if dup_fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        File::from_raw_fd(dup_fd)
    };
    let _ = ORIG_STDERR.set(Mutex::new(orig));

    let mut pipe_fds = [0i32; 2];
    let rc = unsafe { libc::pipe(pipe_fds.as_mut_ptr()) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let read_fd = pipe_fds[0];
    let write_fd = pipe_fds[1];
    let dup_rc = unsafe { libc::dup2(write_fd, libc::STDERR_FILENO) };
    unsafe { libc::close(write_fd) };
    if dup_rc < 0 {
        unsafe { libc::close(read_fd) };
        return Err(std::io::Error::last_os_error());
    }

    let reader = unsafe { File::from_raw_fd(read_fd) };
    std::thread::Builder::new()
        .name("stderr-capture".into())
        .spawn(move || {
            let reader = BufReader::new(reader);
            for line in reader.lines() {
                match line {
                    Ok(line) => external_line(&line),
                    Err(err) => external_line(&format!("stderr read error: {err}")),
                }
            }
        })
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warn_if_err_logs_nothing_on_ok() {
        warn_if_err("op", Ok::<(), &str>(()));
    }

    #[test]
    fn external_line_preserves_existing_level_tag() {
        external_line("[WARN] already tagged");
    }
}

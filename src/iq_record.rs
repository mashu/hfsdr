//! Gzip-compressed IQ capture format (`.hiq.gz`) for offline replay and tests.
//!
//! Layout: 32-byte header + gzip stream of interleaved little-endian `f32` I/Q pairs.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{sync_channel, RecvTimeoutError, SyncSender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use rtrb::{Consumer, Producer, RingBuffer};

use crate::source::Complex32;

pub const MAGIC: &[u8; 5] = b"HFSR\x01";
const HEADER_LEN: usize = 32;

/// Metadata stored in the file header.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IqCaptureMeta {
    pub sample_rate: u32,
    pub center_hz: f64,
    pub sample_count: u64,
}

impl IqCaptureMeta {
    pub fn duration_secs(&self) -> f64 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        self.sample_count as f64 / self.sample_rate as f64
    }
}

/// Read capture metadata without decompressing the payload.
pub fn read_meta(path: &Path) -> io::Result<IqCaptureMeta> {
    let mut file = File::open(path)?;
    read_meta_from(&mut file)
}

fn read_meta_from(file: &mut File) -> io::Result<IqCaptureMeta> {
    let mut hdr = [0u8; 32];
    file.read_exact(&mut hdr)?;
    parse_header(&hdr)
}

fn parse_header(hdr: &[u8; 32]) -> io::Result<IqCaptureMeta> {
    if &hdr[0..5] != MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not an hfsdr IQ capture (bad magic)",
        ));
    }
    if hdr[5] != 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported IQ capture version {}", hdr[5]),
        ));
    }
    Ok(IqCaptureMeta {
        sample_rate: u32::from_le_bytes(hdr[8..12].try_into().unwrap()),
        center_hz: f64::from_le_bytes(hdr[12..20].try_into().unwrap()),
        sample_count: u64::from_le_bytes(hdr[20..28].try_into().unwrap()),
    })
}

fn write_header(file: &mut File, meta: &IqCaptureMeta) -> io::Result<()> {
    let mut hdr = [0u8; 32];
    hdr[0..5].copy_from_slice(MAGIC);
    hdr[5] = 1;
    hdr[8..12].copy_from_slice(&meta.sample_rate.to_le_bytes());
    hdr[12..20].copy_from_slice(&meta.center_hz.to_le_bytes());
    hdr[20..28].copy_from_slice(&meta.sample_count.to_le_bytes());
    file.write_all(&hdr)?;
    file.sync_data()?;
    Ok(())
}

fn samples_to_bytes(samples: &[Complex32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 8);
    for s in samples {
        out.extend_from_slice(&s.re.to_le_bytes());
        out.extend_from_slice(&s.im.to_le_bytes());
    }
    out
}

fn bytes_to_samples(bytes: &[u8]) -> Vec<Complex32> {
    let mut out = Vec::with_capacity(bytes.len() / 8);
    for chunk in bytes.chunks_exact(8) {
        let re = f32::from_le_bytes(chunk[0..4].try_into().unwrap());
        let im = f32::from_le_bytes(chunk[4..8].try_into().unwrap());
        out.push(Complex32 { re, im });
    }
    out
}

enum RecMsg {
    Chunk(Vec<Complex32>),
    Finish,
}

/// Background IQ recorder — engine thread only sends chunks; writer compresses off-thread.
pub struct IqRecorder {
    tx: SyncSender<RecMsg>,
    join: Option<JoinHandle<io::Result<u64>>>,
    path: PathBuf,
}

impl IqRecorder {
    pub fn start(path: PathBuf, sample_rate: u32, center_hz: f64) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)?;
        let mut file = file;
        write_header(
            &mut file,
            &IqCaptureMeta {
                sample_rate,
                center_hz,
                sample_count: 0,
            },
        )?;
        let enc = GzEncoder::new(file, Compression::fast());
        let (tx, rx) = sync_channel::<RecMsg>(64);
        let path_thread = path.clone();
        let join = thread::Builder::new()
            .name("iq-record".into())
            .spawn(move || writer_thread(rx, enc, sample_rate, center_hz, path_thread))?;
        Ok(Self {
            tx,
            join: Some(join),
            path,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn push(&self, samples: &[Complex32]) {
        if samples.is_empty() {
            return;
        }
        let _ = self.tx.try_send(RecMsg::Chunk(samples.to_vec()));
    }

    pub fn stop(mut self) -> io::Result<IqCaptureMeta> {
        let _ = self.tx.send(RecMsg::Finish);
        let count = self
            .join
            .take()
            .expect("recorder join")
            .join()
            .map_err(|_| io::Error::other("recorder thread panicked"))??;
        read_meta(&self.path).map(|mut m| {
            m.sample_count = count;
            m
        })
    }
}

fn writer_thread(
    rx: std::sync::mpsc::Receiver<RecMsg>,
    mut enc: GzEncoder<File>,
    sample_rate: u32,
    center_hz: f64,
    _path: PathBuf,
) -> io::Result<u64> {
    let mut count = 0u64;
    loop {
        match rx.recv_timeout(Duration::from_secs(2)) {
            Ok(RecMsg::Chunk(chunk)) => {
                count += chunk.len() as u64;
                enc.write_all(&samples_to_bytes(&chunk))?;
            }
            Ok(RecMsg::Finish) => break,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    let mut file = enc.finish()?;
    write_header(
        &mut file,
        &IqCaptureMeta {
            sample_rate,
            center_hz,
            sample_count: count,
        },
    )?;
    Ok(count)
}

/// Real-time IQ playback into a ring buffer (engine drains like a live source).
pub struct IqPlayback {
    consumer: Consumer<Complex32>,
    meta: IqCaptureMeta,
    done: Arc<std::sync::atomic::AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl IqPlayback {
    pub fn open(path: impl Into<PathBuf>) -> io::Result<Self> {
        let path = path.into();
        let meta = read_meta(&path)?;
        let mut file = File::open(&path)?;
        file.seek(SeekFrom::Start(HEADER_LEN as u64))?;
        let (mut prod, cons) = RingBuffer::<Complex32>::new(65_536);
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let done_thread = Arc::clone(&done);
        let rate = meta.sample_rate.max(1);
        let join = thread::Builder::new()
            .name("iq-playback".into())
            .spawn(move || playback_thread(file, rate, &mut prod, done_thread))?;
        Ok(Self {
            consumer: cons,
            meta,
            done,
            join: Some(join),
        })
    }

    pub fn meta(&self) -> IqCaptureMeta {
        self.meta
    }

    pub fn finished(&self) -> bool {
        self.done.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn pop(&mut self) -> Option<Complex32> {
        self.consumer.pop().ok()
    }
}

impl Drop for IqPlayback {
    fn drop(&mut self) {
        self.done.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

fn playback_thread(
    file: File,
    sample_rate: u32,
    prod: &mut Producer<Complex32>,
    done: Arc<std::sync::atomic::AtomicBool>,
) {
    let mut dec = GzDecoder::new(file);
    let mut raw = [0u8; 8192];
    let mut pending = Vec::<u8>::new();
    let mut carry = 0.0f64;
    let rate = sample_rate as f64;

    while !done.load(std::sync::atomic::Ordering::Relaxed) {
        let n = match dec.read(&mut raw) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        pending.extend_from_slice(&raw[..n]);
        while pending.len() >= 8 {
            let chunk_bytes = pending.len().min(8192);
            let chunk_bytes = chunk_bytes - (chunk_bytes % 8);
            let samples = bytes_to_samples(&pending[..chunk_bytes]);
            pending.drain(..chunk_bytes);
            for s in samples {
                while prod.is_full() && !done.load(std::sync::atomic::Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(2));
                }
                if done.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }
                let _ = prod.push(s);
                carry += 1.0;
                if carry >= rate / 50.0 {
                    thread::sleep(Duration::from_micros((carry / rate * 1e6) as u64));
                    carry = 0.0;
                }
            }
        }
    }
    done.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Default directory for IQ captures.
pub fn default_capture_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("hfsdr")
        .join("captures")
}

pub fn timestamped_capture_path() -> PathBuf {
    timestamped_capture_path_in(default_capture_dir())
}

pub fn timestamped_capture_path_in(dir: impl AsRef<Path>) -> PathBuf {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    dir.as_ref().join(format!("capture-{now}.hiq.gz"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_capture() {
        let dir = std::env::temp_dir().join("hfsdr_iq_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test.hiq.gz");
        let samples: Vec<Complex32> = (0..4000)
            .map(|i| Complex32 {
                re: (i as f32 * 0.01).cos(),
                im: (i as f32 * 0.01).sin(),
            })
            .collect();
        let rec = IqRecorder::start(path.clone(), 12_000, 14_030_000.0).expect("rec");
        rec.push(&samples);
        let meta = rec.stop().expect("stop");
        assert_eq!(meta.sample_count, 4000);

        let mut pb = IqPlayback::open(path.clone()).expect("play");
        let mut got = Vec::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while got.len() < samples.len() && std::time::Instant::now() < deadline {
            while let Some(s) = pb.pop() {
                got.push(s);
            }
            if got.len() < samples.len() {
                thread::sleep(Duration::from_millis(10));
            }
        }
        assert_eq!(got.len(), samples.len());
        assert!((got[100].re - samples[100].re).abs() < 1e-4);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn header_parse() {
        let mut buf = [0u8; 32];
        buf[0..5].copy_from_slice(MAGIC);
        buf[5] = 1;
        buf[8..12].copy_from_slice(&12_000u32.to_le_bytes());
        buf[12..20].copy_from_slice(&14_030_000.0f64.to_le_bytes());
        buf[20..28].copy_from_slice(&99_000u64.to_le_bytes());
        let m = parse_header(&buf).expect("hdr");
        assert_eq!(m.sample_rate, 12_000);
        assert_eq!(m.sample_count, 99_000);
    }
}

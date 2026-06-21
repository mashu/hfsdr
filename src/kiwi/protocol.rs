//! KiwiSDR SND frame parsing and IQ sample extraction.

use crate::source::Complex32;
use rtrb::Producer;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};

/// Nominal Kiwi IQ sample rate (the server reports ~12001 Hz).
pub const KIWI_IQ_RATE: u32 = 12_000;

/// Default Kiwi IQ passband half-width (Hz) — full ~10 kHz panadapter view.
pub const KIWI_IQ_HALF_HZ: i32 = 5_000;

/// Pull `audio_rate=<n>` out of a Kiwi status line (float or integer Hz).
pub fn audio_rate(text: &str) -> Option<u32> {
    parse_kiwi_hz_param(text, "audio_rate=")
}

fn parse_kiwi_hz_param(text: &str, key: &str) -> Option<u32> {
    kiwi_msg_params(text)
        .split_whitespace()
        .find_map(|tok| {
            tok.strip_prefix(key).and_then(|v| {
                v.parse::<f32>()
                    .ok()
                    .map(|hz| hz.round().clamp(1.0, 2_000_000.0) as u32)
            })
        })
}

/// Text payload of a binary `MSG` WebSocket frame (`MSG` tag + 1 flag byte).
pub fn msg_body_text(buf: &[u8]) -> Option<&str> {
    if buf.len() < 5 || buf.get(0..3) != Some(b"MSG") {
        return None;
    }
    std::str::from_utf8(&buf[4..]).ok()
}

/// Strip the optional `MSG ` prefix Kiwi puts on status lines.
pub fn kiwi_msg_params(text: &str) -> &str {
    text.strip_prefix("MSG ").unwrap_or(text).trim()
}

/// Whether a Kiwi status line announces the IQ sample rate.
pub fn has_sample_rate(text: &str) -> bool {
    kiwi_msg_params(text).contains("sample_rate=")
}

/// Commands sent after the server reports `sample_rate=…` (kiwiclient handshake).
pub struct KiwiRxSetup {
    pub low_cut: i32,
    pub high_cut: i32,
    pub freq_hz: f64,
    pub agc_on: bool,
    pub compression: bool,
}

impl KiwiRxSetup {
    pub fn setup_commands(&self) -> Vec<String> {
        // Match kiwiclient order after `sample_rate=…`: squelch, gen, mod=iq, keepalive.
        vec![
            "SET squelch=0 max=0".to_string(),
            "SET genattn=0".to_string(),
            "SET gen=0 mix=-1".to_string(),
            mod_iq_command(self.low_cut, self.high_cut, self.freq_hz),
            format!(
                "SET agc={} hang=0 thresh=-100 slope=6 decay=1000 manGain=50",
                self.agc_on as u8
            ),
            format!("SET compression={}", self.compression as u8),
            "SET keepalive".to_string(),
        ]
    }
}

/// Build the `SET mod=iq` command for a given passband and center frequency.
pub fn mod_iq_command(low_cut: i32, high_cut: i32, freq_hz: f64) -> String {
    format!(
        "SET mod=iq low_cut={} high_cut={} freq={:.3}",
        low_cut,
        high_cut,
        freq_hz / 1000.0
    )
}

/// Parse one SND frame and push its IQ samples into the ring.
pub fn parse_snd(
    buf: &[u8],
    prod: &mut Producer<Complex32>,
    dropped: &AtomicU64,
    rssi: &AtomicI32,
) {
    if buf.len() < 10 || &buf[0..3] != b"SND" {
        return;
    }
    let smeter = u16::from_be_bytes([buf[8], buf[9]]);
    let rssi_dbm = 0.1 * smeter as f32 - 127.0;
    rssi.store((rssi_dbm * 100.0) as i32, Ordering::Relaxed);

    let payload = &buf[10..];
    if payload.len() < 10 {
        return;
    }
    push_iq_samples(&payload[10..], prod, dropped);
}

/// Decode big-endian int16 I/Q pairs and push into the ring in bulk.
pub fn push_iq_samples(iq: &[u8], prod: &mut Producer<Complex32>, dropped: &AtomicU64) {
    let pairs = iq.len() / 4;
    if pairs == 0 {
        return;
    }

    let avail = prod.slots();
    let to_write = pairs.min(avail);
    let drop_count = pairs - to_write;

    if to_write > 0 {
        if let Ok(mut chunk) = prod.write_chunk_uninit(to_write) {
            let (first, second) = chunk.as_mut_slices();
            for (pair_idx, slot) in first.iter_mut().chain(second.iter_mut()).enumerate() {
                let base = pair_idx * 4;
                let i = i16::from_be_bytes([iq[base], iq[base + 1]]) as f32 / 32768.0;
                let q = i16::from_be_bytes([iq[base + 2], iq[base + 3]]) as f32 / 32768.0;
                slot.write(Complex32::new(i, q));
            }
            // SAFETY: every slot in the chunk was initialized above.
            unsafe { chunk.commit_all() };
        }
    }
    if drop_count > 0 {
        dropped.fetch_add(drop_count as u64, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtrb::RingBuffer;

    fn make_snd_frame(iq_pairs: &[(i16, i16)]) -> Vec<u8> {
        let mut frame = Vec::new();
        frame.extend_from_slice(b"SND");
        frame.push(0); // flags
        frame.extend_from_slice(&0u32.to_le_bytes()); // sequence
        frame.extend_from_slice(&1000u16.to_be_bytes()); // S-meter
        frame.extend_from_slice(&[0u8; 10]); // GPS timestamp header
        for &(i, q) in iq_pairs {
            frame.extend_from_slice(&i.to_be_bytes());
            frame.extend_from_slice(&q.to_be_bytes());
        }
        frame
    }

    #[test]
    fn audio_rate_parses_server_msg() {
        assert_eq!(
            audio_rate("MSG audio_rate=12001 squelch=0"),
            Some(12001)
        );
        assert_eq!(
            audio_rate("audio_init=0 audio_rate=12000"),
            Some(12000)
        );
        assert_eq!(audio_rate("MSG squelch=0"), None);
    }

    #[test]
    fn msg_body_text_skips_flag_byte() {
        let frame = b"MSG\x00sample_rate=12000";
        assert_eq!(msg_body_text(frame), Some("sample_rate=12000"));
    }

    #[test]
    fn has_sample_rate_accepts_msg_prefix() {
        assert!(has_sample_rate("MSG sample_rate=12000"));
        assert!(has_sample_rate("sample_rate=12000"));
        assert!(!has_sample_rate("MSG squelch=0"));
    }

    #[test]
    fn audio_rate_parses_float_sample_rate() {
        assert_eq!(
            parse_kiwi_hz_param("sample_rate=11998.914787", "sample_rate="),
            Some(11_999)
        );
        assert_eq!(audio_rate("audio_rate=11998.914787"), Some(11_999));
    }

    #[test]
    fn rx_setup_includes_compression_off_by_default() {
        let setup = KiwiRxSetup {
            low_cut: -500,
            high_cut: 500,
            freq_hz: 7_030_000.0,
            agc_on: true,
            compression: false,
        };
        let cmds = setup.setup_commands();
        assert!(cmds.iter().any(|c| c.contains("mod=iq")));
        assert!(cmds.iter().any(|c| c == "SET compression=0"));
        assert!(cmds.iter().any(|c| c.starts_with("SET squelch")));
    }

    #[test]
    fn mod_iq_command_formats_passband() {
        let cmd = mod_iq_command(-3000, 3000, 7_030_000.0);
        assert!(cmd.contains("mod=iq"));
        assert!(cmd.contains("low_cut=-3000"));
        assert!(cmd.contains("high_cut=3000"));
        assert!(cmd.contains("freq=7030.000"));
    }

    #[test]
    fn parse_snd_decodes_iq_samples() {
        let frame = make_snd_frame(&[(16384, 0), (0, -16384)]);
        let (mut prod, mut cons) = RingBuffer::<Complex32>::new(8);
        let dropped = AtomicU64::new(0);
        let rssi = AtomicI32::new(0);

        parse_snd(&frame, &mut prod, &dropped, &rssi);

        let s0 = cons.pop().expect("sample 0");
        assert!((s0.re - 0.5).abs() < 0.01);
        assert!(s0.im.abs() < 0.01);

        let s1 = cons.pop().expect("sample 1");
        assert!(s1.re.abs() < 0.01);
        assert!((s1.im + 0.5).abs() < 0.01);
    }

    #[test]
    fn parse_snd_counts_drops_on_full_ring() {
        let frame = make_snd_frame(&[(100, 0), (200, 0), (300, 0)]);
        let (mut prod, _cons) = RingBuffer::<Complex32>::new(2);
        let dropped = AtomicU64::new(0);
        let rssi = AtomicI32::new(0);

        parse_snd(&frame, &mut prod, &dropped, &rssi);
        assert_eq!(dropped.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn parse_snd_ignores_short_buffers() {
        let (mut prod, mut cons) = RingBuffer::<Complex32>::new(4);
        let dropped = AtomicU64::new(0);
        let rssi = AtomicI32::new(0);

        parse_snd(b"SN", &mut prod, &dropped, &rssi);
        parse_snd(b"SND\x00\x00\x00\x00\x00", &mut prod, &dropped, &rssi);
        assert!(cons.pop().is_err());
    }
}

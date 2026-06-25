//! KiwiSDR SND frame parsing and IQ sample extraction.

use crate::source::Complex32;
use rtrb::Producer;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};

/// Nominal Kiwi IQ sample rate (the server reports ~12001 Hz).
pub const KIWI_IQ_RATE: u32 = 12_000;

/// Maximum IQ half-width (Hz) at `sample_rate` — matches kiwiclient / SDRangel (rate/2 − 20).
pub fn kiwi_iq_half_hz(sample_rate: u32) -> i32 {
    (sample_rate as i32 / 2).saturating_sub(20).max(1_000)
}

/// Default Kiwi IQ passband half-width at [`KIWI_IQ_RATE`].
pub const KIWI_IQ_HALF_HZ: i32 = 5_980;

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

/// Kiwi `manGain` at full manual gain (0 dB below the firmware headroom).
pub const KIWI_MAN_GAIN_MAX: u8 = 100;
/// Default for new sessions — max manual gain, like analog RF gain wide open (Yaesu-style).
pub const KIWI_MAN_GAIN_DEFAULT: u8 = KIWI_MAN_GAIN_MAX;

/// dB below full `manGain` (100 → 0 dB, 90 → −10 dB). Kiwi uses 1 wire step ≈ 1 dB.
pub fn man_gain_db_below_max(man_gain: u8) -> i32 {
    man_gain as i32 - i32::from(KIWI_MAN_GAIN_MAX)
}

/// Convert dB-below-max (−100..=0) to Kiwi `manGain` 0..=100.
pub fn man_gain_from_db_below_max(db: i32) -> u8 {
    (db + i32::from(KIWI_MAN_GAIN_MAX)).clamp(0, i32::from(KIWI_MAN_GAIN_MAX)) as u8
}

/// Linear IQ multiply factor from Kiwi CuteSDR `CAgc` when RF AGC is off.
///
/// Firmware: `m_ManualAgcGain = MAX_MANUAL_AMPLITUDE * 10^(-(100 - manGain)/20)`.
/// Normalized so `100` → `1.0` (0 dB below max).
pub fn kiwi_manual_agc_linear(man_gain: u8) -> f32 {
    let g = f32::from(man_gain.clamp(0, KIWI_MAN_GAIN_MAX));
    10f32.powf(-(f32::from(KIWI_MAN_GAIN_MAX) - g) / 20.0)
}

/// Build the Kiwi `SET agc=… manGain=…` command.
pub fn agc_command(agc_on: bool, man_gain: u8) -> String {
    format!(
        "SET agc={} hang=0 thresh=-100 slope=6 decay=1000 manGain={}",
        agc_on as u8,
        man_gain.clamp(0, 100)
    )
}

/// Test-signal generator attenuation (`SET genattn=`, used with `SET gen=`).
pub fn genattn_command(attn: u8) -> String {
    format!("SET genattn={}", attn)
}

/// Hardware RF attenuator on KiwiSDR 2 (`SET rf_attn=`, dB).
pub fn rf_attn_command(db: f32) -> String {
    format!("SET rf_attn={:.1}", db.clamp(0.0, KIWI_RF_ATTN_MAX_DB))
}

/// Maximum RF attenuator setting on KiwiSDR 2 (0.5 dB steps).
pub const KIWI_RF_ATTN_MAX_DB: f32 = 31.5;

fn parse_kiwi_scalar<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    kiwi_msg_params(text)
        .split_whitespace()
        .find_map(|tok| tok.strip_prefix(key))
}

/// Parse `has_attn=1` from a Kiwi status line.
pub fn has_rf_attn(text: &str) -> Option<bool> {
    parse_kiwi_scalar(text, "has_attn=")
        .and_then(|v| v.parse::<u8>().ok())
        .map(|n| n != 0)
}

/// Parse `rf_attn=12.0` from a Kiwi status line.
pub fn rf_attn_db(text: &str) -> Option<f32> {
    parse_kiwi_scalar(text, "rf_attn=")
        .and_then(|v| v.parse::<f32>().ok())
        .map(|db| db.clamp(0.0, KIWI_RF_ATTN_MAX_DB))
}

/// Commands sent after the server reports `sample_rate=…` (kiwiclient handshake).
pub struct KiwiRxSetup {
    pub low_cut: i32,
    pub high_cut: i32,
    pub freq_hz: f64,
    pub agc_on: bool,
    pub man_gain: u8,
    /// Test generator attenuation sent during IQ handshake (`SET genattn=`).
    pub gen_attn: u8,
    /// Requested hardware RF attenuator (dB) when `has_attn=1`.
    pub rf_attn_db: f32,
    pub compression: bool,
    pub ar_out_hz: u32,
}

impl KiwiRxSetup {
    pub fn setup_commands(&self) -> Vec<String> {
        // Match kiwiclient order after `sample_rate=…`: squelch, gen, mod=iq, keepalive.
        vec![
            "SET squelch=0 max=0".to_string(),
            genattn_command(self.gen_attn),
            "SET gen=0 mix=-1".to_string(),
            mod_iq_command(self.low_cut, self.high_cut, self.freq_hz),
            agc_command(self.agc_on, self.man_gain),
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

    #[test]
    fn kiwi_full_passband_half_width() {
        assert_eq!(kiwi_iq_half_hz(12_000), 5_980);
    }
}

#[cfg(test)]
mod parse_tests {
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
    fn man_gain_db_mapping() {
        assert_eq!(man_gain_db_below_max(100), 0);
        assert_eq!(man_gain_db_below_max(90), -10);
        assert_eq!(man_gain_db_below_max(50), -50);
        assert_eq!(man_gain_from_db_below_max(0), 100);
        assert_eq!(man_gain_from_db_below_max(-10), 90);
        assert_eq!(KIWI_MAN_GAIN_DEFAULT, KIWI_MAN_GAIN_MAX);
    }

    #[test]
    fn man_gain_db_roundtrip_and_clamp() {
        for mg in 0..=100u8 {
            let db = man_gain_db_below_max(mg);
            assert_eq!(man_gain_from_db_below_max(db), mg);
        }
        assert_eq!(man_gain_from_db_below_max(-200), 0);
        assert_eq!(man_gain_from_db_below_max(50), 100);
    }

    #[test]
    fn kiwi_manual_agc_matches_firmware_curve() {
        assert!((kiwi_manual_agc_linear(100) - 1.0).abs() < 1e-6);
        assert!((kiwi_manual_agc_linear(90) - 10f32.powf(-0.5)).abs() < 1e-5);
        assert!((kiwi_manual_agc_linear(50) - 10f32.powf(-2.5)).abs() < 1e-5);
        // Last 10 steps ≈ last 10 dB — explains why only 90–100 feels active on noise.
        let top_decade = kiwi_manual_agc_linear(100) / kiwi_manual_agc_linear(90);
        let mid_to_top = kiwi_manual_agc_linear(90) / kiwi_manual_agc_linear(50);
        assert!((top_decade - 10f32.powf(0.5)).abs() < 0.02);
        assert!(mid_to_top > 20.0, "50→90 spans most of the dynamic range: {mid_to_top}");
    }

    #[test]
    fn agc_command_manual_vs_automatic() {
        assert_eq!(
            agc_command(false, 100),
            "SET agc=0 hang=0 thresh=-100 slope=6 decay=1000 manGain=100"
        );
        assert_eq!(
            agc_command(true, 100),
            "SET agc=1 hang=0 thresh=-100 slope=6 decay=1000 manGain=100"
        );
        assert!(agc_command(true, 200).ends_with("manGain=100"));
    }

    #[test]
    fn rx_setup_includes_compression_off_by_default() {
        let setup = KiwiRxSetup {
            low_cut: -500,
            high_cut: 500,
            freq_hz: 7_030_000.0,
            agc_on: true,
            man_gain: 50,
            gen_attn: 0,
            rf_attn_db: 0.0,
            compression: false,
            ar_out_hz: 44_100,
        };
        let cmds = setup.setup_commands();
        assert!(cmds.iter().any(|c| c.contains("mod=iq")));
        assert!(cmds.iter().any(|c| c == "SET compression=0"));
        assert!(cmds.iter().any(|c| c.contains("manGain=50")));
        assert!(cmds.iter().any(|c| c == "SET genattn=0"));
        assert!(cmds.iter().any(|c| c.starts_with("SET squelch")));
    }

    #[test]
    fn genattn_and_rf_attn_commands() {
        assert_eq!(genattn_command(12), "SET genattn=12");
        assert_eq!(rf_attn_command(6.0), "SET rf_attn=6.0");
        assert_eq!(rf_attn_command(99.0), "SET rf_attn=31.5");
    }

    #[test]
    fn parses_has_attn_and_rf_attn_from_status() {
        assert_eq!(has_rf_attn("MSG has_attn=1 foo=bar"), Some(true));
        assert_eq!(has_rf_attn("has_attn=0"), Some(false));
        assert_eq!(rf_attn_db("MSG rf_attn=12.5"), Some(12.5));
        assert!(has_rf_attn("MSG squelch=0").is_none());
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

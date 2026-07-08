//! End-to-end CW skimmer accuracy tests over synthetic IQ.
//!
//! Exercises the whole receive path the way the app runs it — IQ chunks into
//! [`Skimmer::process`] alongside a peak-hold spectrum — across keying speed,
//! signal strength, QSB fading, adjacent interferers 10 Hz away, a crowded
//! band, and pure noise (which must produce no spots).

use std::f32::consts::TAU;

use hfsdr::{
    encode_char, Complex32, MasterScp, Skimmer, SkimmerConfig, SkimmerDecoderKind,
    SpectrumAnalyzer, SpotSort,
};

const RATE: f32 = 12_000.0;
const SAMPLE_SCP: &str =
    "VER20260202\nW1AW\nOH2BH\nK3LR\nSM5DAJ\nDL1QQ\nG4ABC\nVE3NEA\nN2IC\n";

/// Deterministic gaussian noise (LCG + Box-Muller), no external crates.
struct NoiseGen {
    state: u64,
}

impl NoiseGen {
    fn new(seed: u64) -> Self {
        Self {
            state: seed.wrapping_mul(2862933555777941757).wrapping_add(1),
        }
    }

    fn uniform(&mut self) -> f32 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.state >> 33) as f32 / (1u64 << 31) as f32).clamp(1e-7, 1.0)
    }

    fn gaussian_pair(&mut self) -> (f32, f32) {
        let r = (-2.0 * self.uniform().ln()).sqrt();
        let th = TAU * self.uniform();
        (r * th.cos(), r * th.sin())
    }
}

/// One keyed CW transmitter.
#[derive(Clone)]
struct Station {
    text: &'static str,
    wpm: f32,
    offset_hz: f32,
    amplitude: f32,
    /// QSB: fractional depth (0 = none) and fade period in seconds.
    qsb_depth: f32,
    qsb_period_s: f32,
    /// Delay before keying starts, in dits.
    start_delay_dits: f32,
}

impl Station {
    fn new(text: &'static str, wpm: f32, offset_hz: f32, amplitude: f32) -> Self {
        Self {
            text,
            wpm,
            offset_hz,
            amplitude,
            qsb_depth: 0.0,
            qsb_period_s: 5.0,
            start_delay_dits: 6.0,
        }
    }

    /// Key-down envelope (0/1 with raised-cosine edges) sampled at `RATE`.
    fn keying(&self) -> Vec<f32> {
        let dit = (1.2 / self.wpm * RATE).round() as usize;
        let edge = ((0.004 * RATE) as usize).min(dit / 3).max(1);
        let mut gate: Vec<bool> = Vec::new();
        let push = |on: bool, dits: usize, gate: &mut Vec<bool>| {
            gate.extend(std::iter::repeat_n(on, dits * dit));
        };
        gate.extend(std::iter::repeat_n(
            false,
            (self.start_delay_dits * dit as f32) as usize,
        ));
        for (wi, word) in self.text.split(' ').enumerate() {
            if wi > 0 {
                push(false, 7, &mut gate);
            }
            for (ci, ch) in word.chars().enumerate() {
                if ci > 0 {
                    push(false, 3, &mut gate);
                }
                for (ei, el) in encode_char(ch).unwrap_or("").chars().enumerate() {
                    if ei > 0 {
                        push(false, 1, &mut gate);
                    }
                    push(true, if el == '-' { 3 } else { 1 }, &mut gate);
                }
            }
        }
        push(false, 10, &mut gate);

        // Raised-cosine shaping over the boolean gate.
        let mut env = vec![0.0f32; gate.len()];
        let mut level = 0.0f32;
        let step = 1.0 / edge as f32;
        for (i, &on) in gate.iter().enumerate() {
            let target = if on { 1.0 } else { 0.0 };
            if level < target {
                level = (level + step).min(1.0);
            } else if level > target {
                level = (level - step).max(0.0);
            }
            // Raised-cosine profile from the linear ramp.
            env[i] = 0.5 - 0.5 * (std::f32::consts::PI * level).cos();
        }
        env
    }
}

/// Sum stations plus AWGN into a complex IQ buffer.
fn synth_iq(stations: &[Station], seconds: f32, noise_rms: f32, seed: u64) -> Vec<Complex32> {
    let n = (seconds * RATE) as usize;
    let mut out = vec![Complex32 { re: 0.0, im: 0.0 }; n];
    for st in stations {
        let keying = st.keying();
        let mut phase = 0.0f32;
        for (i, slot) in out.iter_mut().enumerate() {
            phase += TAU * st.offset_hz / RATE;
            if phase > TAU {
                phase -= TAU;
            }
            let key = keying.get(i % keying.len().max(1)).copied().unwrap_or(0.0);
            let t = i as f32 / RATE;
            let fade = if st.qsb_depth > 0.0 {
                1.0 - st.qsb_depth * (0.5 + 0.5 * (TAU * t / st.qsb_period_s).sin())
            } else {
                1.0
            };
            let a = st.amplitude * key * fade;
            let (s, c) = phase.sin_cos();
            slot.re += a * c;
            slot.im += a * s;
        }
    }
    if noise_rms > 0.0 {
        let mut rng = NoiseGen::new(seed);
        let sigma = noise_rms / 2.0f32.sqrt();
        for slot in &mut out {
            let (a, b) = rng.gaussian_pair();
            slot.re += sigma * a;
            slot.im += sigma * b;
        }
    }
    out
}

fn config_for(decoder: SkimmerDecoderKind) -> SkimmerConfig {
    SkimmerConfig {
        decoder,
        require_scp: true,
        min_snr_db: 10.0,
        min_decode_snr_db: 8.0,
        max_channels: 12,
        focus_span_hz: 0.0, // whole band — focus behaviour has its own test
        ..SkimmerConfig::default()
    }
}

fn test_config() -> SkimmerConfig {
    config_for(SkimmerDecoderKind::Bigram)
}

/// Run IQ through the skimmer the way the engine does: peak-hold spectrum
/// updated per chunk, then `process`.
fn run_skimmer(iq: &[Complex32], config: SkimmerConfig, scp: &str) -> Skimmer {
    let mut skimmer = Skimmer::new(config);
    skimmer.replace_scp(MasterScp::from_text(scp));
    let mut analyzer = SpectrumAnalyzer::new(2048, 1024);
    let mut peak_hold = vec![-140.0f32; 2048];
    for chunk in iq.chunks(2048) {
        analyzer.process(chunk, |row| {
            for (hold, &v) in peak_hold.iter_mut().zip(row.iter()) {
                *hold = hold.max(v);
            }
        });
        skimmer.process(chunk, RATE, &peak_hold, RATE, 0.0, 7_030_000.0);
    }
    skimmer
}

fn spotted_calls(skimmer: &Skimmer) -> Vec<String> {
    skimmer
        .store()
        .sorted(SpotSort::SnrDesc)
        .into_iter()
        .filter_map(|s| s.callsign)
        .collect()
}

fn assert_spotted(skimmer: &Skimmer, call: &str, label: &str) {
    let calls = spotted_calls(skimmer);
    assert!(
        calls.iter().any(|c| c == call),
        "{label}: expected {call}, got spots {calls:?}, channels {:?}",
        skimmer.debug_channels()
    );
}

#[test]
fn decodes_clean_signal_across_speeds() {
    for wpm in [12.0, 15.0, 20.0, 25.0, 32.0, 40.0, 45.0] {
        let st = Station::new("CQ CQ DE W1AW W1AW K", wpm, 700.0, 1.0);
        let iq = synth_iq(&[st], 40.0, 0.05, 7);
        let skimmer = run_skimmer(&iq, test_config(), SAMPLE_SCP);
        assert_spotted(&skimmer, "W1AW", &format!("{wpm} wpm"));
    }
}

#[test]
fn decodes_weak_station_in_noise() {
    // Channel-bandwidth SNR ≈ 7–8 dB — weak but copyable.
    let st = Station::new("CQ CQ DE OH2BH OH2BH K", 24.0, 500.0, 1.0);
    let iq = synth_iq(&[st], 45.0, 3.2, 21);
    let skimmer = run_skimmer(&iq, test_config(), SAMPLE_SCP);
    assert_spotted(&skimmer, "OH2BH", "weak station");
}

#[test]
fn decodes_through_qsb_fading() {
    let mut st = Station::new("CQ CQ DE SM5DAJ SM5DAJ K", 22.0, 600.0, 1.0);
    st.qsb_depth = 0.92;
    st.qsb_period_s = 5.0;
    let iq = synth_iq(&[st], 55.0, 0.4, 3);
    let skimmer = run_skimmer(&iq, test_config(), SAMPLE_SCP);
    assert_spotted(&skimmer, "SM5DAJ", "qsb");
}

#[test]
fn decodes_across_signal_strength_steps() {
    // Same sender at strong, weak, then medium level (repeats of the message).
    let mut iq = Vec::new();
    for amp in [1.0, 0.28, 0.6] {
        let st = Station::new("CQ CQ DE K3LR K3LR K", 26.0, 400.0, amp);
        iq.extend(synth_iq(&[st], 22.0, 0.35, 11));
    }
    let skimmer = run_skimmer(&iq, test_config(), SAMPLE_SCP);
    assert_spotted(&skimmer, "K3LR", "strength steps");
}

#[test]
fn decodes_wanted_station_with_interferer_10hz_away() {
    let wanted = Station::new("CQ CQ DE VE3NEA VE3NEA K", 25.0, 800.0, 1.0);
    let mut interferer = Station::new("TEST TEST TEST DE N2IC N2IC", 19.0, 810.0, 0.32);
    interferer.start_delay_dits = 11.0;
    let iq = synth_iq(&[wanted, interferer], 45.0, 0.25, 17);
    let skimmer = run_skimmer(&iq, test_config(), SAMPLE_SCP);
    assert_spotted(&skimmer, "VE3NEA", "10 Hz interferer");
}

#[test]
fn decodes_crowded_band_without_false_calls() {
    let stations = vec![
        Station::new("CQ CQ DE W1AW W1AW K", 18.0, -1_500.0, 0.9),
        Station::new("CQ CQ DE OH2BH OH2BH K", 24.0, -600.0, 0.7),
        Station::new("CQ CQ DE K3LR K3LR K", 30.0, 300.0, 1.0),
        Station::new("CQ CQ DE SM5DAJ SM5DAJ K", 21.0, 1_100.0, 0.55),
        Station::new("CQ CQ DE DL1QQ DL1QQ K", 27.0, 2_000.0, 0.8),
    ];
    let expected = ["W1AW", "OH2BH", "K3LR", "SM5DAJ", "DL1QQ"];
    let iq = synth_iq(&stations, 50.0, 0.3, 5);
    let skimmer = run_skimmer(&iq, test_config(), SAMPLE_SCP);
    let calls = spotted_calls(&skimmer);
    let hits = expected.iter().filter(|e| calls.iter().any(|c| c == *e)).count();
    assert!(
        hits >= 4,
        "crowded band: only {hits}/5 decoded, spots {calls:?}, channels {:?}",
        skimmer.debug_channels()
    );
    for c in &calls {
        assert!(
            expected.contains(&c.as_str()),
            "false callsign {c} spotted; all spots {calls:?}"
        );
    }
}

#[test]
fn decode_focus_limits_channels_to_tuned_span() {
    let stations = vec![
        Station::new("CQ CQ DE W1AW W1AW K", 24.0, 400.0, 1.0),
        Station::new("CQ CQ DE OH2BH OH2BH K", 24.0, 2_500.0, 1.0),
    ];
    let iq = synth_iq(&stations, 40.0, 0.2, 9);
    let mut config = test_config();
    config.focus_span_hz = 3_000.0; // ±1.5 kHz around the tuned frequency
    config.focus_center_hz = 0.0;
    let skimmer = run_skimmer(&iq, config, SAMPLE_SCP);
    let calls = spotted_calls(&skimmer);
    assert!(calls.iter().any(|c| c == "W1AW"), "in-focus station missed: {calls:?}");
    assert!(
        !calls.iter().any(|c| c == "OH2BH"),
        "out-of-focus station decoded: {calls:?}"
    );
    for (off, _, _) in skimmer.debug_channels() {
        assert!(
            off.abs() <= 1_600.0,
            "channel outside decode focus at {off} Hz"
        );
    }
}

// --- Bayes decoder: same end-to-end scenarios plus harder stress cases ---

#[test]
fn bayes_decodes_clean_signal_across_speeds() {
    for wpm in [12.0, 15.0, 20.0, 25.0, 32.0, 40.0, 45.0] {
        let st = Station::new("CQ CQ DE W1AW W1AW K", wpm, 700.0, 1.0);
        let iq = synth_iq(&[st], 40.0, 0.05, 7);
        let skimmer = run_skimmer(&iq, config_for(SkimmerDecoderKind::Bayes), SAMPLE_SCP);
        assert_spotted(&skimmer, "W1AW", &format!("bayes {wpm} wpm"));
    }
}

#[test]
fn bayes_decodes_weak_station_in_noise() {
    let st = Station::new("CQ CQ DE OH2BH OH2BH K", 24.0, 500.0, 1.0);
    let iq = synth_iq(&[st], 45.0, 3.2, 21);
    let skimmer = run_skimmer(&iq, config_for(SkimmerDecoderKind::Bayes), SAMPLE_SCP);
    assert_spotted(&skimmer, "OH2BH", "bayes weak station");
}

#[test]
fn bayes_decodes_through_qsb_fading() {
    let mut st = Station::new("CQ CQ DE SM5DAJ SM5DAJ K", 22.0, 600.0, 1.0);
    st.qsb_depth = 0.92;
    st.qsb_period_s = 5.0;
    let iq = synth_iq(&[st], 55.0, 0.4, 3);
    let skimmer = run_skimmer(&iq, config_for(SkimmerDecoderKind::Bayes), SAMPLE_SCP);
    assert_spotted(&skimmer, "SM5DAJ", "bayes qsb");
}

#[test]
fn bayes_decodes_wanted_station_with_interferer_10hz_away() {
    let wanted = Station::new("CQ CQ DE VE3NEA VE3NEA K", 25.0, 800.0, 1.0);
    let mut interferer = Station::new("TEST TEST TEST DE N2IC N2IC", 19.0, 810.0, 0.32);
    interferer.start_delay_dits = 11.0;
    let iq = synth_iq(&[wanted, interferer], 45.0, 0.25, 17);
    let skimmer = run_skimmer(&iq, config_for(SkimmerDecoderKind::Bayes), SAMPLE_SCP);
    assert_spotted(&skimmer, "VE3NEA", "bayes 10 Hz interferer");
}

#[test]
fn bayes_decodes_crowded_band_without_false_calls() {
    let stations = vec![
        Station::new("CQ CQ DE W1AW W1AW K", 18.0, -1_500.0, 0.9),
        Station::new("CQ CQ DE OH2BH OH2BH K", 24.0, -600.0, 0.7),
        Station::new("CQ CQ DE K3LR K3LR K", 30.0, 300.0, 1.0),
        Station::new("CQ CQ DE SM5DAJ SM5DAJ K", 21.0, 1_100.0, 0.55),
        Station::new("CQ CQ DE DL1QQ DL1QQ K", 27.0, 2_000.0, 0.8),
    ];
    let expected = ["W1AW", "OH2BH", "K3LR", "SM5DAJ", "DL1QQ"];
    let iq = synth_iq(&stations, 50.0, 0.3, 5);
    let skimmer = run_skimmer(&iq, config_for(SkimmerDecoderKind::Bayes), SAMPLE_SCP);
    let calls = spotted_calls(&skimmer);
    let hits = expected.iter().filter(|e| calls.iter().any(|c| c == *e)).count();
    assert!(
        hits >= 4,
        "bayes crowded band: only {hits}/5 decoded, spots {calls:?}, channels {:?}",
        skimmer.debug_channels()
    );
    for c in &calls {
        assert!(
            expected.contains(&c.as_str()),
            "bayes false callsign {c} spotted; all spots {calls:?}"
        );
    }
}

#[test]
fn bayes_decodes_speed_change_mid_transmission() {
    // Same channel, next sender QRQs from 16 to 30 WPM — the timing model
    // must re-lock without an operator hint. Spots are checked per over
    // because the store keeps one spot per frequency.
    let slow = synth_iq(
        &[Station::new("CQ CQ DE K3LR K3LR K", 16.0, 450.0, 1.0)],
        30.0,
        0.3,
        13,
    );
    let fast = synth_iq(
        &[Station::new("CQ CQ DE OH2BH OH2BH K", 30.0, 450.0, 1.0)],
        22.0,
        0.3,
        14,
    );
    let mut skimmer = Skimmer::new(config_for(SkimmerDecoderKind::Bayes));
    skimmer.replace_scp(MasterScp::from_text(SAMPLE_SCP));
    let mut analyzer = SpectrumAnalyzer::new(2048, 1024);
    let mut peak_hold = vec![-140.0f32; 2048];
    let mut feed = |skimmer: &mut Skimmer, iq: &[Complex32]| {
        for chunk in iq.chunks(2048) {
            analyzer.process(chunk, |row| {
                for (hold, &v) in peak_hold.iter_mut().zip(row.iter()) {
                    *hold = hold.max(v);
                }
            });
            skimmer.process(chunk, RATE, &peak_hold, RATE, 0.0, 7_030_000.0);
        }
    };
    feed(&mut skimmer, &slow);
    assert_spotted(&skimmer, "K3LR", "bayes slow over");
    feed(&mut skimmer, &fast);
    assert_spotted(&skimmer, "OH2BH", "bayes fast over");
}

/// Diagnostic sweep: weakest copyable signal per decoder.
/// `cargo test --test cw_accuracy weak_signal_margin -- --ignored --nocapture`
#[test]
#[ignore]
fn debug_weak_signal_margin() {
    for kind in [SkimmerDecoderKind::Bigram, SkimmerDecoderKind::Bayes] {
        for noise in [4.4f32, 5.0, 5.6, 6.2, 6.8] {
            let mut hits = 0;
            for seed in [21u64, 22, 23] {
                let st = Station::new("CQ CQ DE OH2BH OH2BH K", 24.0, 500.0, 1.0);
                let iq = synth_iq(&[st], 45.0, noise, seed);
                let mut config = config_for(kind);
                config.min_snr_db = 3.0;
                config.min_decode_snr_db = 3.0;
                let skimmer = run_skimmer(&iq, config, SAMPLE_SCP);
                if spotted_calls(&skimmer).iter().any(|c| c == "OH2BH") {
                    hits += 1;
                }
            }
            eprintln!("{kind:?} noise={noise}: {hits}/3 decoded");
        }
        for wpm in [48.0f32, 52.0, 56.0] {
            let mut hits = 0;
            for seed in [7u64, 8, 9] {
                let st = Station::new("CQ CQ DE W1AW W1AW K", wpm, 700.0, 1.0);
                let iq = synth_iq(&[st], 40.0, 0.2, seed);
                let skimmer = run_skimmer(&iq, config_for(kind), SAMPLE_SCP);
                if spotted_calls(&skimmer).iter().any(|c| c == "W1AW") {
                    hits += 1;
                }
            }
            eprintln!("{kind:?} qrq wpm={wpm}: {hits}/3 decoded");
        }
        for (depth, period) in [(0.95f32, 5.0f32), (0.95, 2.5), (0.98, 3.5)] {
            let mut hits = 0;
            for seed in [3u64, 4, 5] {
                let mut st = Station::new("CQ CQ DE SM5DAJ SM5DAJ K", 22.0, 600.0, 1.0);
                st.qsb_depth = depth;
                st.qsb_period_s = period;
                let iq = synth_iq(&[st], 55.0, 0.4, seed);
                let skimmer = run_skimmer(&iq, config_for(kind), SAMPLE_SCP);
                if spotted_calls(&skimmer).iter().any(|c| c == "SM5DAJ") {
                    hits += 1;
                }
            }
            eprintln!("{kind:?} qsb depth={depth} period={period}s: {hits}/3 decoded");
        }
    }
}

/// Diagnostic replay of the slow over: watch channel text grow second by second.
#[test]
#[ignore]
fn debug_bayes_slow_over() {
    let iq = synth_iq(
        &[Station::new("CQ CQ DE K3LR K3LR K", 16.0, 450.0, 1.0)],
        30.0,
        0.3,
        13,
    );
    let mut skimmer = Skimmer::new(config_for(SkimmerDecoderKind::Bayes));
    skimmer.replace_scp(MasterScp::from_text(SAMPLE_SCP));
    let mut analyzer = SpectrumAnalyzer::new(2048, 1024);
    let mut peak_hold = vec![-140.0f32; 2048];
    let mut t = 0.0f32;
    let mut next_dump = 2.0f32;
    for chunk in iq.chunks(2048) {
        analyzer.process(chunk, |row| {
            for (hold, &v) in peak_hold.iter_mut().zip(row.iter()) {
                *hold = hold.max(v);
            }
        });
        skimmer.process(chunk, RATE, &peak_hold, RATE, 0.0, 7_030_000.0);
        t += chunk.len() as f32 / RATE;
        if t >= next_dump {
            next_dump += 2.0;
            let near: Vec<_> = skimmer
                .debug_channels()
                .into_iter()
                .filter(|(off, _, _)| (*off - 450.0).abs() < 120.0)
                .collect();
            eprintln!("t={t:5.1}s near-450 channels: {near:?}");
        }
    }
    eprintln!("spots: {:?}", spotted_calls(&skimmer));
    eprintln!("all channels: {:?}", skimmer.debug_channels());
}

#[test]
fn bayes_pure_noise_produces_no_spots() {
    let iq = synth_iq(&[], 40.0, 1.0, 42);
    let mut config = config_for(SkimmerDecoderKind::Bayes);
    config.require_scp = false;
    config.min_snr_db = 6.0;
    config.min_decode_snr_db = 6.0;
    let skimmer = run_skimmer(&iq, config, SAMPLE_SCP);
    let spots = skimmer.store().sorted(SpotSort::SnrDesc);
    assert!(
        spots.is_empty(),
        "bayes: noise produced spots: {:?}, channels {:?}",
        spots
            .iter()
            .map(|s| (s.callsign.clone(), s.kind))
            .collect::<Vec<_>>(),
        skimmer.debug_channels()
    );
}

#[test]
fn bayes_noise_bursts_produce_no_spots() {
    let mut iq = synth_iq(&[], 30.0, 0.8, 99);
    let mut rng = NoiseGen::new(123);
    let burst_len = (0.030 * RATE) as usize;
    for k in 0..60 {
        let start = (k * iq.len() / 60).min(iq.len() - burst_len);
        for slot in &mut iq[start..start + burst_len] {
            let (a, b) = rng.gaussian_pair();
            slot.re += 4.0 * a;
            slot.im += 4.0 * b;
        }
    }
    let mut config = config_for(SkimmerDecoderKind::Bayes);
    config.require_scp = false;
    config.min_snr_db = 6.0;
    config.min_decode_snr_db = 6.0;
    let skimmer = run_skimmer(&iq, config, SAMPLE_SCP);
    let calls = spotted_calls(&skimmer);
    assert!(
        calls.is_empty(),
        "bayes: noise bursts produced callsign spots: {calls:?}"
    );
}

#[test]
fn pure_noise_produces_no_spots() {
    let iq = synth_iq(&[], 40.0, 1.0, 42);
    // Worst case for false positives: no SCP gate, heuristic validation only.
    let mut config = test_config();
    config.require_scp = false;
    config.min_snr_db = 6.0;
    config.min_decode_snr_db = 6.0;
    let mut skimmer = Skimmer::new(config);
    let mut analyzer = SpectrumAnalyzer::new(2048, 1024);
    let mut peak_hold = vec![-140.0f32; 2048];
    for chunk in iq.chunks(2048) {
        analyzer.process(chunk, |row| {
            for (hold, &v) in peak_hold.iter_mut().zip(row.iter()) {
                *hold = hold.max(v);
            }
        });
        skimmer.process(chunk, RATE, &peak_hold, RATE, 0.0, 7_030_000.0);
    }
    let spots = skimmer.store().sorted(SpotSort::SnrDesc);
    assert!(
        spots.is_empty(),
        "noise produced spots: {:?}, channels {:?}",
        spots
            .iter()
            .map(|s| (s.callsign.clone(), s.kind))
            .collect::<Vec<_>>(),
        skimmer.debug_channels()
    );
}

#[test]
fn noise_bursts_produce_no_spots() {
    // Static crashes: strong wideband impulses over noise, no CW present.
    let mut iq = synth_iq(&[], 30.0, 0.8, 99);
    let mut rng = NoiseGen::new(123);
    let burst_len = (0.030 * RATE) as usize;
    for k in 0..60 {
        let start = (k * iq.len() / 60).min(iq.len() - burst_len);
        for slot in &mut iq[start..start + burst_len] {
            let (a, b) = rng.gaussian_pair();
            slot.re += 4.0 * a;
            slot.im += 4.0 * b;
        }
    }
    let mut config = test_config();
    config.require_scp = false;
    config.min_snr_db = 6.0;
    config.min_decode_snr_db = 6.0;
    let skimmer = run_skimmer(&iq, config, SAMPLE_SCP);
    let calls = spotted_calls(&skimmer);
    assert!(
        calls.is_empty(),
        "noise bursts produced callsign spots: {calls:?}"
    );
}

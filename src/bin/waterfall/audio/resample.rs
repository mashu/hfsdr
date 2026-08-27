//! Clock-matching resampler shared by every audio backend.
//!
//! Demod audio arrives on the SDR's clock and leaves on the sound card's (or
//! the browser's). Those clocks are never the same, even when the nominal rates
//! match, so this is not a format conversion that can be skipped when the
//! numbers agree — it is the thing that stops the queue drifting into an
//! overflow (a click) or an underrun (a gap) over minutes of listening.
//!
//! The desktop backend writes into an `rtrb` ring; the browser backend writes
//! into a block that is posted to an audio worklet. Only the destination
//! differs, so only [`SampleSink`] does.

/// Somewhere resampled samples go.
pub(crate) trait SampleSink {
    /// No space for another sample; the caller stops and carries its phase.
    fn is_full(&self) -> bool;
    fn push_sample(&mut self, sample: f32);
}

/// ~200 ms at 48 kHz — enough jitter headroom without desyncing from the waterfall.
pub(crate) const RING_CAPACITY: usize = 9_600;

/// Fraction of the queue currently occupied, smoothed toward `fill_avg`.
pub(crate) fn smooth_fill(fill_avg: f32, occupancy: f32) -> f32 {
    fill_avg + 0.02 * (occupancy - fill_avg)
}

/// Resample step, with a slow servo pulling occupancy toward half full.
///
/// The trim is capped at ±0.3 %, far below the pitch change anyone can hear but
/// far above the tens of ppm that separate two free-running clocks.
pub(crate) fn servo_step(source_rate: u32, output_rate: u32, fill_avg: f32) -> f64 {
    let trim = ((fill_avg - 0.5) * 0.01).clamp(-0.003, 0.003);
    source_rate as f64 / output_rate.max(1) as f64 * (1.0 + trim as f64)
}

/// Linear-interpolation resampler with phase carried across blocks.
///
/// `pos` is the fractional read position into `mono` (may sit in [-1, 0)
/// pointing at `last`, the final sample of the previous block). Returns the
/// carried `(pos, last)` for the next call.
pub(crate) fn resample_push<S: SampleSink>(
    sink: &mut S,
    mono: &[f32],
    step: f64,
    mut pos: f64,
    last: f32,
    volume: f32,
) -> (f64, f32) {
    let n = mono.len();
    let limit = n as f64 - 1.0;
    while pos < limit {
        if sink.is_full() {
            break;
        }
        let i = pos.floor();
        let frac = (pos - i) as f32;
        let (a, b) = if i < 0.0 {
            (last, mono[0])
        } else {
            let idx = i as usize;
            (mono[idx], mono[idx + 1])
        };
        sink.push_sample((a + (b - a) * frac) * volume);
        pos += step;
    }
    // Lands in [-1, 0) when the block was fully consumed; clamp after an
    // overflow break (the dropped tail is a discontinuity either way).
    ((pos - n as f64).max(-1.0), mono[n - 1])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct VecSink {
        out: Vec<f32>,
        cap: usize,
    }

    impl VecSink {
        fn new(cap: usize) -> Self {
            Self {
                out: Vec::new(),
                cap,
            }
        }
    }

    impl SampleSink for VecSink {
        fn is_full(&self) -> bool {
            self.out.len() >= self.cap
        }
        fn push_sample(&mut self, s: f32) {
            self.out.push(s);
        }
    }

    /// At equal rates the resampler must be sample-accurate, not merely close:
    /// it runs on every source, so any per-sample error is always present.
    #[test]
    fn equal_rates_are_sample_accurate() {
        let src: Vec<f32> = (0..64).map(|i| (i as f32) * 0.01).collect();
        let mut sink = VecSink::new(1024);
        resample_push(&mut sink, &src, 1.0, 0.0, 0.0, 1.0);
        for (i, got) in sink.out.iter().enumerate() {
            assert!(
                (got - src[i]).abs() < 1e-6,
                "sample {i}: {got} != {}",
                src[i]
            );
        }
    }

    /// Phase carries across blocks, so a ramp split into two pushes must come
    /// out as one continuous ramp with no repeated or skipped sample at the
    /// seam — that seam is audible as a click once per block otherwise.
    #[test]
    fn phase_is_continuous_across_block_boundaries() {
        let all: Vec<f32> = (0..128).map(|i| i as f32).collect();
        let step = 0.5;

        let mut whole = VecSink::new(4096);
        resample_push(&mut whole, &all, step, 0.0, 0.0, 1.0);

        let mut split = VecSink::new(4096);
        let (pos, last) = resample_push(&mut split, &all[..64], step, 0.0, 0.0, 1.0);
        resample_push(&mut split, &all[64..], step, pos, last, 1.0);

        let n = split.out.len().min(whole.out.len());
        assert!(n > 200, "too few samples to be meaningful ({n})");
        for i in 0..n {
            assert!(
                (split.out[i] - whole.out[i]).abs() < 1e-4,
                "seam discontinuity at {i}: {} vs {}",
                split.out[i],
                whole.out[i]
            );
        }
    }

    /// The servo must push back toward half full from both directions, and
    /// never by enough to be heard.
    #[test]
    fn servo_corrects_in_both_directions_within_limits() {
        let nominal = servo_step(12_000, 48_000, 0.5);
        let draining = servo_step(12_000, 48_000, 0.0);
        let filling = servo_step(12_000, 48_000, 1.0);

        assert!(
            draining < nominal,
            "a draining queue must consume the source more slowly"
        );
        assert!(
            filling > nominal,
            "a filling queue must consume the source faster"
        );
        for (label, step) in [("draining", draining), ("filling", filling)] {
            let ppm = (step / nominal - 1.0).abs();
            assert!(ppm <= 0.0031, "{label} trim {ppm} exceeds the ±0.3 % cap");
        }
    }

    /// A full sink stops the loop instead of spinning or dropping silently
    /// mid-block; the carried phase must stay in range for the next call.
    #[test]
    fn full_sink_stops_and_leaves_usable_phase() {
        let src: Vec<f32> = (0..64).map(|i| i as f32).collect();
        let mut sink = VecSink::new(8);
        let (pos, last) = resample_push(&mut sink, &src, 1.0, 0.0, 0.0, 1.0);
        assert_eq!(sink.out.len(), 8, "sink overfilled");
        assert!(pos >= -1.0, "carried phase {pos} is out of range");
        assert_eq!(last, src[63]);
    }

    #[test]
    fn smoothing_moves_toward_the_observation() {
        let up = smooth_fill(0.0, 1.0);
        assert!(up > 0.0 && up < 0.1, "smoothing should be slow, got {up}");
        let down = smooth_fill(1.0, 0.0);
        assert!(down < 1.0 && down > 0.9, "smoothing should be slow, got {down}");
    }
}

/// Closed-loop behaviour of the drift servo.
///
/// The servo is the only thing bounding audio latency. Without it a clock
/// mismatch of a few tens of ppm accumulates silently: the queue either drains
/// to nothing (gaps) or grows without limit (audio falling further and further
/// behind the waterfall). Neither shows up in a short test of the resampler
/// alone, so simulate the loop.
#[cfg(test)]
mod servo_loop {
    use super::*;

    /// Run the source/sink loop with a deliberate clock error and report the
    /// queue depth over time, in output samples.
    ///
    /// `clock_error` is the sink's true rate divided by its nominal rate — the
    /// mismatch the servo cannot see directly and has to infer from occupancy.
    fn simulate(clock_error: f64, blocks: usize) -> Vec<f32> {
        const SOURCE_RATE: u32 = 12_000;
        const OUTPUT_RATE: u32 = 48_000;
        const BLOCK: usize = 240; // 20 ms of source audio

        let mut queue = (RING_CAPACITY / 2) as f64;
        let mut fill_avg = 0.5f32;
        let mut pos = 0.0f64;
        let mut last = 0.0f32;
        let mut depths = Vec::with_capacity(blocks);
        let src = vec![0.0f32; BLOCK];

        for _ in 0..blocks {
            let occupancy = (queue as f32 / RING_CAPACITY as f32).clamp(0.0, 1.0);
            fill_avg = smooth_fill(fill_avg, occupancy);
            let step = servo_step(SOURCE_RATE, OUTPUT_RATE, fill_avg);

            // Emit as many output samples as the resampler would for this block.
            let mut sink = CountingSink {
                count: 0,
                cap: RING_CAPACITY.saturating_sub(queue as usize),
            };
            let (p, l) = resample_push(&mut sink, &src, step, pos, last, 1.0);
            pos = p;
            last = l;
            queue += sink.count as f64;

            // The sink consumes one block's worth on its own (mismatched) clock.
            let consumed = BLOCK as f64 * (OUTPUT_RATE as f64 / SOURCE_RATE as f64) * clock_error;
            queue = (queue - consumed).max(0.0);
            depths.push(queue as f32);
        }
        depths
    }

    struct CountingSink {
        count: usize,
        cap: usize,
    }

    impl SampleSink for CountingSink {
        fn is_full(&self) -> bool {
            self.count >= self.cap
        }
        fn push_sample(&mut self, _s: f32) {
            self.count += 1;
        }
    }

    /// With clocks in perfect agreement the queue must simply stay put.
    #[test]
    fn matched_clocks_hold_the_queue_steady() {
        let depths = simulate(1.0, 2_000);
        let target = (RING_CAPACITY / 2) as f32;
        let final_depth = depths[depths.len() - 1];
        assert!(
            (final_depth - target).abs() < target * 0.25,
            "queue wandered to {final_depth} from {target} with matched clocks"
        );
    }

    /// A sink running fast drains the queue; the servo must catch it before it
    /// empties, or the user hears gaps.
    #[test]
    fn fast_sink_does_not_starve_the_queue() {
        let depths = simulate(1.000_050, 6_000);
        let settled = &depths[depths.len() / 2..];
        let min = settled.iter().copied().fold(f32::MAX, f32::min);
        assert!(
            min > 200.0,
            "queue fell to {min} samples — that is an audible dropout"
        );
    }

    /// A sink running slow fills the queue; the servo must cap it, or audio
    /// latency grows without limit relative to the waterfall.
    #[test]
    fn slow_sink_does_not_let_latency_grow() {
        let depths = simulate(0.999_950, 6_000);
        let settled = &depths[depths.len() / 2..];
        let max = settled.iter().copied().fold(0.0f32, f32::max);
        // 200 ms at 48 kHz is the whole queue; staying well inside it means
        // latency is bounded rather than creeping upward.
        assert!(
            max < RING_CAPACITY as f32 * 0.9,
            "queue grew to {max} of {RING_CAPACITY} — audio latency is unbounded"
        );
        // And it must actually be settling, not still climbing at the end.
        let first_half_max = depths[..depths.len() / 2]
            .iter()
            .copied()
            .fold(0.0f32, f32::max);
        assert!(
            max <= first_half_max * 1.05,
            "queue still growing ({first_half_max} then {max}) — servo has not converged"
        );
    }

    /// Steady-state latency must be low enough to keep audio with the display.
    #[test]
    fn settled_latency_stays_around_a_tenth_of_a_second() {
        let depths = simulate(1.000_010, 6_000);
        let settled = &depths[depths.len() / 2..];
        let mean = settled.iter().sum::<f32>() / settled.len() as f32;
        let secs = mean / 48_000.0;
        assert!(
            secs < 0.15,
            "settled audio latency {secs:.3}s is too far behind the waterfall"
        );
    }
}

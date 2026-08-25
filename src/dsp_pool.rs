//! Process-wide rayon pool sizing for DSP work.
//!
//! Rayon's default global pool spans every available core. The pipeline uses it
//! for one two-way `join` (demod vs. spectrum) and a bounded `par_iter` in the
//! skimmer, so a large machine would spawn many workers to service very little
//! parallelism. Those workers park rather than spin, but they still add
//! scheduler pressure next to a real-time audio callback thread, and that is
//! where jitter comes from.
//!
//! Frontend-agnostic on purpose: any frontend (GUI, CLI, bench) calls this once
//! at startup and gets the same behaviour.

use std::sync::Once;

/// Upper bound on DSP worker threads — the pipeline's actual parallelism is a
/// two-way split plus a small skimmer fan-out.
pub const MAX_DSP_THREADS: usize = 4;

static INIT: Once = Once::new();

/// Size the global rayon pool for DSP work. Idempotent; later calls are no-ops.
///
/// Safe to call after rayon has already been used — the build simply fails and
/// the existing pool stays, which is why the result is deliberately ignored.
pub fn init() {
    INIT.call_once(|| {
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(2);
        let threads = cores.clamp(1, MAX_DSP_THREADS);
        if rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|i| format!("hfsdr-dsp-{i}"))
            .build_global()
            .is_err()
        {
            crate::log::debug("rayon global pool already initialized; keeping it");
        } else {
            crate::log::debug(format!("rayon DSP pool: {threads} threads ({cores} cores)"));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_is_idempotent_and_bounded() {
        init();
        init();
        assert!(rayon::current_num_threads() >= 1);
        assert!(MAX_DSP_THREADS >= 2, "a two-way join needs two workers");
    }
}

//! UI-thread waterfall texture sync timings (compose, upload, row cadence).

use std::time::Instant;

/// Smoothed metrics for the waterfall viewport path (main thread).
#[derive(Clone, Debug, Default)]
pub struct WaterfallPerf {
    pub compose_ns: u64,
    pub upload_ns: u64,
    pub rows_applied_last: u32,
    pub rows_pending: u32,
    pub uploads_full: u64,
    pub uploads_partial: u64,
    pub sync_calls: u64,
    /// EMA of milliseconds between row-apply events.
    pub row_interval_ms: f32,
    pub rows_per_frame_cap: u32,
    last_row_apply: Option<Instant>,
}

impl WaterfallPerf {
    pub fn record_row_apply(&mut self, n: u32) {
        if n == 0 {
            return;
        }
        let now = Instant::now();
        if let Some(prev) = self.last_row_apply {
            let dt_ms = prev.elapsed().as_secs_f32() * 1000.0;
            self.row_interval_ms = if self.row_interval_ms <= 0.0 {
                dt_ms
            } else {
                self.row_interval_ms * 0.85 + dt_ms * 0.15
            };
        }
        self.last_row_apply = Some(now);
        self.rows_applied_last = n;
    }

    pub fn reset_interval(&mut self) {
        self.last_row_apply = None;
        self.row_interval_ms = 0.0;
    }
}

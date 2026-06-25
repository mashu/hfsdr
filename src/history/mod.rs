//! Slow band-history panel model (build-order item 4).
//!
//! A second, far slower waterfall: one averaged/peak-held row every few seconds
//! over a long window (5/10/15 min), plus time-tagged annotations dropped when a
//! CQ/spot is decoded. Shares the live FFT — only the time decimation differs.
//! This module owns the data model; rendering lives in the GUI binary.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How successive fast rows are folded into one slow row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowFold {
    /// Time-average (smooth activity map).
    Average,
    /// Peak-hold (keeps brief signals visible).
    Peak,
}

/// A marker placed on the slow waterfall at a frequency and time.
#[derive(Clone, Debug)]
pub struct Annotation {
    pub offset_hz: f32,
    pub at: Instant,
    pub label: String,
    pub snr_db: f32,
}

/// Ring of time-decimated spectrum rows + annotations.
#[derive(Debug)]
pub struct SlowWaterfall {
    rows: VecDeque<Vec<f32>>,
    annotations: VecDeque<Annotation>,
    accum: Vec<f32>,
    accum_count: u32,
    last_commit: Instant,
    seconds_per_row: f32,
    max_rows: usize,
    fold: RowFold,
}

impl SlowWaterfall {
    pub fn new(seconds_per_row: f32, window_seconds: f32, fold: RowFold) -> Self {
        let spr = seconds_per_row.max(0.5);
        let max_rows = (window_seconds / spr).ceil().max(1.0) as usize;
        Self {
            rows: VecDeque::with_capacity(max_rows),
            annotations: VecDeque::new(),
            accum: Vec::new(),
            accum_count: 0,
            last_commit: Instant::now(),
            seconds_per_row: spr,
            max_rows,
            fold,
        }
    }

    pub fn set_window(&mut self, seconds_per_row: f32, window_seconds: f32) {
        self.seconds_per_row = seconds_per_row.max(0.5);
        self.max_rows = (window_seconds / self.seconds_per_row).ceil().max(1.0) as usize;
        while self.rows.len() > self.max_rows {
            self.rows.pop_back();
        }
    }

    pub fn set_fold(&mut self, fold: RowFold) {
        self.fold = fold;
    }

    pub fn rows(&self) -> &VecDeque<Vec<f32>> {
        &self.rows
    }

    pub fn annotations(&self) -> &VecDeque<Annotation> {
        &self.annotations
    }

    /// Fold one fast FFT row in; commits a slow row when the interval elapses.
    pub fn push_row(&mut self, row: &[f32]) {
        if self.accum.len() != row.len() {
            self.accum = vec![match self.fold {
                RowFold::Average => 0.0,
                RowFold::Peak => f32::NEG_INFINITY,
            }; row.len()];
            self.accum_count = 0;
        }
        match self.fold {
            RowFold::Average => {
                for (a, &v) in self.accum.iter_mut().zip(row) {
                    *a += v;
                }
            }
            RowFold::Peak => {
                for (a, &v) in self.accum.iter_mut().zip(row) {
                    *a = a.max(v);
                }
            }
        }
        self.accum_count += 1;

        if self.last_commit.elapsed() >= Duration::from_secs_f32(self.seconds_per_row) {
            self.commit();
        }
    }

    fn commit(&mut self) {
        if self.accum.is_empty() || self.accum_count == 0 {
            return;
        }
        let row: Vec<f32> = match self.fold {
            RowFold::Average => self
                .accum
                .iter()
                .map(|&s| s / self.accum_count as f32)
                .collect(),
            RowFold::Peak => self.accum.clone(),
        };
        if self.rows.len() >= self.max_rows {
            self.rows.pop_back();
        }
        self.rows.push_front(row);

        for a in self.accum.iter_mut() {
            *a = match self.fold {
                RowFold::Average => 0.0,
                RowFold::Peak => f32::NEG_INFINITY,
            };
        }
        self.accum_count = 0;
        self.last_commit = Instant::now();
    }

    /// Drop a time-tagged marker (e.g. a decoded CQ).
    pub fn annotate(&mut self, offset_hz: f32, label: impl Into<String>, snr_db: f32) {
        self.annotations.push_front(Annotation {
            offset_hz,
            at: Instant::now(),
            label: label.into(),
            snr_db,
        });
        let max_age = Duration::from_secs_f32(self.seconds_per_row * self.max_rows as f32);
        while self
            .annotations
            .back()
            .map(|a| a.at.elapsed() > max_age)
            .unwrap_or(false)
        {
            self.annotations.pop_back();
        }
    }
}

#[cfg(test)]
impl SlowWaterfall {
    fn finish_accumulator(&mut self) {
        self.commit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_sizing_limits_rows() {
        let wf = SlowWaterfall::new(1.0, 10.0, RowFold::Average);
        assert_eq!(wf.max_rows, 10);
    }

    #[test]
    fn annotations_record() {
        let mut wf = SlowWaterfall::new(1.0, 10.0, RowFold::Peak);
        wf.annotate(300.0, "CQ AA1A", 20.0);
        assert_eq!(wf.annotations().len(), 1);
    }

    #[test]
    fn peak_fold_commits_row() {
        let mut wf = SlowWaterfall::new(1.0, 10.0, RowFold::Peak);
        wf.push_row(&[1.0, 3.0, 2.0]);
        wf.push_row(&[0.5, 4.0, 1.0]);
        assert!(wf.rows().is_empty());
        wf.finish_accumulator();
        assert_eq!(wf.rows().len(), 1);
        assert_eq!(wf.rows()[0], vec![1.0, 4.0, 2.0]);
    }

    #[test]
    fn average_fold_divides_accumulator() {
        let mut wf = SlowWaterfall::new(1.0, 10.0, RowFold::Average);
        wf.push_row(&[2.0, 4.0]);
        wf.push_row(&[4.0, 8.0]);
        wf.finish_accumulator();
        let row = &wf.rows()[0];
        assert!((row[0] - 3.0).abs() < 1e-3);
        assert!((row[1] - 6.0).abs() < 1e-3);
    }

    #[test]
    fn set_window_trims_old_rows() {
        let mut wf = SlowWaterfall::new(1.0, 10.0, RowFold::Peak);
        for v in 1..=5 {
            wf.push_row(&[v as f32]);
            wf.finish_accumulator();
        }
        assert_eq!(wf.rows().len(), 5);
        wf.set_window(1.0, 2.0);
        assert_eq!(wf.rows().len(), 2);
        wf.set_fold(RowFold::Average);
    }

    #[test]
    fn push_row_resizes_accumulator_on_width_change() {
        let mut wf = SlowWaterfall::new(1.0, 10.0, RowFold::Peak);
        wf.push_row(&[1.0, 2.0]);
        wf.push_row(&[3.0, 4.0, 5.0]);
        wf.finish_accumulator();
        assert_eq!(wf.rows()[0].len(), 3);
        assert_eq!(wf.rows()[0], vec![3.0, 4.0, 5.0]);
    }
}

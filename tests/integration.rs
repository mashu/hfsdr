//! Integration tests for the public library API.

use hfsdr::{Complex32, SpectrumAnalyzer, SourceError};
use std::f32::consts::TAU;

#[test]
fn spectrum_analyzer_processes_tone() {
    let n = 128;
    let mut sa = SpectrumAnalyzer::new(n, n / 2);
    let sr = 12_000.0;
    let freq = 500.0;
    let samples: Vec<Complex32> = (0..n * 2)
        .map(|t| {
            let phase = TAU * freq * t as f32 / sr;
            Complex32::new(phase.cos(), phase.sin())
        })
        .collect();

    let mut emitted = 0;
    sa.process(&samples, |_| emitted += 1);
    assert!(emitted >= 1);
}

#[test]
fn source_error_is_std_error() {
    let err: Box<dyn std::error::Error> = Box::new(SourceError::NotFound);
    assert!(err.to_string().contains("device"));
}

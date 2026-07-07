//! Integration tests for the public library API.

use hfsdr::{
    decimation_factor, Complex32, CwChannelSettings, IngressWorker, IqAudioDemod, SpectrumAnalyzer,
    SourceError,
};
use std::f32::consts::TAU;
use std::sync::Arc;

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

#[test]
fn ingress_worker_decimates_block() {
    let worker = IngressWorker::spawn();
    let raw: Vec<Complex32> = (0..128)
        .map(|i| Complex32::new((i as f32 * 0.1).sin(), 0.0))
        .collect();
    assert!(worker.start(
        Arc::new(raw),
        48_000.0,
        4,
        hfsdr::DecimFilterKind::LinearFir,
        Vec::new(),
    ));
    let out = worker.finish().expect("decimated");
    assert!(!out.is_empty());
    assert!(out.len() < 128);
}

#[test]
fn cw_demod_end_to_end_from_integration() {
    let rate = 12_000.0;
    let iq: Vec<Complex32> = (0..rate as usize)
        .map(|i| {
            let t = i as f32 / rate;
            let p = TAU * 300.0 * t;
            Complex32::new(p.cos(), p.sin())
        })
        .collect();
    let mut demod = IqAudioDemod::new();
    let mut settings = CwChannelSettings::default();
    settings.agc.enabled = false;
    let mut audio = Vec::new();
    demod.process(&iq, rate, &settings, &mut audio);
    assert!(!audio.is_empty());
}

#[test]
fn decimation_heuristic_targets_audio_rate() {
    assert_eq!(decimation_factor(12_000.0), 1);
    assert!(decimation_factor(384_000.0) >= 16);
}

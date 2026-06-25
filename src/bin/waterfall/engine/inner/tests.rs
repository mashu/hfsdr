//! Direct engine-thread tests (playback IQ file + mock ring) — no live hardware.

use std::f32::consts::TAU;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hfsdr::{Complex32, IqRecorder};

use super::Engine;
use crate::audio;
use crate::engine::types::{
    ConnState, EngineCommand, EngineParams, EngineShared,
};
use crate::engine::FFT_SIZE;
use crate::source::ConnectRequest;

fn test_engine() -> (
    Engine,
    Arc<Mutex<EngineShared>>,
    Arc<Mutex<EngineParams>>,
) {
    audio::set_test_output_devices(Some(vec!["Test Output".into()]));
    let (_tx, rx) = channel();
    let shared = Arc::new(Mutex::new(EngineShared::default()));
    let params = Arc::new(Mutex::new(EngineParams::default()));
    let cancel = Arc::new(AtomicBool::new(false));
    let engine = Engine::new(rx, Arc::clone(&shared), Arc::clone(&params), cancel);
    (engine, shared, params)
}

fn tone_iq(n: usize, rate: f32, tone_hz: f32, amp: f32) -> Vec<Complex32> {
    (0..n)
        .map(|i| {
            let t = i as f32 / rate;
            let ph = TAU * tone_hz * t;
            Complex32::new(ph.cos() * amp, ph.sin() * amp)
        })
        .collect()
}

static CAPTURE_SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_capture_path(prefix: &str) -> PathBuf {
    let seq = CAPTURE_SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "hfsdr_{prefix}_{}_{}.hiq.gz",
        std::process::id(),
        seq
    ))
}

fn write_capture(samples: &[Complex32], rate: u32, center_hz: f64) -> PathBuf {
    let path = temp_capture_path("engine_test");
    let rec = IqRecorder::start(path.clone(), rate, center_hz).expect("recorder");
    rec.push(samples);
    rec.stop().expect("stop");
    path
}

fn mock_kiwi_ring(samples: &[Complex32]) -> crate::source::Connection {
    crate::source::Connection::mock_ring(samples, 14_010_000.0, false)
}

fn wait_playback_prefill(engine: &Engine, max_ms: u64) {
    let deadline = Instant::now() + Duration::from_millis(max_ms);
    while Instant::now() < deadline {
        if engine
            .playback
            .as_ref()
            .is_some_and(|pb| pb.buffer_fill() > 0.05)
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn pump_until<F>(engine: &mut Engine, max_iters: usize, mut done: F) -> bool
where
    F: FnMut(&Engine) -> bool,
{
    for _ in 0..max_iters {
        engine.pump_stream();
        engine.publish_stats(0);
        if done(engine) {
            return true;
        }
    }
    false
}

#[test]
fn playback_command_streams_iq_through_pump() {
    let samples = tone_iq(48_000, 12_000.0, 700.0, 0.35);
    let path = write_capture(&samples, 12_000, 14_010_000.0);
    let (mut engine, shared, _) = test_engine();
    engine.handle_command(EngineCommand::PlayIqFile(path.clone()));
    assert!(engine.playback.is_some());
    wait_playback_prefill(&engine, 500);
    let ok = pump_until(&mut engine, 120, |e| {
        e.last_pump_got > 0 && e.latest.iter().any(|&v| v > -100.0)
    });
    let state = shared.lock().expect("lock").state.clone();
    assert!(ok, "pump should produce spectrum from playback");
    assert!(matches!(state, ConnState::Streaming));
    let _ = std::fs::remove_file(path);
}

#[test]
fn mock_ring_connection_pumps_spectrum() {
    let samples = tone_iq(24_000, 12_000.0, 750.0, 0.4);
    let (mut engine, shared, _) = test_engine();
    engine.conn = Some(mock_kiwi_ring(&samples));
    engine.first_iq_received = true;
    engine.set_state(ConnState::Streaming);
    engine.last_data = Instant::now();
    let ok = pump_until(&mut engine, 60, |e| e.latest.iter().any(|&v| v > -95.0));
    assert!(ok);
    assert!(shared.lock().expect("lock").stats.sample_rate > 0.0);
}

#[test]
fn disconnect_clears_playback_and_connection() {
    let path = write_capture(&tone_iq(8_192, 12_000.0, 700.0, 0.3), 12_000, 14_000_000.0);
    let (mut engine, shared, _) = test_engine();
    engine.handle_command(EngineCommand::PlayIqFile(path.clone()));
    engine.handle_command(EngineCommand::Disconnect);
    assert!(engine.playback.is_none());
    assert!(engine.conn.is_none());
    assert!(matches!(
        shared.lock().expect("lock").state,
        ConnState::Disconnected
    ));
    let _ = std::fs::remove_file(path);
}

#[test]
fn stop_playback_returns_disconnected() {
    let path = write_capture(&tone_iq(4_096, 12_000.0, 700.0, 0.3), 12_000, 14_000_000.0);
    let (mut engine, shared, _) = test_engine();
    engine.handle_command(EngineCommand::PlayIqFile(path.clone()));
    engine.handle_command(EngineCommand::StopIqPlayback);
    assert!(engine.playback.is_none());
    assert!(matches!(
        shared.lock().expect("lock").state,
        ConnState::Disconnected
    ));
    let _ = std::fs::remove_file(path);
}

#[test]
fn fail_connection_schedules_reconnect() {
    let (mut engine, shared, _) = test_engine();
    engine.request = Some(ConnectRequest {
        host: "rx.test".into(),
        ..ConnectRequest::default()
    });
    engine.fail_connection("test failure".into());
    assert!(engine.retry_at.is_some());
    assert!(matches!(
        shared.lock().expect("lock").state,
        ConnState::Reconnecting { .. }
    ));
    assert_eq!(
        shared.lock().expect("lock").last_error.as_deref(),
        Some("test failure")
    );
}

#[test]
fn fail_connection_without_request_disconnects() {
    let (mut engine, shared, _) = test_engine();
    engine.request = None;
    engine.fail_connection("ignored".into());
    assert!(engine.retry_at.is_none());
    assert!(matches!(
        shared.lock().expect("lock").state,
        ConnState::Disconnected
    ));
}

#[test]
fn tune_updates_request_center() {
    let (mut engine, _, _) = test_engine();
    engine.conn = Some(mock_kiwi_ring(&[]));
    engine.request = Some(ConnectRequest {
        host: "rx.test".into(),
        ..ConnectRequest::default()
    });
    engine.handle_command(EngineCommand::Tune(14_050_000.0));
    assert_eq!(
        engine.request.as_ref().map(|r| r.center_hz),
        Some(14_050_000.0)
    );
    assert_eq!(
        engine.conn.as_ref().map(|c| c.center_hz),
        Some(14_050_000.0)
    );
}

#[test]
fn clear_skimmer_spots_resets_peak_hold() {
    let (mut engine, _, _) = test_engine();
    engine.skimmer_peak_hold.fill(-40.0);
    engine.handle_command(EngineCommand::ClearSkimmerSpots);
    assert!(engine.skimmer_peak_hold.iter().all(|&v| v <= -119.0));
}

#[test]
fn start_iq_record_during_streaming() {
    let samples = tone_iq(24_000, 12_000.0, 700.0, 0.3);
    let rec_path = temp_capture_path("rec");
    let (mut engine, shared, _) = test_engine();
    engine.conn = Some(mock_kiwi_ring(&samples));
    engine.first_iq_received = true;
    engine.set_state(ConnState::Streaming);
    engine.last_data = Instant::now();
    engine.handle_command(EngineCommand::StartIqRecord(rec_path.clone()));
    assert!(engine.recorder.is_some());
    for _ in 0..40 {
        engine.pump_stream();
    }
    engine.handle_command(EngineCommand::StopIqRecord);
    assert!(engine.recorder.is_none());
    assert!(shared.lock().expect("lock").stats.iq_capture_samples > 0);
    let _ = std::fs::remove_file(rec_path);
}

#[test]
fn playback_finishes_to_disconnected() {
    let samples = tone_iq(4_096, 12_000.0, 700.0, 0.3);
    let path = write_capture(&samples, 12_000, 14_000_000.0);
    let (mut engine, shared, _) = test_engine();
    engine.handle_command(EngineCommand::PlayIqFile(path.clone()));
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        engine.pump_stream();
        if engine.playback.is_none() {
            break;
        }
    }
    assert!(engine.playback.is_none());
    assert!(matches!(
        shared.lock().expect("lock").state,
        ConnState::Disconnected
    ));
    let _ = std::fs::remove_file(path);
}

#[test]
fn publish_stats_reflects_fft_size() {
    let (mut engine, shared, _) = test_engine();
    engine.latest = vec![-80.0; FFT_SIZE];
    engine.publish_stats(128);
    let stats = shared.lock().expect("lock").stats.clone();
    assert_eq!(stats.spectrum_fft, FFT_SIZE);
    assert_eq!(stats.last_drain, 128);
}

#[test]
fn start_connect_without_request_disconnects() {
    let (mut engine, shared, _) = test_engine();
    engine.request = None;
    engine.start_connect(&ConnectRequest::default());
    assert!(matches!(
        shared.lock().expect("lock").state,
        ConnState::Disconnected
    ));
}

#[test]
fn schedule_reconnect_sets_retry_at() {
    let (mut engine, _, _) = test_engine();
    engine.request = Some(ConnectRequest {
        host: "rx.test".into(),
        ..ConnectRequest::default()
    });
    engine.schedule_reconnect();
    assert_eq!(engine.reconnect_attempt, 1);
    assert!(engine.retry_at.is_some());
}

#[test]
fn disconnect_command_clears_request() {
    let (mut engine, shared, _) = test_engine();
    engine.request = Some(ConnectRequest::default());
    engine.handle_command(EngineCommand::Disconnect);
    assert!(engine.request.is_none());
    assert!(matches!(
        shared.lock().expect("lock").state,
        ConnState::Disconnected
    ));
}

#[test]
fn set_kiwi_controls_with_mock_connection() {
    let (mut engine, _, _) = test_engine();
    engine.conn = Some(mock_kiwi_ring(&[]));
    engine.handle_command(EngineCommand::SetRfAgc(true));
    engine.handle_command(EngineCommand::SetKiwiManGain(40));
    engine.handle_command(EngineCommand::SetKiwiRfAttn(6.0));
}

#[test]
fn poll_handshake_fails_when_kiwi_stalled() {
    let (mut engine, shared, _) = test_engine();
    engine.request = Some(ConnectRequest {
        host: "rx.test".into(),
        ..ConnectRequest::default()
    });
    engine.conn = Some(crate::source::Connection::mock_ring(&[], 14_010_000.0, true));
    engine.first_iq_received = false;
    engine.connected_at = Instant::now() - Duration::from_secs(120);
    engine.poll_handshake();
    assert!(matches!(
        shared.lock().expect("lock").state,
        ConnState::Reconnecting { .. }
    ));
}

#[test]
fn maybe_reconnect_on_stall_after_data_gap() {
    let (mut engine, shared, _) = test_engine();
    engine.request = Some(ConnectRequest::default());
    engine.conn = Some(mock_kiwi_ring(&tone_iq(1024, 12_000.0, 700.0, 0.2)));
    engine.first_iq_received = true;
    engine.last_data = Instant::now() - Duration::from_secs(120);
    engine.maybe_reconnect_on_stall();
    assert!(matches!(
        shared.lock().expect("lock").state,
        ConnState::Reconnecting { .. }
    ));
}

#[test]
fn schedule_reconnect_busy_uses_longer_delay() {
    let (mut engine, _, _) = test_engine();
    engine.request = Some(ConnectRequest::default());
    engine.set_error(Some("receiver busy".into()));
    engine.schedule_reconnect();
    let retry = engine.retry_at.expect("retry");
    assert!(retry > Instant::now());
    assert!(engine.reconnect_attempt >= 1);
}

#[test]
fn set_audio_device_reopens_output() {
    let (mut engine, _, _) = test_engine();
    engine.handle_command(EngineCommand::SetAudioDevice(Some("Test Output".into())));
    assert_eq!(engine.audio_device.as_deref(), Some("Test Output"));
}

#[test]
fn reload_scp_commands_publish_stats() {
    let (mut engine, shared, _) = test_engine();
    engine.handle_command(EngineCommand::ReloadScp);
    engine.handle_command(EngineCommand::ReloadScpFrom(
        std::path::PathBuf::from("/nonexistent/master.scp"),
    ));
    let _ = shared.lock().expect("lock").stats.clone();
}

#[test]
fn wideband_mock_ring_pumps_with_skimmer() {
    let samples = tone_iq(96_000, 384_000.0, 700.0, 0.3);
    let (mut engine, shared, params) = test_engine();
    let mut conn = mock_kiwi_ring(&samples);
    conn.device_sample_rate = 384_000.0;
    conn.sample_rate = 96_000.0;
    conn.iq_ingress_decim = 4;
    engine.conn = Some(conn);
    engine.first_iq_received = true;
    engine.set_state(ConnState::Streaming);
    engine.last_data = Instant::now();
    {
        let mut p = params.lock().expect("lock");
        p.skimmer_enabled = true;
        p.full_drain_spectrum = true;
    }
    for _ in 0..40 {
        engine.pump_stream();
    }
    assert!(engine.last_pump_got > 0 || shared.lock().expect("lock").stats.sample_rate > 0.0);
}

#[test]
fn maybe_retry_reconnect_when_due() {
    let (mut engine, shared, _) = test_engine();
    engine.request = Some(ConnectRequest::default());
    engine.reconnect_attempt = 1;
    engine.retry_at = Some(Instant::now() - Duration::from_secs(1));
    engine.maybe_retry_reconnect();
    assert!(matches!(
        shared.lock().expect("lock").state,
        ConnState::Disconnected | ConnState::Connecting { .. } | ConnState::Reconnecting { .. }
    ));
}

#[test]
fn engine_run_loop_playback_and_shutdown() {
    use std::sync::mpsc::channel;
    use std::thread;

    let (tx, rx) = channel();
    let shared = Arc::new(Mutex::new(EngineShared::default()));
    let params = Arc::new(Mutex::new(EngineParams::default()));
    let cancel = Arc::new(AtomicBool::new(false));
    audio::set_test_output_devices(Some(vec!["Test Output".into()]));
    let shared_bg = Arc::clone(&shared);
    let handle = thread::spawn(move || {
        let mut engine = Engine::new(rx, shared_bg, params, cancel);
        engine.run();
    });
    let path = write_capture(&tone_iq(8_192, 12_000.0, 700.0, 0.3), 12_000, 14_010_000.0);
    tx.send(EngineCommand::PlayIqFile(path.clone())).expect("play");
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if matches!(
            shared.lock().expect("lock").state,
            ConnState::Streaming
        ) {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    tx.send(EngineCommand::Shutdown).expect("shutdown");
    handle.join().expect("join");
    let _ = std::fs::remove_file(path);
}

#[test]
fn device_specific_commands_on_kiwi_connection() {
    let (mut engine, _, _) = test_engine();
    engine.conn = Some(mock_kiwi_ring(&[]));
    #[cfg(feature = "airspy")]
    {
        engine.handle_command(EngineCommand::SetAirspyAtt(2));
        engine.handle_command(EngineCommand::SetAirspyLna(true));
        engine.handle_command(EngineCommand::SetAirspyAgcThreshold(true));
        engine.handle_command(EngineCommand::SetAirspyFrontendOptions(1));
        engine.handle_command(EngineCommand::SetAirspyBiasTee(true));
    }
    #[cfg(feature = "rtlsdr")]
    {
        engine.handle_command(EngineCommand::SetRtlSdrRtlAgc(true));
        engine.handle_command(EngineCommand::SetRtlSdrManualGain(true));
        engine.handle_command(EngineCommand::SetRtlSdrTunerGain(196));
        engine.handle_command(EngineCommand::SetRtlSdrBiasTee(false));
        engine.handle_command(EngineCommand::SetRtlSdrPpm(5));
    }
    #[cfg(feature = "qmx")]
    {
        engine.handle_command(EngineCommand::SetQmxRfGain(8));
    }
}

#[test]
fn connect_command_stores_request() {
    let (mut engine, shared, _) = test_engine();
    let req = ConnectRequest {
        host: "rx.test".into(),
        ..ConnectRequest::default()
    };
    engine.handle_command(EngineCommand::Connect(req.clone()));
    assert_eq!(engine.request.as_ref().map(|r| r.host.as_str()), Some("rx.test"));
    assert!(matches!(
        shared.lock().expect("lock").state,
        ConnState::Connecting { .. } | ConnState::Disconnected | ConnState::Reconnecting { .. }
    ));
}

#[test]
fn tune_without_connection_updates_request_only() {
    let (mut engine, _, _) = test_engine();
    engine.request = Some(ConnectRequest::default());
    engine.handle_command(EngineCommand::Tune(14_050_000.0));
    assert_eq!(
        engine.request.as_ref().map(|r| r.center_hz),
        Some(14_050_000.0)
    );
}

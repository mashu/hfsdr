//! Direct engine-thread tests (playback IQ file + mock ring) — no live hardware.

use std::f32::consts::TAU;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use hfsdr::time::{Duration, Instant};

use hfsdr::{Complex32, IqRecorder};

use super::Engine;
use crate::audio;
use crate::engine::types::{
    ConnState, EngineCommand, EngineParams, EngineSnapshot,
};
use crate::engine::FFT_SIZE;
use crate::source::ConnectRequest;

/// Reads what the engine published, the way the UI does.
///
/// Keeps the `.lock().expect("lock").field` shape the tests were written
/// against, so the boundary change stays a boundary change. There is no lock
/// underneath any more — `lock` fetches the latest snapshot and hands back a
/// view of it.
pub(super) struct Published(std::cell::RefCell<crate::engine::link::UiLink>);

impl Published {
    #[allow(clippy::result_unit_err)]
    pub(super) fn lock(&self) -> Result<std::cell::Ref<'_, EngineSnapshot>, ()> {
        self.0.borrow_mut().snapshot.fetch();
        Ok(std::cell::Ref::map(self.0.borrow(), |l| l.snapshot.slot()))
    }

    pub(super) fn set_params(&self, params: EngineParams) {
        let mut link = self.0.borrow_mut();
        *link.params.slot() = params;
        link.params.publish();
    }
}

fn test_engine() -> (Engine, Published, Published) {
    audio::set_test_output_devices(Some(vec!["Test Output".into()]));
    let (engine_side, ui_side) = crate::engine::link::engine_link();
    let engine = Engine::new(engine_side);
    let published = Published(std::cell::RefCell::new(ui_side));
    // Both slots of the old tuple are the same object now; the second used to
    // be the params mutex and is kept so call sites do not all have to change.
    let dummy = Published(std::cell::RefCell::new(crate::engine::link::engine_link().1));
    (engine, published, dummy)
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

/// Opening audio for a connection must keep an output that is already usable.
///
/// `audio_device_open` runs on every connection attempt, and a receiver that
/// refuses the connection is retried on a backoff. Replacing the output each
/// time discarded one per retry — looks harmless natively, but in a browser
/// each is an `AudioContext` the page cannot get back, and each orphaned one
/// keeps posting into a freed closure until the tab is unusable.
///
/// Tested through the decision rather than its effect: a headless runner has no
/// sound card, so every open returns `None` and a test written against
/// `engine.audio` passes without exercising anything. That version of this test
/// stayed green with the bug reintroduced.
#[test]
fn an_open_output_is_kept_across_connection_attempts() {
    use crate::engine::inner::connection::keep_existing_output;

    // No output yet: must open one.
    assert!(!keep_existing_output(None, None));
    assert!(!keep_existing_output(None, Some("Speakers")));

    // Open, and no particular device asked for: keep it. This is the case that
    // ran on every attempt, and reopening here is what leaked.
    assert!(keep_existing_output(Some("Speakers"), None));

    // Open on the device that was asked for: keep it.
    assert!(keep_existing_output(Some("Speakers"), Some("Speakers")));

    // Open on a different device: must reopen.
    assert!(!keep_existing_output(Some("Speakers"), Some("Headphones")));
}

#[test]
fn wideband_mock_ring_pumps() {
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
    params.set_params(EngineParams {
        full_drain_spectrum: true,
        ..EngineParams::default()
    });
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
    use std::thread;

    audio::set_test_output_devices(Some(vec!["Test Output".into()]));
    let (engine_side, ui_side) = crate::engine::link::engine_link();
    let tx = ui_side.cmd_tx.clone();
    let shared = Published(std::cell::RefCell::new(ui_side));
    let handle = thread::spawn(move || {
        let mut engine = Engine::new(engine_side);
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
    engine.handle_command(EngineCommand::Connect(Box::new(req.clone())));
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

/// The browser drives the engine with `step` instead of `run`, and that path
/// cannot be exercised on any CI target — wasm32 has no threads and no live
/// receiver. These tests run the same driver natively, which is the only place
/// its behaviour can be checked at all.
mod stepped_driver {
    use super::*;
    use crate::engine::inner::IdlePacing;
    use std::sync::mpsc::Sender;

    /// An engine plus a *live* command sender.
    ///
    /// `test_engine` drops its sender, which disconnects the channel and stops
    /// the engine on the first step — every idle path then short-circuits and
    /// a test built on it silently checks nothing.
    fn stepped_engine() -> (Engine, Sender<EngineCommand>, Published) {
        audio::set_test_output_devices(Some(vec!["Test Output".into()]));
        let (engine_side, ui_side) = crate::engine::link::engine_link();
        let tx = ui_side.cmd_tx.clone();
        let engine = Engine::new(engine_side);
        (engine, tx, Published(std::cell::RefCell::new(ui_side)))
    }

    /// `step` must never block: the browser calls it from the frame callback,
    /// where a 20 ms wait per idle iteration is a visibly frozen tab.
    ///
    /// The sender is held for the whole test precisely so the channel stays
    /// open — that is what forces the idle wait to be reached at all.
    #[test]
    fn step_never_blocks_while_idle() {
        let (mut engine, tx, _shared) = stepped_engine();
        const STEPS: usize = 50;

        let t0 = Instant::now();
        for _ in 0..STEPS {
            engine.step(IdlePacing::Return);
        }
        let elapsed = t0.elapsed();

        assert!(engine.running, "engine stopped early; the idle path never ran");
        // The parking driver waits 20 ms per idle step, so it would need ~1 s.
        assert!(
            elapsed < Duration::from_millis(100),
            "{STEPS} idle steps took {elapsed:?} — the browser driver blocks and would stall the tab"
        );
        drop(tx);
    }

    /// An idle engine still publishes every step, so the UI keeps updating
    /// instead of freezing on whatever it last saw.
    ///
    /// The proof is that the reader sees a *fresh* publish: a latest-value slot
    /// reports whether anything new arrived since the last fetch, which is
    /// exactly the question. Asserting on a field's value would pass on a stale
    /// slot that happened to hold the right number.
    #[test]
    fn idle_steps_publish_every_time() {
        let (mut engine, tx, published) = stepped_engine();
        engine.latest = vec![-80.0; FFT_SIZE];

        // Clear anything published during construction.
        let _ = published.lock();

        engine.step(IdlePacing::Return);
        {
            let guard = published.lock().expect("lock");
            assert_eq!(guard.stats.spectrum_fft, FFT_SIZE);
        }

        // And again: every idle step must publish, not just the first.
        assert!(
            !published.0.borrow_mut().snapshot.fetch(),
            "nothing should be pending immediately after a fetch"
        );
        engine.step(IdlePacing::Return);
        assert!(
            published.0.borrow_mut().snapshot.fetch(),
            "an idle step published nothing"
        );
        drop(tx);
    }

    /// A command queued before the step must be acted on within that step.
    #[test]
    fn step_handles_queued_commands() {
        let (mut engine, tx, _shared) = stepped_engine();
        tx.send(EngineCommand::Shutdown).expect("send shutdown");
        engine.step(IdlePacing::Return);
        assert!(
            !engine.running,
            "stepped driver ignored a command that run() would have handled"
        );
    }

    /// A command arriving *while streaming* is drained too. The idle branch and
    /// the streaming branch take different paths to the command queue, and only
    /// this one goes through `drain_commands` inside the pump loop.
    #[test]
    fn step_handles_commands_while_streaming() {
        let (mut engine, tx, _shared) = stepped_engine();
        engine.conn = Some(crate::source::Connection::mock_ring(
            &tone_iq(4096, 12_000.0, 700.0, 0.2),
            14_010_000.0,
            true,
        ));
        engine.step(IdlePacing::Return);
        assert!(engine.running);

        tx.send(EngineCommand::Shutdown).expect("send shutdown");
        engine.step(IdlePacing::Return);
        assert!(
            !engine.running,
            "a command sent while streaming was never drained"
        );
    }

    /// Dropping the last sender must stop the engine rather than leave the
    /// browser stepping a dead engine forever.
    ///
    /// "Last" is the point: the UI half of the link owns a sender too, so both
    /// have to go. That is also how it happens in the app — the whole handle is
    /// dropped, taking the link with it.
    #[test]
    fn step_stops_when_every_sender_is_gone() {
        let (mut engine, tx, published) = stepped_engine();
        engine.step(IdlePacing::Return);
        assert!(engine.running, "engine stopped while the channel was open");

        drop(tx);
        engine.step(IdlePacing::Return);
        assert!(
            engine.running,
            "the UI half still holds a sender; the engine must keep running"
        );

        drop(published);
        engine.step(IdlePacing::Return);
        assert!(!engine.running, "a fully closed channel must end the engine");
    }
}

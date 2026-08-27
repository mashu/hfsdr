//! WebAudio output for the browser build.
//!
//! Three things make a tab different from a sound card, and all three are why
//! this is not just cpal with different names:
//!
//! 1. **An `AudioContext` starts suspended.** Autoplay policy requires a user
//!    gesture before any sound. Resuming is retried on every push and also
//!    hooked to the first pointer/key event, so the stream starts the moment
//!    the user touches anything.
//! 2. **There is no callback thread we can share memory with.** Without
//!    cross-origin isolation there is no `SharedArrayBuffer`, so samples reach
//!    the audio thread by `postMessage` and the worklet keeps its own queue.
//! 3. **The output rate is the browser's, not ours.** It is whatever the
//!    device runs at (often 48 kHz, sometimes 44.1), read back after the
//!    context exists.
//!
//! The worklet reports its queue depth back, which is what feeds the same
//! drift servo the desktop backend uses — without it the queue would walk into
//! a permanent underrun or a growing delay.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Mutex;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{AudioContext, AudioContextState, AudioWorkletNode, MessageEvent};

use super::resample::{resample_push, servo_step, smooth_fill, SampleSink, RING_CAPACITY};

static TEST_OUTPUT_DEVICES: Mutex<Option<Vec<String>>> = Mutex::new(None);

/// Injection point used by UI tests; the browser has no device list.
pub fn set_test_output_devices(devices: Option<Vec<String>>) {
    if let Ok(mut g) = TEST_OUTPUT_DEVICES.lock() {
        *g = devices;
    }
}

/// Nominal rate reported before a context exists.
pub const OUTPUT_SAMPLE_RATE: u32 = 48_000;

/// The audio-thread half. Kept as source text so the build needs no second
/// asset: it is turned into a module at runtime through a blob URL.
///
/// `process` must never allocate or block — it runs on the audio thread, and
/// overrunning its deadline is a dropout. It only copies out of an already
/// queued array and reports depth every 8th quantum (~21 ms at 48 kHz), which
/// is often enough for a servo whose whole correction range is 0.3 %.
const WORKLET_SOURCE: &str = r#"
class HfsdrSink extends AudioWorkletProcessor {
  constructor() {
    super();
    this.queue = [];
    this.head = 0;
    this.buffered = 0;
    this.skip = 0;
    this.ticks = 0;
    this.port.onmessage = (e) => {
      const m = e.data;
      if (m.samples) { this.queue.push(m.samples); this.buffered += m.samples.length; }
      else if (m.skip) { this.skip += m.skip; }
    };
  }
  pull() {
    while (this.queue.length && this.head >= this.queue[0].length) {
      this.queue.shift();
      this.head = 0;
    }
    if (!this.queue.length) return null;
    this.buffered--;
    return this.queue[0][this.head++];
  }
  process(_inputs, outputs) {
    // Drop stale audio in one go rather than muting for the skip duration.
    while (this.skip > 0 && this.pull() !== null) this.skip--;
    const out = outputs[0];
    const ch0 = out[0];
    for (let i = 0; i < ch0.length; i++) {
      const v = this.pull();
      ch0[i] = v === null ? 0 : v;
    }
    for (let c = 1; c < out.length; c++) out[c].set(ch0);
    if ((this.ticks++ & 7) === 0) this.port.postMessage(this.buffered);
    return true;
  }
}
registerProcessor('hfsdr-sink', HfsdrSink);
"#;

/// Resampled samples destined for one `postMessage`.
struct BlockSink<'a> {
    out: &'a mut Vec<f32>,
    limit: usize,
}

impl SampleSink for BlockSink<'_> {
    fn is_full(&self) -> bool {
        self.out.len() >= self.limit
    }
    fn push_sample(&mut self, sample: f32) {
        self.out.push(sample);
    }
}

pub struct AudioOutput {
    ctx: AudioContext,
    /// `None` until `audioWorklet.addModule` resolves; pushes are dropped
    /// until then rather than queued, because stale audio at start-up is worse
    /// than a short silence.
    node: Rc<std::cell::RefCell<Option<AudioWorkletNode>>>,
    /// Queue depth last reported by the worklet, in output samples.
    buffered: Rc<Cell<usize>>,
    output_rate: u32,
    device_name: String,
    resample_pos: f64,
    resample_last: f32,
    fill_avg: f32,
    scratch: Vec<f32>,
    /// Kept alive for the node's lifetime: dropping a closure detaches it.
    _callbacks: Vec<Closure<dyn FnMut(MessageEvent)>>,
    _gesture: Option<Closure<dyn FnMut(web_sys::Event)>>,
}

impl AudioOutput {
    pub fn list_output_devices() -> Vec<String> {
        if let Ok(g) = TEST_OUTPUT_DEVICES.lock() {
            if let Some(devices) = g.as_ref() {
                return devices.clone();
            }
        }
        // A tab cannot enumerate output devices without a media permission
        // prompt, and routing is the browser's job anyway.
        vec!["Browser audio".to_string()]
    }

    pub fn try_open_default(iq_rate: u32) -> Option<Self> {
        Self::open(iq_rate)
    }

    /// The browser picks the device, so a name cannot select one.
    pub fn try_open_named(_name: &str, iq_rate: u32) -> Option<Self> {
        Self::open(iq_rate)
    }

    fn open(_iq_rate: u32) -> Option<Self> {
        let ctx = AudioContext::new()
            .map_err(|e| crate::log::error(format!("audio: no AudioContext: {e:?}")))
            .ok()?;
        let output_rate = ctx.sample_rate().round().max(1.0) as u32;

        let node: Rc<std::cell::RefCell<Option<AudioWorkletNode>>> =
            Rc::new(std::cell::RefCell::new(None));
        let buffered = Rc::new(Cell::new(0usize));
        let mut callbacks: Vec<Closure<dyn FnMut(MessageEvent)>> = Vec::new();

        match Self::spawn_worklet(&ctx, Rc::clone(&node), Rc::clone(&buffered)) {
            Ok(cb) => callbacks.push(cb),
            Err(e) => {
                crate::log::error(format!("audio: worklet setup failed: {e:?}"));
                return None;
            }
        }

        let gesture = Self::resume_on_first_gesture(&ctx);
        let _ = ctx.resume();

        crate::log::info(format!("audio: WebAudio @ {output_rate} Hz"));
        Some(Self {
            ctx,
            node,
            buffered,
            output_rate,
            device_name: "Browser audio".to_string(),
            resample_pos: 0.0,
            resample_last: 0.0,
            fill_avg: 0.5,
            scratch: Vec::with_capacity(RING_CAPACITY),
            _callbacks: callbacks,
            _gesture: gesture,
        })
    }

    /// Compile the worklet from a blob URL and connect it once it resolves.
    fn spawn_worklet(
        ctx: &AudioContext,
        node_slot: Rc<std::cell::RefCell<Option<AudioWorkletNode>>>,
        buffered: Rc<Cell<usize>>,
    ) -> Result<Closure<dyn FnMut(MessageEvent)>, JsValue> {
        let parts = js_sys::Array::new();
        parts.push(&JsValue::from_str(WORKLET_SOURCE));
        let opts = web_sys::BlobPropertyBag::new();
        opts.set_type("application/javascript");
        let blob = web_sys::Blob::new_with_str_sequence_and_options(&parts, &opts)?;
        let url = web_sys::Url::create_object_url_with_blob(&blob)?;

        // Depth reports from the audio thread drive the drift servo.
        let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |ev: MessageEvent| {
            if let Some(n) = ev.data().as_f64() {
                buffered.set(n.max(0.0) as usize);
            }
        });

        let promise = ctx.audio_worklet()?.add_module(&url)?;
        let ctx_for_then = ctx.clone();
        let handler_ref: JsValue = on_message.as_ref().clone();
        wasm_bindgen_futures::spawn_local(async move {
            if let Err(e) = wasm_bindgen_futures::JsFuture::from(promise).await {
                crate::log::error(format!("audio: worklet module rejected: {e:?}"));
                web_sys::Url::revoke_object_url(&url).ok();
                return;
            }
            web_sys::Url::revoke_object_url(&url).ok();
            match AudioWorkletNode::new(&ctx_for_then, "hfsdr-sink") {
                Ok(node) => {
                    if let Ok(port) = node.port() {
                        port.set_onmessage(Some(handler_ref.unchecked_ref()));
                    }
                    if let Err(e) = node.connect_with_audio_node(&ctx_for_then.destination()) {
                        crate::log::error(format!("audio: worklet connect failed: {e:?}"));
                        return;
                    }
                    *node_slot.borrow_mut() = Some(node);
                }
                Err(e) => crate::log::error(format!("audio: worklet node failed: {e:?}")),
            }
        });
        Ok(on_message)
    }

    /// Autoplay policy keeps the context suspended until the user acts, and a
    /// suspended context makes no sound however much we push into it.
    fn resume_on_first_gesture(ctx: &AudioContext) -> Option<Closure<dyn FnMut(web_sys::Event)>> {
        let document = web_sys::window()?.document()?;
        let ctx_cb = ctx.clone();
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
            let _ = ctx_cb.resume();
        });
        for event in ["pointerdown", "keydown", "touchstart"] {
            let _ = document
                .add_event_listener_with_callback(event, cb.as_ref().unchecked_ref());
        }
        Some(cb)
    }

    pub fn skip_seconds(&self, secs: f32) {
        if secs <= 0.0 {
            return;
        }
        let n = (secs * self.output_rate as f32).round() as usize;
        if n == 0 {
            return;
        }
        if let Some(node) = self.node.borrow().as_ref() {
            let msg = js_sys::Object::new();
            let _ = js_sys::Reflect::set(
                &msg,
                &JsValue::from_str("skip"),
                &JsValue::from_f64(n as f64),
            );
            if let Ok(port) = node.port() {
                let _ = port.post_message(&msg);
            }
        }
    }

    pub fn output_rate(&self) -> u32 {
        self.output_rate
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// Push mono samples at `source_rate`, resampled onto the browser's clock.
    ///
    /// Same contract as the desktop backend, including that equal nominal rates
    /// are not special-cased: the browser's clock and the SDR's still differ.
    pub fn push(&mut self, mono: &[f32], source_rate: u32, volume: f32) {
        if mono.is_empty() || volume <= 0.0 {
            return;
        }
        // Retry the resume here too: the gesture may have happened over a DOM
        // node that stopped propagation before our document listener saw it.
        if self.ctx.state() == AudioContextState::Suspended {
            let _ = self.ctx.resume();
        }

        let queued = self.buffered.get();
        let occupancy = (queued as f32 / RING_CAPACITY as f32).clamp(0.0, 1.0);
        self.fill_avg = smooth_fill(self.fill_avg, occupancy);
        let step = servo_step(source_rate, self.output_rate, self.fill_avg);

        self.scratch.clear();
        let headroom = RING_CAPACITY.saturating_sub(queued).max(1);
        let (pos, last) = resample_push(
            &mut BlockSink {
                out: &mut self.scratch,
                limit: headroom,
            },
            mono,
            step,
            self.resample_pos,
            self.resample_last,
            volume,
        );
        self.resample_pos = pos;
        self.resample_last = last;

        if self.scratch.is_empty() {
            return;
        }
        let Some(node) = self.node.borrow().as_ref().cloned() else {
            // Worklet still compiling: drop rather than queue, so audio starts
            // live instead of replaying a backlog.
            return;
        };
        let samples = js_sys::Float32Array::from(self.scratch.as_slice());
        let msg = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&msg, &JsValue::from_str("samples"), &samples);
        if let Ok(port) = node.port() {
            let _ = port.post_message(&msg);
        }
    }
}

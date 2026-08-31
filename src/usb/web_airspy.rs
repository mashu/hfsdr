//! An Airspy HF+ reached from a browser tab, as an [`IqSource`].
//!
//! Device selection cannot happen inside [`IqSource::start`]. `requestDevice`
//! is async and must be called from a user gesture — a click — so the browser
//! chooser appears when the user asks for it, not when the engine happens to
//! connect. The granted device is therefore held here between the two, and
//! `start` only opens the stream on a radio that is already in hand.
//!
//! Single-threaded by construction: a wasm tab has one thread, so the handle
//! is a `thread_local` rather than a lock. That is also why the sample pump is
//! a spawned task rather than a thread — it yields to the event loop between
//! transfers, so a stall in the radio cannot freeze the UI.

use std::cell::RefCell;
use std::rc::Rc;

use rtrb::{Consumer, RingBuffer};

use crate::source::{IqSource, Result as SourceResult, SourceError};
use crate::Complex32;

use super::airspyhf;
use super::web_transport::WebUsbDevice;

thread_local! {
    /// The radio the user has granted this origin, if any.
    static GRANTED: RefCell<Option<Rc<Granted>>> = const { RefCell::new(None) };
}

struct Granted {
    device: WebUsbDevice,
    rates: Vec<u32>,
}

/// Whether this browser can reach USB devices at all.
pub fn is_supported() -> bool {
    super::web_transport::is_supported()
}

/// Whether a radio has been granted and opened.
pub fn have_device() -> bool {
    GRANTED.with(|g| g.borrow().is_some())
}

/// The sample rates the granted radio reports, empty if there is none.
pub fn device_rates() -> Vec<u32> {
    GRANTED.with(|g| {
        g.borrow()
            .as_ref()
            .map(|d| d.rates.clone())
            .unwrap_or_default()
    })
}

/// Show the browser's device chooser and open what the user picks.
///
/// Must be called from a click handler. Returns `Ok(false)` when the chooser
/// is dismissed — a normal outcome, not a failure.
pub async fn choose_device() -> Result<bool, String> {
    let Some(device) = WebUsbDevice::request().await.map_err(|e| e.to_string())? else {
        return Ok(false);
    };
    adopt(device).await.map(|()| true)
}

/// Re-open a radio this origin was already granted, without prompting.
///
/// The permission persists, so a returning visitor gets their radio back with
/// no chooser. Silent when there is nothing granted.
pub async fn reopen_granted() -> Result<bool, String> {
    let Some(device) = WebUsbDevice::already_granted()
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(false);
    };
    adopt(device).await.map(|()| true)
}

async fn adopt(device: WebUsbDevice) -> Result<(), String> {
    // Identify before storing: a device that will not answer its open sequence
    // is not usable, and finding that out now gives the user an error next to
    // the button they just pressed rather than later, next to Connect.
    let rates = device.identify().await.map_err(|e| e.to_string())?;
    GRANTED.with(|g| {
        *g.borrow_mut() = Some(Rc::new(Granted { device, rates }));
    });
    Ok(())
}

/// Release the radio so another tab, or the desktop build, can use it.
pub fn release() {
    if let Some(granted) = GRANTED.with(|g| g.borrow_mut().take()) {
        wasm_bindgen_futures::spawn_local(async move {
            granted.device.close().await;
        });
    }
}

/// How much IQ to buffer between the pump and the engine.
///
/// One second at the highest rate the HF+ offers. The pump cannot block, so
/// anything it cannot fit is dropped and counted rather than stalling the
/// event loop.
pub fn iq_ring_capacity(sample_rate: u32) -> usize {
    (sample_rate.max(1) as usize).next_power_of_two()
}

/// The granted radio, as a source the engine can drive.
pub struct WebAirspySource {
    rates: Vec<u32>,
    rate: u32,
    rate_index: u16,
    frequency_hz: f64,
    streaming: Rc<RefCell<bool>>,
    dropped: Rc<RefCell<u64>>,
}

impl WebAirspySource {
    /// Build a source over the radio the user already granted.
    pub fn new() -> Result<Self, String> {
        let rates = device_rates();
        if rates.is_empty() {
            return Err("no Airspy HF+ has been chosen yet".into());
        }
        let rate = rates[0];
        Ok(Self {
            rates,
            rate,
            rate_index: 0,
            frequency_hz: 0.0,
            streaming: Rc::new(RefCell::new(false)),
            dropped: Rc::new(RefCell::new(0)),
        })
    }

    fn granted(&self) -> SourceResult<Rc<Granted>> {
        GRANTED
            .with(|g| g.borrow().clone())
            .ok_or(SourceError::NotFound)
    }
}

impl IqSource for WebAirspySource {
    fn sample_rates(&self) -> Vec<u32> {
        self.rates.clone()
    }

    fn sample_rate(&self) -> u32 {
        self.rate
    }

    fn set_sample_rate(&mut self, sr: u32) -> SourceResult<()> {
        // Selection is by position in the firmware's list — the field is 16
        // bits and the rates are not. See `airspyhf::select_sample_rate`.
        let index = self
            .rates
            .iter()
            .position(|&r| r == sr)
            .ok_or_else(|| SourceError::Unsupported(format!("{sr} Sa/s is not one of the radio's rates")))?;
        self.rate = sr;
        self.rate_index = index as u16;
        let granted = self.granted()?;
        let request = airspyhf::select_sample_rate(self.rate_index);
        wasm_bindgen_futures::spawn_local(async move {
            if let Err(e) = granted.device.control(&request).await {
                crate::log::error(format!("airspy: could not select sample rate: {e}"));
            }
        });
        Ok(())
    }

    fn tune(&mut self, hz: f64) -> SourceResult<()> {
        self.frequency_hz = hz;
        let granted = self.granted()?;
        let request = airspyhf::set_frequency(hz.max(0.0) as u32);
        wasm_bindgen_futures::spawn_local(async move {
            if let Err(e) = granted.device.control(&request).await {
                crate::log::error(format!("airspy: could not tune: {e}"));
            }
        });
        Ok(())
    }

    fn frequency(&self) -> f64 {
        self.frequency_hz
    }

    fn start(&mut self) -> SourceResult<Consumer<Complex32>> {
        let granted = self.granted()?;
        let (mut producer, consumer) = RingBuffer::new(iq_ring_capacity(self.rate));
        *self.streaming.borrow_mut() = true;
        let streaming = Rc::clone(&self.streaming);
        let dropped = Rc::clone(&self.dropped);

        wasm_bindgen_futures::spawn_local(async move {
            if let Err(e) = granted
                .device
                .control(&airspyhf::set_receiver_mode(true))
                .await
            {
                crate::log::error(format!("airspy: could not start the stream: {e}"));
                *streaming.borrow_mut() = false;
                return;
            }
            let mut samples: Vec<f32> = Vec::with_capacity(airspyhf::TRANSFER_SAMPLES * 2);
            while *streaming.borrow() {
                match granted.device.read_samples().await {
                    Ok(bytes) if bytes.is_empty() => continue,
                    Ok(bytes) => {
                        samples.clear();
                        airspyhf::decode_samples(&bytes, &mut samples);
                        for pair in samples.chunks_exact(2) {
                            // Never block the event loop: a consumer that has
                            // fallen behind loses samples, counted here, rather
                            // than stalling the tab.
                            if producer.push(Complex32::new(pair[0], pair[1])).is_err() {
                                *dropped.borrow_mut() += 1;
                            }
                        }
                    }
                    Err(e) => {
                        crate::log::error(format!("airspy: stream ended: {e}"));
                        break;
                    }
                }
            }
            let _ = granted
                .device
                .control(&airspyhf::set_receiver_mode(false))
                .await;
            *streaming.borrow_mut() = false;
        });

        Ok(consumer)
    }

    fn stop(&mut self) -> SourceResult<()> {
        // The pump notices on its next turn and stops the receiver itself, so
        // this is idempotent and does not need to await anything.
        *self.streaming.borrow_mut() = false;
        Ok(())
    }

    fn dropped_samples(&self) -> u64 {
        *self.dropped.borrow()
    }

    fn is_streaming(&self) -> bool {
        *self.streaming.borrow()
    }
}

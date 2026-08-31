//! WebUSB transport: executes [`ControlRequest`]s from a browser tab.
//!
//! The counterpart to [`super::nusb_transport`], and deliberately just as thin.
//! It knows how to move bytes through `navigator.usb` and nothing about radios:
//! the commands, the replies, the sample format and the open ordering all live
//! in [`super::airspyhf`], are shared with the native path, and are tested on
//! the host with no radio present.
//!
//! A tab really can talk to a USB radio — no driver, no installer. What it
//! needs instead is a user gesture: `requestDevice` shows the browser's own
//! chooser and must be called from a click. The grant is remembered per origin
//! afterwards, so `getDevices` finds the radio on later visits without asking
//! again.
//!
//! Chromium only. Firefox and Safari have both filed negative standards
//! positions on WebUSB and neither implements it, so [`is_supported`] is what
//! the UI should ask before offering any of this.

use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{UsbControlTransferParameters, UsbDevice, UsbDeviceFilter, UsbDeviceRequestOptions,
              UsbInTransferResult, UsbRecipient, UsbRequestType};

use super::{airspyhf, ControlRequest, Direction};

/// Whether this browser exposes WebUSB at all.
///
/// Asked before offering a USB source, so a Firefox or Safari user is told
/// their browser cannot do this rather than being handed a button that fails.
pub fn is_supported() -> bool {
    web_sys::window()
        .map(|w| !w.navigator().usb().is_undefined())
        .unwrap_or(false)
}

/// An opened WebUSB device with its interface claimed.
pub struct WebUsbDevice {
    device: UsbDevice,
}

impl WebUsbDevice {
    /// Ask the user to pick a device, filtered to the Airspy HF+.
    ///
    /// Must be called from a user gesture — a click handler — or the browser
    /// rejects it. Returns `None` when the chooser is dismissed, which is a
    /// normal outcome and not an error worth reporting.
    pub async fn request() -> Result<Option<Self>, WebUsbError> {
        let window = web_sys::window().ok_or(WebUsbError::NoWindow)?;
        let filter = UsbDeviceFilter::new();
        filter.set_vendor_id(airspyhf::VENDOR_ID);
        filter.set_product_id(airspyhf::PRODUCT_ID);
        let options = UsbDeviceRequestOptions::new(&[filter]);

        let picked = JsFuture::from(window.navigator().usb().request_device(&options)).await;
        let device = match picked {
            Ok(d) => d.dyn_into::<UsbDevice>().map_err(|_| WebUsbError::NotADevice)?,
            // A dismissed chooser rejects the promise. So does a call outside a
            // gesture, and the two are not distinguishable here, so neither is
            // reported as a failure — the user simply has no device yet.
            Err(_) => return Ok(None),
        };
        Self::open(device).await.map(Some)
    }

    /// Re-open a device the user already granted, without prompting.
    ///
    /// The permission persists per origin, so a returning visitor gets their
    /// radio back with no chooser. Callable from anywhere, including a worker.
    pub async fn already_granted() -> Result<Option<Self>, WebUsbError> {
        let window = web_sys::window().ok_or(WebUsbError::NoWindow)?;
        let list = JsFuture::from(window.navigator().usb().get_devices())
            .await
            .map_err(|e| WebUsbError::Enumerate(describe(&e)))?;
        let devices = js_sys::Array::from(&list);
        for value in devices.iter() {
            let Ok(device) = value.dyn_into::<UsbDevice>() else {
                continue;
            };
            if device.vendor_id() == airspyhf::VENDOR_ID
                && device.product_id() == airspyhf::PRODUCT_ID
            {
                return Self::open(device).await.map(Some);
            }
        }
        Ok(None)
    }

    async fn open(device: UsbDevice) -> Result<Self, WebUsbError> {
        JsFuture::from(device.open())
            .await
            .map_err(|e| WebUsbError::Open(describe(&e)))?;
        // A device that has never been configured has no interfaces to claim.
        // Selecting configuration 1 is a no-op once it is already selected.
        if device.configuration().is_none() {
            JsFuture::from(device.select_configuration(1))
                .await
                .map_err(|e| WebUsbError::Configure(describe(&e)))?;
        }
        JsFuture::from(device.claim_interface(0))
            .await
            .map_err(|e| WebUsbError::Claim(describe(&e)))?;
        Ok(Self { device })
    }

    /// Perform one control transfer.
    ///
    /// Async, so this cannot implement [`super::UsbControl`] — that trait is
    /// the blocking shape `nusb` needs. Both sides execute the same
    /// [`ControlRequest`] values regardless.
    pub async fn control(&self, request: &ControlRequest) -> Result<Vec<u8>, WebUsbError> {
        let params = UsbControlTransferParameters::new(
            request.index,
            UsbRecipient::Device,
            request.request,
            UsbRequestType::Vendor,
            request.value,
        );
        match request.direction {
            Direction::In => {
                let result = JsFuture::from(
                    self.device.control_transfer_in(&params, request.length),
                )
                .await
                .map_err(|e| WebUsbError::Transfer(describe(&e)))?
                .dyn_into::<UsbInTransferResult>()
                .map_err(|_| WebUsbError::NotADevice)?;
                Ok(result
                    .data()
                    .map(|view| js_sys::Uint8Array::new(&view.buffer()).to_vec())
                    .unwrap_or_default())
            }
            Direction::Out => {
                let mut data = request.data.clone();
                JsFuture::from(
                    self.device
                        .control_transfer_out_with_u8_slice(&params, &mut data)
                        .map_err(|e| WebUsbError::Transfer(describe(&e)))?,
                )
                .await
                .map_err(|e| WebUsbError::Transfer(describe(&e)))?;
                Ok(Vec::new())
            }
        }
    }

    /// Read one bulk transfer from the sample endpoint.
    pub async fn read_samples(&self) -> Result<Vec<u8>, WebUsbError> {
        let result = JsFuture::from(
            self.device
                .transfer_in(airspyhf::BULK_IN_ENDPOINT & 0x7F, airspyhf::TRANSFER_BYTES as u32),
        )
        .await
        .map_err(|e| WebUsbError::Transfer(describe(&e)))?
        .dyn_into::<UsbInTransferResult>()
        .map_err(|_| WebUsbError::NotADevice)?;
        Ok(result
            .data()
            .map(|view| js_sys::Uint8Array::new(&view.buffer()).to_vec())
            .unwrap_or_default())
    }

    /// Bring the device up and report the sample rates it offers.
    ///
    /// Drives [`airspyhf::OpenSequence`], so the ordering is the one the
    /// blocking path uses and the host tests cover — not a second copy of it.
    pub async fn identify(&self) -> Result<Vec<u32>, WebUsbError> {
        let mut sequence = airspyhf::OpenSequence::new();
        while let Some(request) = sequence.next_request() {
            let reply = self.control(&request).await?;
            if let Some(rates) = sequence
                .accept(&reply)
                .map_err(|e| WebUsbError::Protocol(e.to_string()))?
            {
                return Ok(rates);
            }
        }
        Err(WebUsbError::Protocol(
            "the device stopped answering before reporting its sample rates".into(),
        ))
    }

    /// Release the device so another tab, or the desktop build, can have it.
    pub async fn close(&self) {
        let _ = JsFuture::from(self.device.release_interface(0)).await;
        let _ = JsFuture::from(self.device.close()).await;
    }
}

/// Pull a readable reason out of a rejected promise.
fn describe(err: &JsValue) -> String {
    err.as_string()
        .or_else(|| {
            js_sys::Reflect::get(err, &"message".into())
                .ok()
                .and_then(|m| m.as_string())
        })
        .unwrap_or_else(|| "the browser gave no reason".into())
}

/// Why a WebUSB operation failed.
#[derive(Clone, Debug)]
pub enum WebUsbError {
    NoWindow,
    NotADevice,
    Enumerate(String),
    Open(String),
    Configure(String),
    Claim(String),
    Transfer(String),
    Protocol(String),
}

impl std::fmt::Display for WebUsbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoWindow => write!(f, "no browser window"),
            Self::NotADevice => write!(f, "the browser returned something that is not a USB device"),
            Self::Enumerate(e) => write!(f, "could not list granted devices: {e}"),
            // The permission cases are the ones users hit, and the browser's
            // own message for them says only "Access denied".
            Self::Open(e) => write!(
                f,
                "could not open the radio: {e} — on Linux this usually means a \
                 udev rule is missing, and on Windows that it is not bound to WinUSB"
            ),
            Self::Configure(e) => write!(f, "could not configure the radio: {e}"),
            Self::Claim(e) => write!(
                f,
                "could not claim the radio: {e} — another tab, another program, \
                 or a kernel driver already has it"
            ),
            Self::Transfer(e) => write!(f, "usb transfer failed: {e}"),
            Self::Protocol(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for WebUsbError {}

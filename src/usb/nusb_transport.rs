//! Native USB transport: executes [`ControlRequest`]s over `nusb`.
//!
//! This is the half of the USB story that is written twice — once here for the
//! desktop and once over WebUSB for the browser — and it is deliberately the
//! smallest half. It knows how to move bytes and nothing about radios: no
//! command numbers, no sample format, no ordering. Those live in
//! [`super::airspyhf`], are shared by both transports, and are tested without
//! either.
//!
//! `nusb` is pure Rust, so this links statically. Compared with the dlopen
//! path in [`crate::sdr_ffi`] there is no vendor `.so` to find, no ABI to
//! guess at, and nothing to install alongside the binary.

use std::time::Duration;

use nusb::transfer::{ControlIn, ControlOut, ControlType, Recipient};
use nusb::MaybeFuture;

use super::{ControlRequest, Direction, UsbControl};

/// How long a control transfer may take before it is abandoned.
///
/// Control transfers on these devices answer in microseconds; a second means
/// the device has stopped talking, and waiting longer only delays saying so.
const CONTROL_TIMEOUT: Duration = Duration::from_secs(1);

/// An opened USB device, ready to carry control transfers.
pub struct NusbTransport {
    interface: nusb::Interface,
}

impl NusbTransport {
    /// Claim interface 0 of the first device matching `vendor_id`/`product_id`.
    ///
    /// Interface 0 is where these radios put both the vendor control endpoint
    /// and the bulk IN endpoint; a device with a second interface is not one
    /// of them.
    pub fn open(vendor_id: u16, product_id: u16) -> Result<Self, TransportError> {
        let info = nusb::list_devices()
            .wait()
            .map_err(TransportError::Enumerate)?
            .find(|d| d.vendor_id() == vendor_id && d.product_id() == product_id)
            .ok_or(TransportError::NotFound { vendor_id, product_id })?;
        let device = info.open().wait().map_err(TransportError::Open)?;
        let interface = device.claim_interface(0).wait().map_err(TransportError::Claim)?;
        Ok(Self { interface })
    }

    /// The bulk IN endpoint, for the caller to stream from.
    pub fn interface(&self) -> &nusb::Interface {
        &self.interface
    }
}

impl UsbControl for NusbTransport {
    type Error = TransportError;

    fn control(&self, request: &ControlRequest) -> Result<Vec<u8>, Self::Error> {
        match request.direction {
            Direction::In => {
                let data = self
                    .interface
                    .control_in(
                        ControlIn {
                            control_type: ControlType::Vendor,
                            recipient: Recipient::Device,
                            request: request.request,
                            value: request.value,
                            index: request.index,
                            length: request.length,
                        },
                        CONTROL_TIMEOUT,
                    )
                    .wait()
                    .map_err(TransportError::Transfer)?;
                Ok(data)
            }
            Direction::Out => {
                self.interface
                    .control_out(
                        ControlOut {
                            control_type: ControlType::Vendor,
                            recipient: Recipient::Device,
                            request: request.request,
                            value: request.value,
                            index: request.index,
                            data: &request.data,
                        },
                        CONTROL_TIMEOUT,
                    )
                    .wait()
                    .map_err(TransportError::Transfer)?;
                Ok(Vec::new())
            }
        }
    }
}

/// Why a native USB operation failed.
#[derive(Debug)]
pub enum TransportError {
    Enumerate(nusb::Error),
    NotFound { vendor_id: u16, product_id: u16 },
    Open(nusb::Error),
    Claim(nusb::Error),
    Transfer(nusb::transfer::TransferError),
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Enumerate(e) => write!(f, "could not list USB devices: {e}"),
            Self::NotFound { vendor_id, product_id } => write!(
                f,
                "no device {vendor_id:04x}:{product_id:04x} is attached"
            ),
            // The permission cases are the ones users actually hit, and the
            // error the OS returns for them says only "access denied".
            Self::Open(e) => write!(
                f,
                "could not open the device: {e} \
                 (on Linux this usually means a udev rule is missing; \
                 on Windows, that the device is not bound to WinUSB)"
            ),
            Self::Claim(e) => write!(
                f,
                "could not claim the interface: {e} \
                 (another process, or a kernel driver, already has it)"
            ),
            Self::Transfer(e) => write!(f, "usb transfer failed: {e}"),
        }
    }
}

impl std::error::Error for TransportError {}

//! USB device support that is independent of who moves the bytes.
//!
//! The same radio has to be driven from two places that share no USB API: a
//! desktop build talking to `nusb`, and a browser tab talking to WebUSB. Those
//! differ in every way that a driver does not care about — one is blocking and
//! returns `io::Result`, the other returns JavaScript promises — and in no way
//! that it does.
//!
//! So the protocol is written once, as values: [`ControlRequest`] describes a
//! transfer without performing it, and the per-device modules turn radio
//! operations into those values and turn the replies back into numbers. A
//! transport is then a small adapter that executes requests, and gets to be
//! the only part written twice.
//!
//! This is the shape [`crate::kiwi::session`] already uses for the KiwiSDR
//! protocol, for the same reason and with the same payoff: the part with the
//! logic in it is ordinary testable code that needs neither a socket nor a
//! radio, so it is tested on the host with no hardware present.

pub mod airspyhf;

// The native transport. Gated because it is the only part that pulls in a USB
// crate; the protocol above compiles everywhere, including wasm32.
#[cfg(all(feature = "airspyhf-usb", not(target_arch = "wasm32")))]
pub mod nusb_transport;

// The browser transport. WebUSB is async and Chromium-only, so it neither
// implements [`UsbControl`] nor exists off wasm32 — but it executes the same
// [`ControlRequest`] values and drives the same [`airspyhf::OpenSequence`].
#[cfg(target_arch = "wasm32")]
pub mod web_transport;

// The granted radio as an IqSource. Separate from the transport because
// holding the device between the user's click and the engine's connect is a
// concern of its own.
#[cfg(all(target_arch = "wasm32", feature = "gui-web"))]
pub mod web_airspy;

/// Which way the data of a control transfer flows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Host to device. `bmRequestType` bit 7 clear.
    Out,
    /// Device to host. `bmRequestType` bit 7 set.
    In,
}

/// A USB control transfer on the vendor/device pair, described but not sent.
///
/// Only vendor requests addressed to the device are represented, because that
/// is all an SDR uses; `bmRequestType` is therefore derived rather than
/// carried, and cannot be set to a combination the driver never wants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlRequest {
    pub direction: Direction,
    /// `bRequest` — the device's vendor command number.
    pub request: u8,
    pub value: u16,
    pub index: u16,
    /// For [`Direction::In`], how many bytes to read. For [`Direction::Out`],
    /// the length of [`ControlRequest::data`], kept so both directions can be
    /// checked the same way.
    pub length: u16,
    /// Payload for [`Direction::Out`]; always empty for [`Direction::In`].
    pub data: Vec<u8>,
}

impl ControlRequest {
    /// A write with no payload — the whole command is in `value`/`index`.
    pub fn out(request: u8, value: u16, index: u16) -> Self {
        Self { direction: Direction::Out, request, value, index, length: 0, data: Vec::new() }
    }

    /// A write carrying a payload.
    pub fn out_with_data(request: u8, value: u16, index: u16, data: Vec<u8>) -> Self {
        Self {
            direction: Direction::Out,
            request,
            value,
            index,
            length: data.len() as u16,
            data,
        }
    }

    /// A read of exactly `length` bytes.
    pub fn read(request: u8, value: u16, index: u16, length: u16) -> Self {
        Self { direction: Direction::In, request, value, index, length, data: Vec::new() }
    }

    /// `bmRequestType`: vendor type, device recipient, direction in bit 7.
    ///
    /// Spelled out rather than taken from a constant in either USB crate, so
    /// the byte is the same one on both transports and neither backend's
    /// naming leaks into the protocol modules.
    pub fn request_type(&self) -> u8 {
        const VENDOR_DEVICE: u8 = 0x40;
        match self.direction {
            Direction::Out => VENDOR_DEVICE,
            Direction::In => VENDOR_DEVICE | 0x80,
        }
    }
}

/// Executes control transfers for a device that is already open.
///
/// Deliberately not async: `nusb` blocks on a driver thread and WebUSB awaits
/// a promise, and an async trait would force one of them to pretend. The
/// browser transport instead runs this same request sequence in its own task
/// and hands the driver the decoded results.
pub trait UsbControl {
    type Error;

    /// Perform `request`, returning the bytes read for [`Direction::In`] and
    /// an empty vector for [`Direction::Out`].
    fn control(&self, request: &ControlRequest) -> Result<Vec<u8>, Self::Error>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The direction bit is the only part of `bmRequestType` that varies, and
    /// getting it backwards stalls the endpoint rather than failing loudly.
    #[test]
    fn request_type_sets_only_the_direction_bit() {
        assert_eq!(ControlRequest::out(1, 0, 0).request_type(), 0x40);
        assert_eq!(ControlRequest::read(1, 0, 0, 4).request_type(), 0xC0);
    }

    /// `length` drives how many bytes a transport moves, so a payload that
    /// does not set it would be silently truncated to nothing.
    #[test]
    fn out_with_data_reports_its_payload_length() {
        let req = ControlRequest::out_with_data(2, 0, 0, vec![1, 2, 3, 4]);
        assert_eq!(req.length, 4);
        assert_eq!(req.data, vec![1, 2, 3, 4]);

        let empty = ControlRequest::out(2, 0, 0);
        assert_eq!(empty.length, 0);
        assert!(empty.data.is_empty());
    }

    /// A read carries no payload: a transport that wrote `data` for an IN
    /// transfer would corrupt the setup packet.
    #[test]
    fn read_carries_no_payload() {
        let req = ControlRequest::read(3, 0, 8, 32);
        assert!(req.data.is_empty());
        assert_eq!(req.length, 32);
        assert_eq!(req.direction, Direction::In);
    }
}

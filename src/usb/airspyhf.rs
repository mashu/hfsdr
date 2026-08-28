//! Airspy HF+ wire protocol.
//!
//! Command numbers, transfer geometry and sample format follow libairspyhf
//! (BSD-3-Clause, <https://github.com/airspy/airspyhf>); this is an
//! independent Rust implementation of the same protocol, not a translation of
//! that code.
//!
//! The HF+ is the right first target for a native-Rust USB driver here: it is
//! an HF radio rather than a VHF one needing a converter, it streams at rates
//! a browser tab can keep up with, and its vendor library is BSD rather than
//! GPL, so reimplementing the protocol raises no licensing question.
//!
//! Everything here is pure: requests in, bytes out. No device is opened and no
//! transport is named, which is what lets it be tested on a machine with no
//! radio attached.

use super::ControlRequest;
#[cfg(test)]
use super::Direction;

/// Atmel VID used by the HF+ bootloader and firmware.
pub const VENDOR_ID: u16 = 0x03EB;
/// Product ID of the Airspy HF+ / HF+ Discovery.
pub const PRODUCT_ID: u16 = 0x800C;

/// Bulk IN endpoint carrying the sample stream.
pub const BULK_IN_ENDPOINT: u8 = 0x81;

/// Complex samples per bulk transfer.
pub const TRANSFER_SAMPLES: usize = 4096;

/// Bytes per bulk transfer: one i16 pair per sample.
pub const TRANSFER_BYTES: usize = TRANSFER_SAMPLES * BYTES_PER_SAMPLE;

/// An i16 real and an i16 imaginary part, little-endian.
pub const BYTES_PER_SAMPLE: usize = 4;

/// Transfers to keep queued.
///
/// A bulk endpoint stops delivering between the completion of one transfer and
/// the submission of the next, so a single in-flight transfer drops samples on
/// every round trip. Eight is what libairspyhf queues, and at 768 kSPS covers
/// ~43 ms of stream — enough that a scheduling hiccup does not show up as a
/// gap.
pub const IN_FLIGHT_TRANSFERS: usize = 8;

/// Vendor command numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Request {
    ReceiverMode = 1,
    SetFreq = 2,
    GetSamplerates = 3,
    SetSamplerate = 4,
    GetSerialNoBoardId = 7,
    GetVersionString = 9,
    SetAgc = 10,
    SetAgcThreshold = 11,
    SetAtt = 12,
    SetLna = 13,
}

/// Start or stop the sample stream.
///
/// The mode rides in `value`, so this is a bare setup packet with no payload.
pub fn set_receiver_mode(on: bool) -> ControlRequest {
    ControlRequest::out(Request::ReceiverMode as u8, u16::from(on), 0)
}

/// Tune to `hz`, as a little-endian u32 payload.
///
/// The frequency does not fit in the 16-bit `value`/`index` fields, which is
/// why this one command carries data at all.
pub fn set_frequency(hz: u32) -> ControlRequest {
    ControlRequest::out_with_data(Request::SetFreq as u8, 0, 0, hz.to_le_bytes().to_vec())
}

/// Read how many sample rates the firmware offers.
///
/// `index == 0` means "the count", and the reply is a single u32; any other
/// value asks for that many rates. Same command, two meanings — see
/// [`read_sample_rates`].
pub fn read_sample_rate_count() -> ControlRequest {
    ControlRequest::read(Request::GetSamplerates as u8, 0, 0, 4)
}

/// Read `count` sample rates, as `count` little-endian u32s.
pub fn read_sample_rates(count: u16) -> ControlRequest {
    ControlRequest::read(
        Request::GetSamplerates as u8,
        0,
        count,
        count.saturating_mul(4),
    )
}

/// Select a sample rate *by its index* in [`read_sample_rates`].
///
/// The rate itself is passed in the 16-bit `index` field, which cannot hold
/// one: 768000 does not fit in a u16. The firmware takes a position in the
/// list, so a caller that passes 768000 selects nothing and the device keeps
/// whatever rate it had.
pub fn select_sample_rate(index: u16) -> ControlRequest {
    ControlRequest::out(Request::SetSamplerate as u8, 0, index)
}

/// Read the board id and serial number: 4 little-endian u32s.
pub fn read_serial_and_board_id() -> ControlRequest {
    ControlRequest::read(Request::GetSerialNoBoardId as u8, 0, 0, 16)
}

/// Enable or disable the HF AGC.
pub fn set_agc(on: bool) -> ControlRequest {
    ControlRequest::out(Request::SetAgc as u8, u16::from(on), 0)
}

/// Set the input attenuator, in 6 dB steps.
///
/// The firmware accepts 0..=8 (0..48 dB); a larger index is clamped here
/// rather than sent, because the device answers an out-of-range value by
/// stalling the control endpoint, which surfaces later as an unrelated
/// failure.
pub fn set_attenuation(step: u8) -> ControlRequest {
    ControlRequest::out(Request::SetAtt as u8, u16::from(step.min(MAX_ATT_STEP)), 0)
}

/// Highest attenuator index the firmware accepts: 8 × 6 dB = 48 dB.
pub const MAX_ATT_STEP: u8 = 8;

/// Enable or disable the preamp (+6 dB, compensated digitally).
pub fn set_lna(on: bool) -> ControlRequest {
    ControlRequest::out(Request::SetLna as u8, u16::from(on), 0)
}

/// Decoding failures that mean the reply was not what was asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// Fewer bytes came back than the fixed-size reply needs.
    Short { need: usize, got: usize },
    /// The firmware reported a rate of 0, which no caller can select.
    ZeroSampleRate,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Short { need, got } => {
                write!(f, "short reply: needed {need} bytes, got {got}")
            }
            Self::ZeroSampleRate => write!(f, "firmware reported a zero sample rate"),
        }
    }
}

impl std::error::Error for DecodeError {}

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice = bytes.get(offset..offset + 4)?;
    Some(u32::from_le_bytes(slice.try_into().ok()?))
}

/// Decode the reply to [`read_sample_rate_count`].
pub fn parse_sample_rate_count(bytes: &[u8]) -> Result<u32, DecodeError> {
    u32_at(bytes, 0).ok_or(DecodeError::Short { need: 4, got: bytes.len() })
}

/// Decode the reply to [`read_sample_rates`].
///
/// A trailing partial u32 is a short reply, not a rate: returning the rates
/// that did arrive would leave the caller selecting an index the firmware does
/// not have.
pub fn parse_sample_rates(bytes: &[u8], count: u16) -> Result<Vec<u32>, DecodeError> {
    let need = usize::from(count) * 4;
    if bytes.len() < need {
        return Err(DecodeError::Short { need, got: bytes.len() });
    }
    let mut rates = Vec::with_capacity(count.into());
    for i in 0..usize::from(count) {
        let rate = u32_at(bytes, i * 4).ok_or(DecodeError::Short { need, got: bytes.len() })?;
        if rate == 0 {
            return Err(DecodeError::ZeroSampleRate);
        }
        rates.push(rate);
    }
    Ok(rates)
}

/// Board id and serial number, as reported by the firmware.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoardId {
    pub part_id: [u32; 2],
    pub serial: u64,
}

/// Decode the reply to [`read_serial_and_board_id`].
pub fn parse_serial_and_board_id(bytes: &[u8]) -> Result<BoardId, DecodeError> {
    const NEED: usize = 16;
    if bytes.len() < NEED {
        return Err(DecodeError::Short { need: NEED, got: bytes.len() });
    }
    let part_id = [u32_at(bytes, 0).unwrap(), u32_at(bytes, 4).unwrap()];
    // The serial is a big-endian pair of little-endian words: the firmware
    // sends the high word first, and SDR# prints them in that order.
    let high = u64::from(u32_at(bytes, 8).unwrap());
    let low = u64::from(u32_at(bytes, 12).unwrap());
    Ok(BoardId { part_id, serial: (high << 32) | low })
}

/// Full-scale value of the device's i16 samples.
///
/// Dividing by 32768 rather than 32767 keeps the mapping a power of two, so
/// the conversion is exact and −32768 maps to exactly −1.0 instead of just
/// past it.
const FULL_SCALE: f32 = 32_768.0;

/// Decode a bulk transfer into interleaved f32 I/Q in −1.0..1.0, appending to
/// `out`.
///
/// Returns the number of complex samples appended. A trailing partial sample
/// is ignored rather than zero-padded: a bulk transfer can end mid-pair, and
/// inventing the missing half would put a discontinuity into the stream that
/// the FFT would show as a wideband click.
pub fn decode_samples(bytes: &[u8], out: &mut Vec<f32>) -> usize {
    let samples = bytes.len() / BYTES_PER_SAMPLE;
    out.reserve(samples * 2);
    for chunk in bytes.chunks_exact(BYTES_PER_SAMPLE) {
        let re = i16::from_le_bytes([chunk[0], chunk[1]]);
        let im = i16::from_le_bytes([chunk[2], chunk[3]]);
        out.push(f32::from(re) / FULL_SCALE);
        out.push(f32::from(im) / FULL_SCALE);
    }
    samples
}

/// Bring an opened device up to a known state and report what it can do.
///
/// Sequenced deliberately. The stream is stopped first because a device left
/// streaming by a previous process ignores configuration and keeps sending;
/// the rate list is read before anything selects a rate, because the selection
/// is an index into that list and there is no way to validate it otherwise.
///
/// Generic over the transport so this exact sequence — the part with the
/// ordering bug potential — is tested against a recording fake, on a machine
/// with no radio attached.
pub fn open_sequence<T: super::UsbControl>(usb: &T) -> Result<Vec<u32>, OpenError<T::Error>> {
    usb.control(&set_receiver_mode(false)).map_err(OpenError::Transport)?;

    let count = usb
        .control(&read_sample_rate_count())
        .map_err(OpenError::Transport)
        .and_then(|b| parse_sample_rate_count(&b).map_err(OpenError::Decode))?;
    let count = u16::try_from(count).map_err(|_| OpenError::ImplausibleRateCount(count))?;
    if count == 0 {
        return Err(OpenError::ImplausibleRateCount(0));
    }

    let rates = usb
        .control(&read_sample_rates(count))
        .map_err(OpenError::Transport)
        .and_then(|b| parse_sample_rates(&b, count).map_err(OpenError::Decode))?;
    Ok(rates)
}

/// Why bringing a device up failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenError<E> {
    /// The transfer itself failed.
    Transport(E),
    /// A reply was malformed.
    Decode(DecodeError),
    /// The firmware claimed a rate count that cannot be right. Asking for that
    /// many rates would be a multi-gigabyte control transfer, so this is
    /// rejected rather than attempted.
    ImplausibleRateCount(u32),
}

impl<E: std::fmt::Display> std::fmt::Display for OpenError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(e) => write!(f, "usb transfer failed: {e}"),
            Self::Decode(e) => write!(f, "{e}"),
            Self::ImplausibleRateCount(n) => {
                write!(f, "firmware reported {n} sample rates, which cannot be right")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The command numbers are the protocol. A wrong one is not a compile
    /// error and not a runtime error either — the device ignores it or does
    /// something else — so they are pinned against libairspyhf's enum.
    #[test]
    fn command_numbers_match_the_firmware() {
        assert_eq!(Request::ReceiverMode as u8, 1);
        assert_eq!(Request::SetFreq as u8, 2);
        assert_eq!(Request::GetSamplerates as u8, 3);
        assert_eq!(Request::SetSamplerate as u8, 4);
        assert_eq!(Request::GetSerialNoBoardId as u8, 7);
        assert_eq!(Request::SetAgc as u8, 10);
        assert_eq!(Request::SetAtt as u8, 12);
        assert_eq!(Request::SetLna as u8, 13);
    }

    #[test]
    fn receiver_mode_carries_the_flag_in_value() {
        let on = set_receiver_mode(true);
        assert_eq!(on.direction, Direction::Out);
        assert_eq!(on.request, 1);
        assert_eq!(on.value, 1);
        assert!(on.data.is_empty());
        assert_eq!(set_receiver_mode(false).value, 0);
    }

    /// The frequency is the one command with a payload, because 32 bits do not
    /// fit in the 16-bit setup fields. Byte order is the device's, not the
    /// host's.
    #[test]
    fn frequency_is_a_little_endian_u32_payload() {
        let req = set_frequency(14_100_000);
        assert_eq!(req.direction, Direction::Out);
        assert_eq!(req.data, 14_100_000u32.to_le_bytes().to_vec());
        assert_eq!(req.length, 4);
        assert_eq!(req.value, 0);
        assert_eq!(req.index, 0);
    }

    /// `GetSamplerates` means two different things depending on `index`, and
    /// asking for the count with a non-zero index returns rates instead.
    #[test]
    fn sample_rate_query_uses_index_zero_for_the_count() {
        let count = read_sample_rate_count();
        assert_eq!(count.index, 0);
        assert_eq!(count.length, 4);

        let rates = read_sample_rates(4);
        assert_eq!(rates.index, 4);
        assert_eq!(rates.length, 16, "four u32s");
        assert_eq!(rates.direction, Direction::In);
    }

    /// The rate is selected by position, not by value: the field is 16 bits
    /// and the rates are not.
    #[test]
    fn sample_rate_is_selected_by_index_not_by_rate() {
        let req = select_sample_rate(2);
        assert_eq!(req.index, 2);
        assert_eq!(req.direction, Direction::Out);
        assert!(req.data.is_empty());
        // 768000 would truncate to 12928 in a u16, so the API takes an index
        // and this is the type system's job, not a runtime check.
        assert!(u16::try_from(768_000u32).is_err());
    }

    #[test]
    fn parses_sample_rates_and_rejects_a_short_reply() {
        let body: Vec<u8> = [912_000u32, 768_000, 384_000, 256_000]
            .iter()
            .flat_map(|r| r.to_le_bytes())
            .collect();
        assert_eq!(
            parse_sample_rates(&body, 4).expect("rates"),
            vec![912_000, 768_000, 384_000, 256_000]
        );

        // One byte short of the last rate: returning the first three would let
        // the caller select an index the device does not have.
        assert_eq!(
            parse_sample_rates(&body[..15], 4),
            Err(DecodeError::Short { need: 16, got: 15 })
        );
        assert_eq!(parse_sample_rate_count(&body[..3]), Err(DecodeError::Short { need: 4, got: 3 }));
        assert_eq!(parse_sample_rate_count(&body).expect("count"), 912_000);
    }

    /// A zero rate cannot be divided by, and would reach the resampler as a
    /// division by zero rather than as a bad reply.
    #[test]
    fn a_zero_sample_rate_is_rejected() {
        let body: Vec<u8> = [768_000u32, 0].iter().flat_map(|r| r.to_le_bytes()).collect();
        assert_eq!(parse_sample_rates(&body, 2), Err(DecodeError::ZeroSampleRate));
    }

    #[test]
    fn parses_the_serial_number_high_word_first() {
        let mut body = Vec::new();
        body.extend_from_slice(&0x0000_0060u32.to_le_bytes());
        body.extend_from_slice(&0x0000_0000u32.to_le_bytes());
        body.extend_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
        body.extend_from_slice(&0x0123_4567u32.to_le_bytes());
        let id = parse_serial_and_board_id(&body).expect("board id");
        assert_eq!(id.serial, 0xDEAD_BEEF_0123_4567);
        assert_eq!(id.part_id, [0x60, 0]);
        assert_eq!(
            parse_serial_and_board_id(&body[..15]),
            Err(DecodeError::Short { need: 16, got: 15 })
        );
    }

    /// Full scale must land on ±1.0 and zero on 0.0, and I must not be swapped
    /// with Q — a swap mirrors the spectrum about the centre, which looks
    /// plausible on a waterfall and decodes as the wrong sideband.
    #[test]
    fn decodes_iq_pairs_to_unit_scale() {
        let mut bytes = Vec::new();
        for (re, im) in [(0i16, 0i16), (i16::MAX, i16::MIN), (16_384, -16_384)] {
            bytes.extend_from_slice(&re.to_le_bytes());
            bytes.extend_from_slice(&im.to_le_bytes());
        }
        let mut out = Vec::new();
        assert_eq!(decode_samples(&bytes, &mut out), 3);
        assert_eq!(out.len(), 6);
        assert_eq!(out[0], 0.0);
        assert_eq!(out[1], 0.0);
        assert!((out[2] - 0.999_969_5).abs() < 1e-6, "i16::MAX just under 1.0");
        assert_eq!(out[3], -1.0, "i16::MIN is exactly -1.0");
        assert_eq!(out[4], 0.5);
        assert_eq!(out[5], -0.5);
    }

    /// A bulk transfer can end mid-pair. Zero-padding the missing half would
    /// put a step into the stream; dropping it only loses one sample.
    #[test]
    fn a_trailing_partial_sample_is_dropped_not_padded() {
        let bytes = [1u8, 0, 2, 0, 3, 0]; // one whole pair, then half of one
        let mut out = Vec::new();
        assert_eq!(decode_samples(&bytes, &mut out), 1);
        assert_eq!(out.len(), 2, "only the complete pair");
    }

    /// A transport that records what it was asked to do and replies from a
    /// script, so the open sequence can be checked without a radio.
    struct FakeUsb {
        replies: std::cell::RefCell<std::collections::VecDeque<Result<Vec<u8>, &'static str>>>,
        seen: std::cell::RefCell<Vec<ControlRequest>>,
    }

    impl FakeUsb {
        fn new(replies: Vec<Result<Vec<u8>, &'static str>>) -> Self {
            Self {
                replies: std::cell::RefCell::new(replies.into()),
                seen: std::cell::RefCell::new(Vec::new()),
            }
        }
        fn requests(&self) -> Vec<ControlRequest> {
            self.seen.borrow().clone()
        }
    }

    impl super::super::UsbControl for FakeUsb {
        type Error = &'static str;
        fn control(&self, request: &ControlRequest) -> Result<Vec<u8>, Self::Error> {
            self.seen.borrow_mut().push(request.clone());
            self.replies.borrow_mut().pop_front().unwrap_or(Ok(Vec::new()))
        }
    }

    fn rates_reply(rates: &[u32]) -> Vec<u8> {
        rates.iter().flat_map(|r| r.to_le_bytes()).collect()
    }

    /// The order is the logic. Stopping the stream has to come first — a
    /// device left streaming ignores configuration — and the rate list has to
    /// be read before any rate is selected, because selection is by index into
    /// that list.
    #[test]
    fn open_sequence_stops_the_stream_before_reading_rates() {
        let usb = FakeUsb::new(vec![
            Ok(Vec::new()),
            Ok(4u32.to_le_bytes().to_vec()),
            Ok(rates_reply(&[912_000, 768_000, 384_000, 256_000])),
        ]);
        let rates = open_sequence(&usb).expect("open");
        assert_eq!(rates, vec![912_000, 768_000, 384_000, 256_000]);

        let seen = usb.requests();
        assert_eq!(seen.len(), 3);
        assert_eq!(seen[0].request, Request::ReceiverMode as u8);
        assert_eq!(seen[0].value, 0, "stream stopped, not started");
        assert_eq!(seen[1].request, Request::GetSamplerates as u8);
        assert_eq!(seen[1].index, 0, "the count");
        assert_eq!(seen[2].request, Request::GetSamplerates as u8);
        assert_eq!(seen[2].index, 4, "then that many rates");
    }

    /// A failed transfer must stop the sequence, not carry on and decode
    /// whatever the next reply happens to be as if it answered this request.
    #[test]
    fn open_sequence_stops_at_the_first_transport_error() {
        let usb = FakeUsb::new(vec![Err("pipe stalled")]);
        assert_eq!(open_sequence(&usb), Err(OpenError::Transport("pipe stalled")));
        assert_eq!(usb.requests().len(), 1, "no further transfers after a failure");
    }

    /// A garbage count would otherwise become a `read_sample_rates` asking for
    /// gigabytes over a control endpoint.
    #[test]
    fn open_sequence_rejects_an_implausible_rate_count() {
        let usb = FakeUsb::new(vec![Ok(Vec::new()), Ok(0xFFFF_FFFFu32.to_le_bytes().to_vec())]);
        assert_eq!(open_sequence(&usb), Err(OpenError::ImplausibleRateCount(0xFFFF_FFFF)));
        assert_eq!(usb.requests().len(), 2, "the rate read is never attempted");

        // Zero is equally unusable: there would be no rate to select.
        let none = FakeUsb::new(vec![Ok(Vec::new()), Ok(0u32.to_le_bytes().to_vec())]);
        assert_eq!(open_sequence(&none), Err(OpenError::ImplausibleRateCount(0)));
        assert_eq!(none.requests().len(), 2);
    }

    /// Out-of-range attenuation stalls the control endpoint on the device,
    /// which surfaces much later as an unrelated failure.
    #[test]
    fn attenuation_is_clamped_to_what_the_firmware_accepts() {
        assert_eq!(set_attenuation(0).value, 0);
        assert_eq!(set_attenuation(8).value, 8);
        assert_eq!(set_attenuation(200).value, 8, "clamped, not wrapped or sent");
    }

    /// Same shape as `set_receiver_mode`: the flag rides in `value` and
    /// `index` stays zero. Swapping them is silent — the device ignores the
    /// command and the AGC simply never comes on.
    #[test]
    fn gain_flags_ride_in_value() {
        for (req, name) in [(set_agc(true), "agc"), (set_lna(true), "lna")] {
            assert_eq!(req.direction, Direction::Out, "{name}");
            assert_eq!(req.value, 1, "{name} flag belongs in value");
            assert_eq!(req.index, 0, "{name} must not use index");
            assert!(req.data.is_empty(), "{name} carries no payload");
        }
        assert_eq!(set_agc(false).value, 0);
        assert_eq!(set_lna(false).value, 0);
        assert_eq!(set_agc(true).request, Request::SetAgc as u8);
        assert_eq!(set_lna(true).request, Request::SetLna as u8);
    }

    /// The request's length and the parser's expectation are one fact stored
    /// twice. If they drift apart the read comes back short and every attempt
    /// to identify the device fails, with nothing pointing at the cause.
    #[test]
    fn serial_read_asks_for_exactly_what_the_parser_needs() {
        let req = read_serial_and_board_id();
        assert_eq!(req.direction, Direction::In);
        assert_eq!(req.request, Request::GetSerialNoBoardId as u8);

        let reply = vec![0u8; usize::from(req.length)];
        assert!(
            parse_serial_and_board_id(&reply).is_ok(),
            "a full-length reply must parse"
        );
        assert!(
            parse_serial_and_board_id(&reply[..reply.len() - 1]).is_err(),
            "and it must be asking for the minimum, not more"
        );
    }

    /// These strings are what a user sees when a radio will not start, so they
    /// have to name the thing that is wrong.
    #[test]
    fn errors_describe_what_went_wrong() {
        let short = DecodeError::Short { need: 16, got: 3 };
        let text = short.to_string();
        assert!(text.contains("16") && text.contains('3'), "{text}");

        assert!(DecodeError::ZeroSampleRate.to_string().contains("zero"));

        let transport: OpenError<&str> = OpenError::Transport("pipe stalled");
        assert!(transport.to_string().contains("pipe stalled"));

        let decode: OpenError<&str> = OpenError::Decode(short);
        assert!(decode.to_string().contains("16"), "the cause is not swallowed");

        let count: OpenError<&str> = OpenError::ImplausibleRateCount(4_000_000);
        assert!(count.to_string().contains("4000000"));
    }

    /// The geometry has to agree with itself: a transfer is a whole number of
    /// samples, or every transfer boundary leaves a partial pair.
    #[test]
    fn transfer_geometry_is_a_whole_number_of_samples() {
        assert_eq!(TRANSFER_BYTES, 16_384);
        assert_eq!(TRANSFER_BYTES % BYTES_PER_SAMPLE, 0);
        assert_eq!(TRANSFER_BYTES / BYTES_PER_SAMPLE, TRANSFER_SAMPLES);
        assert_eq!(BULK_IN_ENDPOINT & 0x80, 0x80, "must be an IN endpoint");
    }
}

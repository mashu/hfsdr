//! Frequency offsets in the CW receive chain — two coordinate frames.
//!
//! - **Channel:** offset from the hardware tune center (panadapter 0 Hz, waterfall x-axis).
//!   UI markers, RIT, notch placement, and skimmer peaks all use this frame.
//! - **Baseband:** offset after the listen NCO has mixed the tuned signal to DC. IQ notches
//!   and per-bin math inside the decimated chain use this frame.
//!
//! Always convert at the boundary with [`ChannelOffsetHz::to_baseband`] — never subtract
//! `listen_offset_hz` by hand in DSP code.

/// Offset from the receiver tune center (Hz). Same frame as the panadapter / waterfall plot.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct ChannelOffsetHz(pub f32);

/// Offset from the current listen point in mixed-down baseband (Hz).
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct BasebandOffsetHz(pub f32);

impl ChannelOffsetHz {
    pub const ZERO: Self = Self(0.0);

    #[inline]
    pub const fn new(hz: f32) -> Self {
        Self(hz)
    }

    #[inline]
    pub const fn hz(self) -> f32 {
        self.0
    }

    /// Baseband offset after mixing `listen` to DC.
    #[inline]
    pub fn to_baseband(self, listen: ChannelOffsetHz) -> BasebandOffsetHz {
        BasebandOffsetHz::new(self.0 - listen.0)
    }

    /// Plot / storage `f32` → channel frame.
    #[inline]
    pub fn from_plot_hz(hz: f32) -> Self {
        Self::new(hz)
    }

    /// Channel offset for a tone at `baseband` when listen sits at `listen`.
    #[inline]
    pub fn from_baseband(baseband: BasebandOffsetHz, listen: ChannelOffsetHz) -> Self {
        Self::new(baseband.0 + listen.0)
    }
}

impl BasebandOffsetHz {
    pub const ZERO: Self = Self(0.0);

    #[inline]
    pub const fn new(hz: f32) -> Self {
        Self(hz)
    }

    #[inline]
    pub const fn hz(self) -> f32 {
        self.0
    }

    #[inline]
    pub fn to_channel(self, listen: ChannelOffsetHz) -> ChannelOffsetHz {
        ChannelOffsetHz::new(self.0 + listen.0)
    }
}

/// Where the listen point sits in channel coordinates for the current IQ segment.
///
/// After wideband ingress the IQ is already listen-shifted; pass the original listen here
/// while [`CwChannelSettings::listen_offset_hz`] is zero so the channel NCO is not applied twice.
#[derive(Clone, Copy, Debug)]
pub struct ListenOrigin {
    pub listen: ChannelOffsetHz,
}

impl ListenOrigin {
    #[inline]
    pub const fn at_center() -> Self {
        Self {
            listen: ChannelOffsetHz::ZERO,
        }
    }

    #[inline]
    pub fn from_settings(listen_offset_hz: ChannelOffsetHz) -> Self {
        Self {
            listen: listen_offset_hz,
        }
    }

    /// Wideband ingress already mixed `listen` to DC; channel NCO should stay at 0.
    #[inline]
    pub fn after_upstream_mix(listen: ChannelOffsetHz) -> Self {
        Self { listen }
    }

    #[inline]
    pub fn channel_to_baseband(&self, ch: ChannelOffsetHz) -> BasebandOffsetHz {
        ch.to_baseband(self.listen)
    }

    /// Single DSP entry point: channel notch marker → baseband mix Hz for [`IqNotch`].
    ///
    /// Do not subtract `listen_offset_hz` by hand in the receive chain — use this instead.
    #[inline]
    pub fn convert_for_notch(self, notch: ChannelOffsetHz) -> f32 {
        self.channel_to_baseband(notch).hz()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_channel_baseband() {
        let listen = ChannelOffsetHz::new(100.0);
        let het = ChannelOffsetHz::new(420.0);
        let bb = het.to_baseband(listen);
        assert!((bb.hz() - 320.0).abs() < f32::EPSILON);
        assert!((bb.to_channel(listen).hz() - het.hz()).abs() < f32::EPSILON);
    }

    #[test]
    fn convert_for_notch_matches_subtract() {
        let origin = ListenOrigin::from_settings(ChannelOffsetHz::new(100.0));
        assert!((origin.convert_for_notch(ChannelOffsetHz::new(400.0)) - 300.0).abs() < f32::EPSILON);
    }

    #[test]
    fn dsp_sources_use_convert_for_notch_not_raw_subtract() {
        let channel = std::fs::read_to_string("src/dsp/cw/channel.rs").expect("channel.rs");
        assert!(
            !channel.contains("offset_hz - settings.listen_offset"),
            "channel.rs: use ListenOrigin::convert_for_notch instead of manual subtraction"
        );
        assert!(
            channel.contains("convert_for_notch"),
            "channel.rs: must call ListenOrigin::convert_for_notch for IQ notches"
        );

        let wideband = std::fs::read_to_string("src/dsp/wideband_cw.rs").expect("wideband_cw.rs");
        assert!(
            wideband.contains("ListenOrigin::after_upstream_mix"),
            "wideband_cw.rs: must pass listen origin after ingress mix"
        );
        assert!(
            !wideband.contains("n.offset_hz -="),
            "wideband_cw.rs: do not rewrite notch offsets — use ListenOrigin"
        );
    }
}

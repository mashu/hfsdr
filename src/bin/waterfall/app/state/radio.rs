use hfsdr::CwChannelSettings;

#[derive(Clone, Debug)]
pub struct RadioState {
    pub sample_rate: f32,
    pub center_khz: f64,
    pub last_center_khz: f64,
    pub is_kiwi: bool,
    pub cw: CwChannelSettings,
    pub rit_hz: f32,
    pub rit_on: bool,
    pub pitch_lock: bool,
    pub lock_ham_bands: bool,
    pub agc_rf_on: bool,
    pub last_agc_rf_on: bool,
    /// Yaesu-style software RF gain (dB), applied to IQ on every source.
    pub rf_gain_db: f32,
    pub last_kiwi_man_gain: u8,
    pub last_kiwi_rf_attn_db: f32,
    pub last_kiwi_has_rf_attn: bool,
    pub last_snr_db: f32,
    /// When true, channel filter BW may extend to 2 kHz; when false, capped at 500 Hz (CW range).
    pub passband_wide: bool,
    /// Follow band-plan CW sideband (CW-L / CW-U) when the RX center changes band.
    pub sideband_auto: bool,
}

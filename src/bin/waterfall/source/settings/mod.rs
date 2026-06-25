mod airspy;
mod kiwi;
mod qmx;
mod rtlsdr;

pub use airspy::AirspySettings;
pub use kiwi::KiwiSettings;
pub use qmx::QmxSettings;
pub use rtlsdr::RtlSdrSettings;
#[cfg(feature = "airspy")]
pub use airspy::default_airspy_sample_rate;
#[cfg(feature = "rtlsdr")]
pub use rtlsdr::default_rtlsdr_sample_rate;

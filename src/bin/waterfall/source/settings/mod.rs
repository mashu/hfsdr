mod airspy;
mod kiwi;
mod qmx;
mod rtlsdr;
#[cfg(feature = "soapy")]
mod soapy;

pub use airspy::AirspySettings;
pub use kiwi::KiwiSettings;
pub use qmx::QmxSettings;
pub use rtlsdr::RtlSdrSettings;
#[cfg(feature = "soapy")]
pub use soapy::{default_soapy_sample_rate, SoapySettings};
#[cfg(feature = "airspy")]
pub use airspy::default_airspy_sample_rate;
#[cfg(feature = "rtlsdr")]
pub use rtlsdr::default_rtlsdr_sample_rate;

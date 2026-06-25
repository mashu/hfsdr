//! IQ preprocessing: block mix-down, anti-alias decimation, SIMD FIR dots.
//!
//! All stages operate on [`Complex32`] (f32) so Airspy HF+ float IQ dynamic range
//! is preserved — no fixed-point or int16 conversion in this path.

mod fir_decim;
mod ingress;
mod mixer;
mod worker;

pub use fir_decim::FirDecimator;
pub use ingress::IqShiftDecim;
pub use mixer::IqRotator;
pub use worker::IngressWorker;

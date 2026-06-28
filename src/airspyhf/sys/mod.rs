//! Raw FFI to libairspyhf (verified against
//! `/usr/include/libairspyhf/airspyhf.h`, version 1.8.x).
//!
//! Loaded at runtime on every platform so the binary starts even when libairspyhf
//! is not installed (see `crate::sdr_ffi::dylib`).

#![allow(non_camel_case_types)]

use num_complex::Complex32;
use std::os::raw::{c_int, c_void};

/// Opaque handle (`airspyhf_device_t`). Never constructed on the Rust side.
#[repr(C)]
pub struct airspyhf_device_t {
    _private: [u8; 0],
}

/// `airspyhf_complex_float_t { float re; float im; }`.
pub type ComplexF32 = Complex32;

/// `airspyhf_transfer_t` — the struct handed to the streaming callback.
#[repr(C)]
pub struct airspyhf_transfer_t {
    pub device: *mut airspyhf_device_t,
    pub ctx: *mut c_void,
    pub samples: *mut ComplexF32,
    pub sample_count: c_int,
    pub dropped_samples: u64,
}

/// `airspyhf_lib_version_t`.
#[repr(C)]
pub struct airspyhf_lib_version_t {
    pub major_version: u32,
    pub minor_version: u32,
    pub revision: u32,
}

/// Return 0 to continue streaming, non-zero to stop.
pub type airspyhf_sample_block_cb_fn =
    extern "C" fn(transfer: *mut airspyhf_transfer_t) -> c_int;

pub const SUCCESS: c_int = 0;

pub const FLAGS_OPTIMIZE_BAND_III: u32 = 1;
pub const FLAGS_OPTIMIZE_PLL_INT_BOUNDARY: u32 = 2;

mod dynamic;
pub use dynamic::*;

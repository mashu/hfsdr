//! Raw FFI to libairspyhf (verified against
//! `/usr/include/libairspyhf/airspyhf.h`, version 1.6.x).
//!
//! This is the only place that talks to C. Everything in the parent module is
//! safe Rust built on top of these declarations.

#![allow(non_camel_case_types)]

use num_complex::Complex32;
use std::os::raw::{c_char, c_int, c_void};

/// Opaque handle (`airspyhf_device_t`). Never constructed on the Rust side.
#[repr(C)]
pub struct airspyhf_device_t {
    _private: [u8; 0],
}

/// `airspyhf_complex_float_t { float re; float im; }`.
///
/// `num_complex::Complex<f32>` is `#[repr(C)]` with fields `re`, `im` in this
/// order, so it is layout-compatible and we can reinterpret the sample pointer
/// directly without a copy/convert step.
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

/// `typedef int (*airspyhf_sample_block_cb_fn)(airspyhf_transfer_t*)`.
///
/// Return 0 to continue streaming, non-zero to stop.
pub type airspyhf_sample_block_cb_fn =
    extern "C" fn(transfer: *mut airspyhf_transfer_t) -> c_int;

/// `AIRSPYHF_SUCCESS`.
pub const SUCCESS: c_int = 0;

#[link(name = "airspyhf")]
extern "C" {
    pub fn airspyhf_lib_version(lib_version: *mut airspyhf_lib_version_t);
    pub fn airspyhf_list_devices(serials: *mut u64, count: c_int) -> c_int;
    pub fn airspyhf_open(device: *mut *mut airspyhf_device_t) -> c_int;
    pub fn airspyhf_open_sn(device: *mut *mut airspyhf_device_t, serial: u64) -> c_int;
    pub fn airspyhf_close(device: *mut airspyhf_device_t) -> c_int;
    /// Number of IQ samples to expect per callback at the current rate.
    pub fn airspyhf_get_output_size(device: *mut airspyhf_device_t) -> c_int;
    pub fn airspyhf_start(
        device: *mut airspyhf_device_t,
        callback: airspyhf_sample_block_cb_fn,
        ctx: *mut c_void,
    ) -> c_int;
    pub fn airspyhf_stop(device: *mut airspyhf_device_t) -> c_int;
    /// 0 = Zero-IF, 1 = Low-IF at the current sample rate.
    pub fn airspyhf_is_low_if(device: *mut airspyhf_device_t) -> c_int;
    pub fn airspyhf_set_freq(device: *mut airspyhf_device_t, freq_hz: u32) -> c_int;
    /// Enable/disable IQ correction, IF shift, and fine tuning in the library.
    pub fn airspyhf_set_lib_dsp(device: *mut airspyhf_device_t, flag: u8) -> c_int;
    /// Pass `len == 0` to write the rate count into `buffer[0]`, then call
    /// again with `len == count` and a buffer of that size to fill it.
    pub fn airspyhf_get_samplerates(
        device: *mut airspyhf_device_t,
        buffer: *mut u32,
        len: u32,
    ) -> c_int;
    pub fn airspyhf_set_samplerate(device: *mut airspyhf_device_t, samplerate: u32) -> c_int;
    pub fn airspyhf_get_calibration(device: *mut airspyhf_device_t, ppb: *mut i32) -> c_int;
    pub fn airspyhf_set_calibration(device: *mut airspyhf_device_t, ppb: i32) -> c_int;
    /// HF AGC: 0 = off, 1 = on.
    pub fn airspyhf_set_hf_agc(device: *mut airspyhf_device_t, flag: u8) -> c_int;
    /// When AGC on: 0 = low threshold, 1 = high threshold.
    pub fn airspyhf_set_hf_agc_threshold(device: *mut airspyhf_device_t, flag: u8) -> c_int;
    /// Attenuator: 0..=8 (0..48 dB in 6 dB steps).
    pub fn airspyhf_set_hf_att(device: *mut airspyhf_device_t, value: u8) -> c_int;
    /// LNA / preamp: 0 or 1 (+6 dB, compensated digitally).
    pub fn airspyhf_set_hf_lna(device: *mut airspyhf_device_t, flag: u8) -> c_int;
    pub fn airspyhf_version_string_read(
        device: *mut airspyhf_device_t,
        version: *mut c_char,
        length: u8,
    ) -> c_int;
}

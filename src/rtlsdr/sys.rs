//! Raw FFI to librtlsdr (verified against `/usr/include/rtl-sdr.h`).

#![allow(non_camel_case_types)]

use std::os::raw::{c_char, c_int, c_void};

/// Opaque handle (`rtlsdr_dev_t`).
#[repr(C)]
pub struct rtlsdr_dev_t {
    _private: [u8; 0],
}

pub const SUCCESS: c_int = 0;

pub type rtlsdr_read_async_cb_t = extern "C" fn(buf: *mut u8, len: u32, ctx: *mut c_void);

#[link(name = "rtlsdr")]
extern "C" {
    pub fn rtlsdr_get_device_count() -> u32;
    pub fn rtlsdr_get_device_name(index: u32) -> *const c_char;
    pub fn rtlsdr_open(dev: *mut *mut rtlsdr_dev_t, index: u32) -> c_int;
    pub fn rtlsdr_close(dev: *mut rtlsdr_dev_t) -> c_int;
    pub fn rtlsdr_set_center_freq(dev: *mut rtlsdr_dev_t, freq: u32) -> c_int;
    pub fn rtlsdr_set_freq_correction(dev: *mut rtlsdr_dev_t, ppm: c_int) -> c_int;
    pub fn rtlsdr_get_tuner_gains(dev: *mut rtlsdr_dev_t, gains: *mut c_int) -> c_int;
    pub fn rtlsdr_set_tuner_gain(dev: *mut rtlsdr_dev_t, gain: c_int) -> c_int;
    pub fn rtlsdr_set_tuner_gain_mode(dev: *mut rtlsdr_dev_t, manual: c_int) -> c_int;
    pub fn rtlsdr_set_sample_rate(dev: *mut rtlsdr_dev_t, rate: u32) -> c_int;
    pub fn rtlsdr_get_sample_rate(dev: *mut rtlsdr_dev_t) -> u32;
    pub fn rtlsdr_set_agc_mode(dev: *mut rtlsdr_dev_t, on: c_int) -> c_int;
    pub fn rtlsdr_set_direct_sampling(dev: *mut rtlsdr_dev_t, on: c_int) -> c_int;
    pub fn rtlsdr_set_offset_tuning(dev: *mut rtlsdr_dev_t, on: c_int) -> c_int;
    pub fn rtlsdr_reset_buffer(dev: *mut rtlsdr_dev_t) -> c_int;
    pub fn rtlsdr_read_async(
        dev: *mut rtlsdr_dev_t,
        cb: rtlsdr_read_async_cb_t,
        ctx: *mut c_void,
        buf_num: u32,
        buf_len: u32,
    ) -> c_int;
    pub fn rtlsdr_cancel_async(dev: *mut rtlsdr_dev_t) -> c_int;
    pub fn rtlsdr_set_bias_tee(dev: *mut rtlsdr_dev_t, on: c_int) -> c_int;
}

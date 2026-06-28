//! Runtime-loaded librtlsdr (Windows, Linux, macOS).

use std::os::raw::{c_char, c_int, c_void};
use std::sync::OnceLock;

use libloading::Library;

use crate::sdr_ffi::dylib::{self, RTLSDR_SONAMES};

use super::{rtlsdr_dev_t, rtlsdr_read_async_cb_t};

type DeviceCountFn = unsafe extern "C" fn() -> u32;
type DeviceNameFn = unsafe extern "C" fn(u32) -> *const c_char;
type OpenFn = unsafe extern "C" fn(*mut *mut rtlsdr_dev_t, u32) -> c_int;
type CloseFn = unsafe extern "C" fn(*mut rtlsdr_dev_t) -> c_int;
type SetCenterFreqFn = unsafe extern "C" fn(*mut rtlsdr_dev_t, u32) -> c_int;
type SetFreqCorrectionFn = unsafe extern "C" fn(*mut rtlsdr_dev_t, c_int) -> c_int;
type GetTunerGainsFn = unsafe extern "C" fn(*mut rtlsdr_dev_t, *mut c_int) -> c_int;
type SetTunerGainFn = unsafe extern "C" fn(*mut rtlsdr_dev_t, c_int) -> c_int;
type SetTunerGainModeFn = unsafe extern "C" fn(*mut rtlsdr_dev_t, c_int) -> c_int;
type SetSampleRateFn = unsafe extern "C" fn(*mut rtlsdr_dev_t, u32) -> c_int;
type GetSampleRateFn = unsafe extern "C" fn(*mut rtlsdr_dev_t) -> u32;
type SetAgcModeFn = unsafe extern "C" fn(*mut rtlsdr_dev_t, c_int) -> c_int;
type SetDirectSamplingFn = unsafe extern "C" fn(*mut rtlsdr_dev_t, c_int) -> c_int;
type SetOffsetTuningFn = unsafe extern "C" fn(*mut rtlsdr_dev_t, c_int) -> c_int;
type ResetBufferFn = unsafe extern "C" fn(*mut rtlsdr_dev_t) -> c_int;
type ReadAsyncFn = unsafe extern "C" fn(
    *mut rtlsdr_dev_t,
    rtlsdr_read_async_cb_t,
    *mut c_void,
    u32,
    u32,
) -> c_int;
type CancelAsyncFn = unsafe extern "C" fn(*mut rtlsdr_dev_t) -> c_int;
type SetBiasTeeFn = unsafe extern "C" fn(*mut rtlsdr_dev_t, c_int) -> c_int;

struct Api {
    _lib: Library,
    get_device_count: DeviceCountFn,
    get_device_name: DeviceNameFn,
    open: OpenFn,
    close: CloseFn,
    set_center_freq: SetCenterFreqFn,
    set_freq_correction: SetFreqCorrectionFn,
    get_tuner_gains: GetTunerGainsFn,
    set_tuner_gain: SetTunerGainFn,
    set_tuner_gain_mode: SetTunerGainModeFn,
    set_sample_rate: SetSampleRateFn,
    get_sample_rate: GetSampleRateFn,
    set_agc_mode: SetAgcModeFn,
    set_direct_sampling: SetDirectSamplingFn,
    set_offset_tuning: SetOffsetTuningFn,
    reset_buffer: ResetBufferFn,
    read_async: ReadAsyncFn,
    cancel_async: CancelAsyncFn,
    set_bias_tee: SetBiasTeeFn,
}

static API: OnceLock<Option<Api>> = OnceLock::new();

fn api() -> Option<&'static Api> {
    API.get_or_init(load_api).as_ref()
}

fn load_api() -> Option<Api> {
    let lib = dylib::load(RTLSDR_SONAMES)?;
    Some(Api {
        get_device_count: dylib::required_sym(&lib, "rtlsdr_get_device_count")?,
        get_device_name: dylib::required_sym(&lib, "rtlsdr_get_device_name")?,
        open: dylib::required_sym(&lib, "rtlsdr_open")?,
        close: dylib::required_sym(&lib, "rtlsdr_close")?,
        set_center_freq: dylib::required_sym(&lib, "rtlsdr_set_center_freq")?,
        set_freq_correction: dylib::required_sym(&lib, "rtlsdr_set_freq_correction")?,
        get_tuner_gains: dylib::required_sym(&lib, "rtlsdr_get_tuner_gains")?,
        set_tuner_gain: dylib::required_sym(&lib, "rtlsdr_set_tuner_gain")?,
        set_tuner_gain_mode: dylib::required_sym(&lib, "rtlsdr_set_tuner_gain_mode")?,
        set_sample_rate: dylib::required_sym(&lib, "rtlsdr_set_sample_rate")?,
        get_sample_rate: dylib::required_sym(&lib, "rtlsdr_get_sample_rate")?,
        set_agc_mode: dylib::required_sym(&lib, "rtlsdr_set_agc_mode")?,
        set_direct_sampling: dylib::required_sym(&lib, "rtlsdr_set_direct_sampling")?,
        set_offset_tuning: dylib::required_sym(&lib, "rtlsdr_set_offset_tuning")?,
        reset_buffer: dylib::required_sym(&lib, "rtlsdr_reset_buffer")?,
        read_async: dylib::required_sym(&lib, "rtlsdr_read_async")?,
        cancel_async: dylib::required_sym(&lib, "rtlsdr_cancel_async")?,
        set_bias_tee: dylib::required_sym(&lib, "rtlsdr_set_bias_tee")?,
        _lib: lib,
    })
}

pub fn rtlsdr_get_device_count() -> u32 {
    api().map_or(0, |a| unsafe { (a.get_device_count)() })
}

pub fn rtlsdr_get_device_name(index: u32) -> *const c_char {
    api().map_or(std::ptr::null(), |a| unsafe { (a.get_device_name)(index) })
}

pub fn rtlsdr_open(dev: *mut *mut rtlsdr_dev_t, index: u32) -> c_int {
    api().map_or(-1, |a| unsafe { (a.open)(dev, index) })
}

pub fn rtlsdr_close(dev: *mut rtlsdr_dev_t) -> c_int {
    api().map_or(-1, |a| unsafe { (a.close)(dev) })
}

pub fn rtlsdr_set_center_freq(dev: *mut rtlsdr_dev_t, freq: u32) -> c_int {
    api().map_or(-1, |a| unsafe { (a.set_center_freq)(dev, freq) })
}

pub fn rtlsdr_set_freq_correction(dev: *mut rtlsdr_dev_t, ppm: c_int) -> c_int {
    api().map_or(-1, |a| unsafe { (a.set_freq_correction)(dev, ppm) })
}

pub fn rtlsdr_get_tuner_gains(dev: *mut rtlsdr_dev_t, gains: *mut c_int) -> c_int {
    api().map_or(-1, |a| unsafe { (a.get_tuner_gains)(dev, gains) })
}

pub fn rtlsdr_set_tuner_gain(dev: *mut rtlsdr_dev_t, gain: c_int) -> c_int {
    api().map_or(-1, |a| unsafe { (a.set_tuner_gain)(dev, gain) })
}

pub fn rtlsdr_set_tuner_gain_mode(dev: *mut rtlsdr_dev_t, manual: c_int) -> c_int {
    api().map_or(-1, |a| unsafe { (a.set_tuner_gain_mode)(dev, manual) })
}

pub fn rtlsdr_set_sample_rate(dev: *mut rtlsdr_dev_t, rate: u32) -> c_int {
    api().map_or(-1, |a| unsafe { (a.set_sample_rate)(dev, rate) })
}

pub fn rtlsdr_get_sample_rate(dev: *mut rtlsdr_dev_t) -> u32 {
    api().map_or(0, |a| unsafe { (a.get_sample_rate)(dev) })
}

pub fn rtlsdr_set_agc_mode(dev: *mut rtlsdr_dev_t, on: c_int) -> c_int {
    api().map_or(-1, |a| unsafe { (a.set_agc_mode)(dev, on) })
}

pub fn rtlsdr_set_direct_sampling(dev: *mut rtlsdr_dev_t, on: c_int) -> c_int {
    api().map_or(-1, |a| unsafe { (a.set_direct_sampling)(dev, on) })
}

pub fn rtlsdr_set_offset_tuning(dev: *mut rtlsdr_dev_t, on: c_int) -> c_int {
    api().map_or(-1, |a| unsafe { (a.set_offset_tuning)(dev, on) })
}

pub fn rtlsdr_reset_buffer(dev: *mut rtlsdr_dev_t) -> c_int {
    api().map_or(-1, |a| unsafe { (a.reset_buffer)(dev) })
}

pub fn rtlsdr_read_async(
    dev: *mut rtlsdr_dev_t,
    cb: rtlsdr_read_async_cb_t,
    ctx: *mut c_void,
    buf_num: u32,
    buf_len: u32,
) -> c_int {
    api().map_or(-1, |a| unsafe { (a.read_async)(dev, cb, ctx, buf_num, buf_len) })
}

pub fn rtlsdr_cancel_async(dev: *mut rtlsdr_dev_t) -> c_int {
    api().map_or(-1, |a| unsafe { (a.cancel_async)(dev) })
}

pub fn rtlsdr_set_bias_tee(dev: *mut rtlsdr_dev_t, on: c_int) -> c_int {
    api().map_or(-1, |a| unsafe { (a.set_bias_tee)(dev, on) })
}

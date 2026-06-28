//! Runtime-loaded libairspyhf (Windows, Linux, macOS).

use std::os::raw::{c_char, c_int, c_void};
use std::sync::OnceLock;

use libloading::Library;

use crate::sdr_ffi::dylib::{self, AIRSPYHF_SONAMES};

use super::{
    airspyhf_device_t, airspyhf_lib_version_t, airspyhf_sample_block_cb_fn,
};

type LibVersionFn = unsafe extern "C" fn(*mut airspyhf_lib_version_t);
type ListDevicesFn = unsafe extern "C" fn(*mut u64, c_int) -> c_int;
type OpenFn = unsafe extern "C" fn(*mut *mut airspyhf_device_t) -> c_int;
type OpenSnFn = unsafe extern "C" fn(*mut *mut airspyhf_device_t, u64) -> c_int;
type CloseFn = unsafe extern "C" fn(*mut airspyhf_device_t) -> c_int;
type OutputSizeFn = unsafe extern "C" fn(*mut airspyhf_device_t) -> c_int;
type StartFn = unsafe extern "C" fn(
    *mut airspyhf_device_t,
    airspyhf_sample_block_cb_fn,
    *mut c_void,
) -> c_int;
type StopFn = unsafe extern "C" fn(*mut airspyhf_device_t) -> c_int;
type IsLowIfFn = unsafe extern "C" fn(*mut airspyhf_device_t) -> c_int;
type SetFreqFn = unsafe extern "C" fn(*mut airspyhf_device_t, u32) -> c_int;
type SetLibDspFn = unsafe extern "C" fn(*mut airspyhf_device_t, u8) -> c_int;
type GetSampleratesFn = unsafe extern "C" fn(*mut airspyhf_device_t, *mut u32, u32) -> c_int;
type SetSamplerateFn = unsafe extern "C" fn(*mut airspyhf_device_t, u32) -> c_int;
type GetCalibrationFn = unsafe extern "C" fn(*mut airspyhf_device_t, *mut i32) -> c_int;
type SetCalibrationFn = unsafe extern "C" fn(*mut airspyhf_device_t, i32) -> c_int;
type SetHfAgcFn = unsafe extern "C" fn(*mut airspyhf_device_t, u8) -> c_int;
type SetHfAgcThresholdFn = unsafe extern "C" fn(*mut airspyhf_device_t, u8) -> c_int;
type SetHfAttFn = unsafe extern "C" fn(*mut airspyhf_device_t, u8) -> c_int;
type SetHfLnaFn = unsafe extern "C" fn(*mut airspyhf_device_t, u8) -> c_int;
type VersionStringReadFn =
    unsafe extern "C" fn(*mut airspyhf_device_t, *mut c_char, u8) -> c_int;
type GetFrontendOptionsFn = unsafe extern "C" fn(*mut airspyhf_device_t, *mut u32) -> c_int;
type SetFrontendOptionsFn = unsafe extern "C" fn(*mut airspyhf_device_t, u32) -> c_int;
type SetBiasTeeFn = unsafe extern "C" fn(*mut airspyhf_device_t, i8) -> c_int;

struct Api {
    _lib: Library,
    lib_version: LibVersionFn,
    list_devices: ListDevicesFn,
    open: OpenFn,
    open_sn: OpenSnFn,
    close: CloseFn,
    get_output_size: OutputSizeFn,
    start: StartFn,
    stop: StopFn,
    is_low_if: IsLowIfFn,
    set_freq: SetFreqFn,
    set_lib_dsp: SetLibDspFn,
    get_samplerates: GetSampleratesFn,
    set_samplerate: SetSamplerateFn,
    get_calibration: GetCalibrationFn,
    set_calibration: SetCalibrationFn,
    set_hf_agc: SetHfAgcFn,
    set_hf_agc_threshold: SetHfAgcThresholdFn,
    set_hf_att: SetHfAttFn,
    set_hf_lna: SetHfLnaFn,
    version_string_read: VersionStringReadFn,
    #[cfg(airspyhf_extended_api)]
    get_frontend_options: GetFrontendOptionsFn,
    #[cfg(airspyhf_extended_api)]
    set_frontend_options: SetFrontendOptionsFn,
    #[cfg(airspyhf_extended_api)]
    set_bias_tee: SetBiasTeeFn,
}

static API: OnceLock<Option<Api>> = OnceLock::new();

fn api() -> Option<&'static Api> {
    API.get_or_init(load_api).as_ref()
}

fn load_api() -> Option<Api> {
    let lib = dylib::load(AIRSPYHF_SONAMES)?;
    Some(Api {
        lib_version: dylib::required_sym(&lib, "airspyhf_lib_version")?,
        list_devices: dylib::required_sym(&lib, "airspyhf_list_devices")?,
        open: dylib::required_sym(&lib, "airspyhf_open")?,
        open_sn: dylib::required_sym(&lib, "airspyhf_open_sn")?,
        close: dylib::required_sym(&lib, "airspyhf_close")?,
        get_output_size: dylib::required_sym(&lib, "airspyhf_get_output_size")?,
        start: dylib::required_sym(&lib, "airspyhf_start")?,
        stop: dylib::required_sym(&lib, "airspyhf_stop")?,
        is_low_if: dylib::required_sym(&lib, "airspyhf_is_low_if")?,
        set_freq: dylib::required_sym(&lib, "airspyhf_set_freq")?,
        set_lib_dsp: dylib::required_sym(&lib, "airspyhf_set_lib_dsp")?,
        get_samplerates: dylib::required_sym(&lib, "airspyhf_get_samplerates")?,
        set_samplerate: dylib::required_sym(&lib, "airspyhf_set_samplerate")?,
        get_calibration: dylib::required_sym(&lib, "airspyhf_get_calibration")?,
        set_calibration: dylib::required_sym(&lib, "airspyhf_set_calibration")?,
        set_hf_agc: dylib::required_sym(&lib, "airspyhf_set_hf_agc")?,
        set_hf_agc_threshold: dylib::required_sym(&lib, "airspyhf_set_hf_agc_threshold")?,
        set_hf_att: dylib::required_sym(&lib, "airspyhf_set_hf_att")?,
        set_hf_lna: dylib::required_sym(&lib, "airspyhf_set_hf_lna")?,
        version_string_read: dylib::required_sym(&lib, "airspyhf_version_string_read")?,
        #[cfg(airspyhf_extended_api)]
        get_frontend_options: dylib::required_sym(&lib, "airspyhf_get_frontend_options")?,
        #[cfg(airspyhf_extended_api)]
        set_frontend_options: dylib::required_sym(&lib, "airspyhf_set_frontend_options")?,
        #[cfg(airspyhf_extended_api)]
        set_bias_tee: dylib::required_sym(&lib, "airspyhf_set_bias_tee")?,
        _lib: lib,
    })
}

pub fn airspyhf_lib_version(lib_version: *mut airspyhf_lib_version_t) {
    if let Some(a) = api() {
        unsafe { (a.lib_version)(lib_version) };
    }
}

pub fn airspyhf_list_devices(serials: *mut u64, count: c_int) -> c_int {
    api().map_or(-1, |a| unsafe { (a.list_devices)(serials, count) })
}

pub fn airspyhf_open(device: *mut *mut airspyhf_device_t) -> c_int {
    api().map_or(-1, |a| unsafe { (a.open)(device) })
}

pub fn airspyhf_open_sn(device: *mut *mut airspyhf_device_t, serial: u64) -> c_int {
    api().map_or(-1, |a| unsafe { (a.open_sn)(device, serial) })
}

pub fn airspyhf_close(device: *mut airspyhf_device_t) -> c_int {
    api().map_or(-1, |a| unsafe { (a.close)(device) })
}

pub fn airspyhf_get_output_size(device: *mut airspyhf_device_t) -> c_int {
    api().map_or(-1, |a| unsafe { (a.get_output_size)(device) })
}

pub fn airspyhf_start(
    device: *mut airspyhf_device_t,
    callback: airspyhf_sample_block_cb_fn,
    ctx: *mut c_void,
) -> c_int {
    api().map_or(-1, |a| unsafe { (a.start)(device, callback, ctx) })
}

pub fn airspyhf_stop(device: *mut airspyhf_device_t) -> c_int {
    api().map_or(-1, |a| unsafe { (a.stop)(device) })
}

pub fn airspyhf_is_low_if(device: *mut airspyhf_device_t) -> c_int {
    api().map_or(-1, |a| unsafe { (a.is_low_if)(device) })
}

pub fn airspyhf_set_freq(device: *mut airspyhf_device_t, freq_hz: u32) -> c_int {
    api().map_or(-1, |a| unsafe { (a.set_freq)(device, freq_hz) })
}

pub fn airspyhf_set_lib_dsp(device: *mut airspyhf_device_t, flag: u8) -> c_int {
    api().map_or(-1, |a| unsafe { (a.set_lib_dsp)(device, flag) })
}

pub fn airspyhf_get_samplerates(
    device: *mut airspyhf_device_t,
    buffer: *mut u32,
    len: u32,
) -> c_int {
    api().map_or(-1, |a| unsafe { (a.get_samplerates)(device, buffer, len) })
}

pub fn airspyhf_set_samplerate(device: *mut airspyhf_device_t, samplerate: u32) -> c_int {
    api().map_or(-1, |a| unsafe { (a.set_samplerate)(device, samplerate) })
}

pub fn airspyhf_get_calibration(device: *mut airspyhf_device_t, ppb: *mut i32) -> c_int {
    api().map_or(-1, |a| unsafe { (a.get_calibration)(device, ppb) })
}

pub fn airspyhf_set_calibration(device: *mut airspyhf_device_t, ppb: i32) -> c_int {
    api().map_or(-1, |a| unsafe { (a.set_calibration)(device, ppb) })
}

pub fn airspyhf_set_hf_agc(device: *mut airspyhf_device_t, flag: u8) -> c_int {
    api().map_or(-1, |a| unsafe { (a.set_hf_agc)(device, flag) })
}

pub fn airspyhf_set_hf_agc_threshold(device: *mut airspyhf_device_t, flag: u8) -> c_int {
    api().map_or(-1, |a| unsafe { (a.set_hf_agc_threshold)(device, flag) })
}

pub fn airspyhf_set_hf_att(device: *mut airspyhf_device_t, value: u8) -> c_int {
    api().map_or(-1, |a| unsafe { (a.set_hf_att)(device, value) })
}

pub fn airspyhf_set_hf_lna(device: *mut airspyhf_device_t, flag: u8) -> c_int {
    api().map_or(-1, |a| unsafe { (a.set_hf_lna)(device, flag) })
}

pub fn airspyhf_version_string_read(
    device: *mut airspyhf_device_t,
    version: *mut c_char,
    length: u8,
) -> c_int {
    api().map_or(-1, |a| unsafe { (a.version_string_read)(device, version, length) })
}

#[cfg(airspyhf_extended_api)]
pub fn airspyhf_get_frontend_options(device: *mut airspyhf_device_t, flags: *mut u32) -> c_int {
    api().map_or(-1, |a| unsafe { (a.get_frontend_options)(device, flags) })
}

#[cfg(airspyhf_extended_api)]
pub fn airspyhf_set_frontend_options(device: *mut airspyhf_device_t, flags: u32) -> c_int {
    api().map_or(-1, |a| unsafe { (a.set_frontend_options)(device, flags) })
}

#[cfg(airspyhf_extended_api)]
pub fn airspyhf_set_bias_tee(device: *mut airspyhf_device_t, value: i8) -> c_int {
    api().map_or(-1, |a| unsafe { (a.set_bias_tee)(device, value) })
}

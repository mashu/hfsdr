//! libSoapySDR via `libloading`.

use std::ffi::{c_char, c_void, CStr, CString};
use std::os::raw::{c_int, c_longlong};
use std::sync::OnceLock;

use libloading::Library;

use crate::sdr_ffi::dylib::{self, SOAPYSDR_SONAMES};

use super::{
    SoapySDRDevice, SoapySDRKwargs, SoapySDRRange, SoapySDRStream, SOAPY_SDR_RX, SOAPY_SDR_TIMEOUT,
};

type EnumerateStrArgsFn = unsafe extern "C" fn(*const c_char, *mut usize) -> *mut SoapySDRKwargs;
type KwargsListClearFn = unsafe extern "C" fn(*mut SoapySDRKwargs, usize);
type KwargsToStringFn = unsafe extern "C" fn(*const SoapySDRKwargs) -> *mut c_char;
type FreeFn = unsafe extern "C" fn(*mut c_void);
type MakeStrArgsFn = unsafe extern "C" fn(*const c_char) -> *mut SoapySDRDevice;
type UnmakeFn = unsafe extern "C" fn(*mut SoapySDRDevice) -> c_int;
type LastErrorFn = unsafe extern "C" fn() -> *const c_char;
type SetupStreamFn = unsafe extern "C" fn(
    *mut SoapySDRDevice,
    c_int,
    *const c_char,
    *const usize,
    usize,
    *const SoapySDRKwargs,
) -> *mut SoapySDRStream;
type CloseStreamFn = unsafe extern "C" fn(*mut SoapySDRDevice, *mut SoapySDRStream) -> c_int;
type GetStreamMtuFn = unsafe extern "C" fn(*const SoapySDRDevice, *const SoapySDRStream) -> usize;
type ActivateStreamFn =
    unsafe extern "C" fn(*mut SoapySDRDevice, *mut SoapySDRStream, c_int, c_longlong, usize) -> c_int;
type DeactivateStreamFn =
    unsafe extern "C" fn(*mut SoapySDRDevice, *mut SoapySDRStream, c_int, c_longlong) -> c_int;
type ReadStreamFn = unsafe extern "C" fn(
    *mut SoapySDRDevice,
    *mut SoapySDRStream,
    *const *mut c_void,
    usize,
    *mut c_int,
    *mut c_longlong,
    c_longlong,
) -> c_int;
type SetSampleRateFn =
    unsafe extern "C" fn(*mut SoapySDRDevice, c_int, usize, f64) -> c_int;
type GetSampleRateFn = unsafe extern "C" fn(*const SoapySDRDevice, c_int, usize) -> f64;
type ListSampleRatesFn =
    unsafe extern "C" fn(*const SoapySDRDevice, c_int, usize, *mut usize) -> *mut f64;
type GetSampleRateRangeFn =
    unsafe extern "C" fn(*const SoapySDRDevice, c_int, usize, *mut usize) -> *mut SoapySDRRange;
type SetFrequencyFn = unsafe extern "C" fn(
    *mut SoapySDRDevice,
    c_int,
    usize,
    f64,
    *const SoapySDRKwargs,
) -> c_int;
type GetFrequencyFn = unsafe extern "C" fn(*const SoapySDRDevice, c_int, usize) -> f64;
type SetGainFn = unsafe extern "C" fn(*mut SoapySDRDevice, c_int, usize, f64) -> c_int;
type GetGainFn = unsafe extern "C" fn(*const SoapySDRDevice, c_int, usize) -> f64;
type SetGainModeFn = unsafe extern "C" fn(*mut SoapySDRDevice, c_int, usize, bool) -> c_int;
type GetGainModeFn = unsafe extern "C" fn(*const SoapySDRDevice, c_int, usize) -> bool;
type GetGainRangeFn = unsafe extern "C" fn(*const SoapySDRDevice, c_int, usize) -> SoapySDRRange;
type ListAntennasFn =
    unsafe extern "C" fn(*const SoapySDRDevice, c_int, usize, *mut usize) -> *mut *mut c_char;
type SetAntennaFn =
    unsafe extern "C" fn(*mut SoapySDRDevice, c_int, usize, *const c_char) -> c_int;
type StringsClearFn = unsafe extern "C" fn(*mut *mut *mut c_char, usize);
type GetDriverKeyFn = unsafe extern "C" fn(*const SoapySDRDevice) -> *mut c_char;

struct Api {
    _lib: Library,
    enumerate_str_args: EnumerateStrArgsFn,
    kwargs_list_clear: KwargsListClearFn,
    kwargs_to_string: KwargsToStringFn,
    free: FreeFn,
    make_str_args: MakeStrArgsFn,
    unmake: UnmakeFn,
    last_error: LastErrorFn,
    setup_stream: SetupStreamFn,
    close_stream: CloseStreamFn,
    get_stream_mtu: GetStreamMtuFn,
    activate_stream: ActivateStreamFn,
    deactivate_stream: DeactivateStreamFn,
    read_stream: ReadStreamFn,
    set_sample_rate: SetSampleRateFn,
    get_sample_rate: GetSampleRateFn,
    list_sample_rates: ListSampleRatesFn,
    get_sample_rate_range: GetSampleRateRangeFn,
    set_frequency: SetFrequencyFn,
    get_frequency: GetFrequencyFn,
    set_gain: SetGainFn,
    get_gain: GetGainFn,
    set_gain_mode: SetGainModeFn,
    get_gain_mode: GetGainModeFn,
    get_gain_range: GetGainRangeFn,
    list_antennas: ListAntennasFn,
    set_antenna: SetAntennaFn,
    strings_clear: StringsClearFn,
    get_driver_key: GetDriverKeyFn,
}

static API: OnceLock<Option<Api>> = OnceLock::new();

fn api() -> Option<&'static Api> {
    API.get_or_init(load_api).as_ref()
}

fn load_api() -> Option<Api> {
    let lib = dylib::load(SOAPYSDR_SONAMES)?;
    Some(Api {
        enumerate_str_args: dylib::required_sym(&lib, "SoapySDRDevice_enumerateStrArgs")?,
        kwargs_list_clear: dylib::required_sym(&lib, "SoapySDRKwargsList_clear")?,
        kwargs_to_string: dylib::required_sym(&lib, "SoapySDRKwargs_toString")?,
        free: dylib::required_sym(&lib, "SoapySDR_free")?,
        make_str_args: dylib::required_sym(&lib, "SoapySDRDevice_makeStrArgs")?,
        unmake: dylib::required_sym(&lib, "SoapySDRDevice_unmake")?,
        last_error: dylib::required_sym(&lib, "SoapySDRDevice_lastError")?,
        setup_stream: dylib::required_sym(&lib, "SoapySDRDevice_setupStream")?,
        close_stream: dylib::required_sym(&lib, "SoapySDRDevice_closeStream")?,
        get_stream_mtu: dylib::required_sym(&lib, "SoapySDRDevice_getStreamMTU")?,
        activate_stream: dylib::required_sym(&lib, "SoapySDRDevice_activateStream")?,
        deactivate_stream: dylib::required_sym(&lib, "SoapySDRDevice_deactivateStream")?,
        read_stream: dylib::required_sym(&lib, "SoapySDRDevice_readStream")?,
        set_sample_rate: dylib::required_sym(&lib, "SoapySDRDevice_setSampleRate")?,
        get_sample_rate: dylib::required_sym(&lib, "SoapySDRDevice_getSampleRate")?,
        list_sample_rates: dylib::required_sym(&lib, "SoapySDRDevice_listSampleRates")?,
        get_sample_rate_range: dylib::required_sym(&lib, "SoapySDRDevice_getSampleRateRange")?,
        set_frequency: dylib::required_sym(&lib, "SoapySDRDevice_setFrequency")?,
        get_frequency: dylib::required_sym(&lib, "SoapySDRDevice_getFrequency")?,
        set_gain: dylib::required_sym(&lib, "SoapySDRDevice_setGain")?,
        get_gain: dylib::required_sym(&lib, "SoapySDRDevice_getGain")?,
        set_gain_mode: dylib::required_sym(&lib, "SoapySDRDevice_setGainMode")?,
        get_gain_mode: dylib::required_sym(&lib, "SoapySDRDevice_getGainMode")?,
        get_gain_range: dylib::required_sym(&lib, "SoapySDRDevice_getGainRange")?,
        list_antennas: dylib::required_sym(&lib, "SoapySDRDevice_listAntennas")?,
        set_antenna: dylib::required_sym(&lib, "SoapySDRDevice_setAntenna")?,
        strings_clear: dylib::required_sym(&lib, "SoapySDRStrings_clear")?,
        get_driver_key: dylib::required_sym(&lib, "SoapySDRDevice_getDriverKey")?,
        _lib: lib,
    })
}

pub fn library_loaded() -> bool {
    api().is_some()
}

fn cstr_lossy(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned()
}

fn free_cstring(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    if let Some(a) = api() {
        unsafe { (a.free)(ptr as *mut c_void) };
    }
}

pub fn last_error_message() -> String {
    api()
        .map(|a| cstr_lossy(unsafe { (a.last_error)() }))
        .unwrap_or_else(|| "libSoapySDR not loaded".into())
}

pub fn enumerate_devices(driver: &str) -> Vec<(String, String)> {
    let Some(a) = api() else {
        return Vec::new();
    };
    let filter = if driver.trim().is_empty() {
        CString::new("").expect("empty filter")
    } else {
        CString::new(format!("driver={}", driver.trim())).unwrap_or_default()
    };
    let mut len = 0usize;
    let list = unsafe { (a.enumerate_str_args)(filter.as_ptr(), &mut len) };
    if list.is_null() || len == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for i in 0..len {
        let kw = unsafe { &*list.add(i) };
        let args_ptr = unsafe { (a.kwargs_to_string)(kw) };
        let args = cstr_lossy(args_ptr);
        free_cstring(args_ptr);
        let label = if args.is_empty() {
            format!("device #{i}")
        } else {
            args.clone()
        };
        out.push((label, args));
    }
    unsafe { (a.kwargs_list_clear)(list, len) };
    out
}

pub fn make_device(args: &str) -> Option<*mut SoapySDRDevice> {
    let a = api()?;
    let cargs = CString::new(args).ok()?;
    let dev = unsafe { (a.make_str_args)(cargs.as_ptr()) };
    if dev.is_null() {
        None
    } else {
        Some(dev)
    }
}

pub fn unmake_device(dev: *mut SoapySDRDevice) {
    if dev.is_null() {
        return;
    }
    if let Some(a) = api() {
        let _ = unsafe { (a.unmake)(dev) };
    }
}

pub fn driver_key(dev: *const SoapySDRDevice) -> String {
    api()
        .map(|a| {
            let ptr = unsafe { (a.get_driver_key)(dev) };
            let s = cstr_lossy(ptr);
            free_cstring(ptr);
            s
        })
        .unwrap_or_default()
}

pub fn list_sample_rates(dev: *const SoapySDRDevice) -> Vec<u32> {
    let Some(a) = api() else {
        return Vec::new();
    };
    let mut len = 0usize;
    let ptr = unsafe { (a.list_sample_rates)(dev, SOAPY_SDR_RX, 0, &mut len) };
    if ptr.is_null() || len == 0 {
        return sample_rates_from_range(dev);
    }
    let mut rates = Vec::with_capacity(len);
    for i in 0..len {
        let hz = unsafe { *ptr.add(i) };
        if hz > 0.0 && hz <= u32::MAX as f64 {
            rates.push(hz.round() as u32);
        }
    }
    unsafe { (a.free)(ptr as *mut c_void) };
    rates.sort_unstable();
    rates.dedup();
    rates
}

fn sample_rates_from_range(dev: *const SoapySDRDevice) -> Vec<u32> {
    let Some(a) = api() else {
        return Vec::new();
    };
    let mut len = 0usize;
    let ptr = unsafe { (a.get_sample_rate_range)(dev, SOAPY_SDR_RX, 0, &mut len) };
    if ptr.is_null() || len == 0 {
        return Vec::new();
    }
    let mut rates = Vec::new();
    for i in 0..len {
        let r = unsafe { *ptr.add(i) };
        let step = if r.step > 0.0 { r.step } else { 1.0 };
        let mut hz = r.minimum;
        while hz <= r.maximum + step * 0.5 {
            if hz > 0.0 && hz <= u32::MAX as f64 {
                rates.push(hz.round() as u32);
            }
            hz += step;
            if rates.len() > 64 {
                break;
            }
        }
    }
    unsafe { (a.free)(ptr as *mut c_void) };
    rates.sort_unstable();
    rates.dedup();
    rates
}

pub fn list_antennas(dev: *const SoapySDRDevice) -> Vec<String> {
    let Some(a) = api() else {
        return Vec::new();
    };
    let mut len = 0usize;
    let ptr = unsafe { (a.list_antennas)(dev, SOAPY_SDR_RX, 0, &mut len) };
    if ptr.is_null() || len == 0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        out.push(cstr_lossy(unsafe { *ptr.add(i) }));
    }
    unsafe { (a.strings_clear)(&mut (ptr as *mut *mut c_char), len) };
    out
}

pub fn gain_range(dev: *const SoapySDRDevice) -> (f64, f64) {
    api()
        .map(|a| {
            let r = unsafe { (a.get_gain_range)(dev, SOAPY_SDR_RX, 0) };
            (r.minimum, r.maximum)
        })
        .unwrap_or((0.0, 60.0))
}

pub fn set_sample_rate(dev: *mut SoapySDRDevice, hz: u32) -> c_int {
    api().map_or(-1, |a| unsafe {
        (a.set_sample_rate)(dev, SOAPY_SDR_RX, 0, hz as f64)
    })
}

pub fn get_sample_rate(dev: *const SoapySDRDevice) -> u32 {
    api()
        .map(|a| unsafe { (a.get_sample_rate)(dev, SOAPY_SDR_RX, 0).round() as u32 })
        .unwrap_or(0)
}

pub fn set_frequency(dev: *mut SoapySDRDevice, hz: f64) -> c_int {
    api().map_or(-1, |a| unsafe {
        (a.set_frequency)(dev, SOAPY_SDR_RX, 0, hz, std::ptr::null())
    })
}

pub fn get_frequency(dev: *const SoapySDRDevice) -> f64 {
    api()
        .map(|a| unsafe { (a.get_frequency)(dev, SOAPY_SDR_RX, 0) })
        .unwrap_or(0.0)
}

pub fn set_gain(dev: *mut SoapySDRDevice, db: f64) -> c_int {
    api().map_or(-1, |a| unsafe { (a.set_gain)(dev, SOAPY_SDR_RX, 0, db) })
}

pub fn get_gain(dev: *const SoapySDRDevice) -> f64 {
    api()
        .map(|a| unsafe { (a.get_gain)(dev, SOAPY_SDR_RX, 0) })
        .unwrap_or(0.0)
}

pub fn set_gain_mode(dev: *mut SoapySDRDevice, automatic: bool) -> c_int {
    api().map_or(-1, |a| unsafe {
        (a.set_gain_mode)(dev, SOAPY_SDR_RX, 0, automatic)
    })
}

pub fn get_gain_mode(dev: *const SoapySDRDevice) -> bool {
    api()
        .map(|a| unsafe { (a.get_gain_mode)(dev, SOAPY_SDR_RX, 0) })
        .unwrap_or(false)
}

pub fn set_antenna(dev: *mut SoapySDRDevice, name: &str) -> c_int {
    let Some(a) = api() else {
        return -1;
    };
    let cname = match CString::new(name) {
        Ok(s) => s,
        Err(_) => return -1,
    };
    unsafe { (a.set_antenna)(dev, SOAPY_SDR_RX, 0, cname.as_ptr()) }
}

pub fn setup_rx_stream(dev: *mut SoapySDRDevice) -> Option<*mut SoapySDRStream> {
    let a = api()?;
    let channel: usize = 0;
    let stream = unsafe {
        (a.setup_stream)(
            dev,
            SOAPY_SDR_RX,
            super::CF32.as_ptr() as *const c_char,
            &channel,
            1,
            std::ptr::null(),
        )
    };
    if stream.is_null() {
        None
    } else {
        Some(stream)
    }
}

pub fn close_stream(dev: *mut SoapySDRDevice, stream: *mut SoapySDRStream) -> c_int {
    api().map_or(-1, |a| unsafe { (a.close_stream)(dev, stream) })
}

pub fn stream_mtu(dev: *const SoapySDRDevice, stream: *const SoapySDRStream) -> usize {
    api()
        .map(|a| unsafe { (a.get_stream_mtu)(dev, stream) })
        .unwrap_or(4096)
        .max(256)
}

pub fn activate_stream(dev: *mut SoapySDRDevice, stream: *mut SoapySDRStream) -> c_int {
    api().map_or(-1, |a| unsafe { (a.activate_stream)(dev, stream, 0, 0, 0) })
}

pub fn deactivate_stream(dev: *mut SoapySDRDevice, stream: *mut SoapySDRStream) -> c_int {
    api().map_or(-1, |a| unsafe { (a.deactivate_stream)(dev, stream, 0, 0) })
}

pub fn read_stream(
    dev: *mut SoapySDRDevice,
    stream: *mut SoapySDRStream,
    bufs: *const *mut c_void,
    num_elems: usize,
    timeout_us: i64,
) -> c_int {
    api().map_or(SOAPY_SDR_TIMEOUT, |a| {
        let mut flags = 0i32;
        let mut time_ns = 0i64;
        unsafe {
            (a.read_stream)(
                dev,
                stream,
                bufs,
                num_elems,
                &mut flags,
                &mut time_ns,
                timeout_us,
            )
        }
    })
}

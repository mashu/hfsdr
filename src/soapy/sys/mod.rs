//! Runtime-loaded libSoapySDR (Windows, Linux, macOS).

#![allow(non_camel_case_types)]

use std::ffi::c_char;

pub const SOAPY_SDR_RX: i32 = 1;
pub const SOAPY_SDR_TIMEOUT: i32 = -1;
pub const CF32: &[u8] = b"CF32\0";

#[repr(C)]
pub struct SoapySDRDevice {
    _private: [u8; 0],
}

#[repr(C)]
pub struct SoapySDRStream {
    _private: [u8; 0],
}

#[repr(C)]
pub struct SoapySDRKwargs {
    pub size: usize,
    pub keys: *mut *mut c_char,
    pub vals: *mut *mut c_char,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SoapySDRRange {
    pub minimum: f64,
    pub maximum: f64,
    pub step: f64,
}

mod dynamic;
pub use dynamic::*;

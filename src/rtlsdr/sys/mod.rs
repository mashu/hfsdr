//! Raw FFI to librtlsdr (verified against `/usr/include/rtl-sdr.h`).
//!
//! Loaded at runtime on every platform so the binary starts even when librtlsdr
//! is not installed (see `crate::sdr_ffi::dylib`).

#![allow(non_camel_case_types)]

use std::os::raw::{c_int, c_void};

#[repr(C)]
pub struct rtlsdr_dev_t {
    _private: [u8; 0],
}

pub const SUCCESS: c_int = 0;

pub type rtlsdr_read_async_cb_t = extern "C" fn(buf: *mut u8, len: u32, ctx: *mut c_void);

mod dynamic;
pub use dynamic::*;

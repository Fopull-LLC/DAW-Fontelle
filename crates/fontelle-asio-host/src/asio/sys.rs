// fontelle-asio — hosts an ASIO driver for Fontelle in a separate process.
// Copyright (C) 2026 Fopull LLC. GPL-3.0-only; see `LICENSE` beside
// `Cargo.toml`.
//
// This file is a Rust translation of the declarations in Steinberg's ASIO
// SDK 2.3.4 (2025-10-15) — `common/asio.h` and `common/iasiodrv.h` —
// used under the SDK's GPLv3 option:
//
//   Project     : ASIO SDK
//   (c) 2025, Steinberg Media Technologies GmbH, All Rights Reserved
//   This Software Development Kit is licensed under the terms of the
//   Steinberg ASIO License, or alternatively under the terms of the General
//   Public License (GPL) Version 3.
//
// The SDK's own licence text is `STEINBERG-ASIO-SDK-LICENSE.txt` beside
// `Cargo.toml`. Only declarations are translated: names, numbers, struct
// layouts and the driver interface's method order. No SDK code is compiled
// into this program. ASIO is a registered trademark of Steinberg Media
// Technologies GmbH.

//! The ASIO driver interface, as the SDK declares it for 64-bit Windows.
//!
//! Three things the C++ headers say implicitly and Rust must say out loud:
//! `long` is 32 bits on Windows; `asio.h` is compiled under
//! `#pragma pack(push,4)`, hence `packed(4)`; and `IASIO`'s methods are plain
//! virtual functions, so on 64-bit Windows (one calling convention) they are
//! `extern "C"` with the object as the first argument. 32-bit Windows would
//! need `thiscall`, and is refused below.

#![allow(dead_code)]

use std::ffi::{c_char, c_void};

#[cfg(target_arch = "x86")]
compile_error!("fontelle-asio supports 64-bit Windows only");

pub type AsioBool = i32;
pub type AsioError = i32;
pub type AsioSampleRate = f64;
pub type AsioSampleType = i32;

pub const ASIO_FALSE: AsioBool = 0;
pub const ASIO_TRUE: AsioBool = 1;

pub const ASE_OK: AsioError = 0;
pub const ASE_SUCCESS: AsioError = 0x3f48_47a0;
pub const ASE_NOT_PRESENT: AsioError = -1000;
pub const ASE_HW_MALFUNCTION: AsioError = -999;
pub const ASE_INVALID_PARAMETER: AsioError = -998;
pub const ASE_INVALID_MODE: AsioError = -997;
pub const ASE_SP_NOT_ADVANCING: AsioError = -996;
pub const ASE_NO_CLOCK: AsioError = -995;
pub const ASE_NO_MEMORY: AsioError = -994;

// `asioMessage` selectors.
pub const K_ASIO_SELECTOR_SUPPORTED: i32 = 1;
pub const K_ASIO_ENGINE_VERSION: i32 = 2;
pub const K_ASIO_RESET_REQUEST: i32 = 3;
pub const K_ASIO_BUFFER_SIZE_CHANGE: i32 = 4;
pub const K_ASIO_RESYNC_REQUEST: i32 = 5;
pub const K_ASIO_LATENCIES_CHANGED: i32 = 6;
pub const K_ASIO_SUPPORTS_TIME_INFO: i32 = 7;
pub const K_ASIO_SUPPORTS_TIME_CODE: i32 = 8;
pub const K_ASIO_MMC_COMMAND: i32 = 9;
pub const K_ASIO_SUPPORTS_INPUT_MONITOR: i32 = 10;
pub const K_ASIO_SUPPORTS_INPUT_GAIN: i32 = 11;
pub const K_ASIO_SUPPORTS_INPUT_METER: i32 = 12;
pub const K_ASIO_SUPPORTS_OUTPUT_GAIN: i32 = 13;
pub const K_ASIO_SUPPORTS_OUTPUT_METER: i32 = 14;
pub const K_ASIO_OVERLOAD: i32 = 15;

// `future` selectors.
pub const K_ASIO_CAN_REPORT_OVERLOAD: i32 = 0x2404_2012;

#[repr(C, packed(4))]
#[derive(Clone, Copy, Default)]
pub struct AsioSamples {
    pub hi: u32,
    pub lo: u32,
}

#[repr(C, packed(4))]
#[derive(Clone, Copy, Default)]
pub struct AsioTimeStamp {
    pub hi: u32,
    pub lo: u32,
}

#[repr(C, packed(4))]
#[derive(Clone, Copy)]
pub struct AsioClockSource {
    pub index: i32,
    pub associated_channel: i32,
    pub associated_group: i32,
    pub is_current_source: AsioBool,
    pub name: [c_char; 32],
}

#[repr(C, packed(4))]
#[derive(Clone, Copy)]
pub struct AsioChannelInfo {
    pub channel: i32,
    pub is_input: AsioBool,
    pub is_active: AsioBool,
    pub channel_group: i32,
    pub sample_type: AsioSampleType,
    pub name: [c_char; 32],
}

#[repr(C, packed(4))]
#[derive(Clone, Copy)]
pub struct AsioBufferInfo {
    pub is_input: AsioBool,
    pub channel_num: i32,
    pub buffers: [*mut c_void; 2],
}

#[repr(C, packed(4))]
#[derive(Clone, Copy)]
pub struct AsioTimeInfo {
    pub speed: f64,
    pub system_time: AsioTimeStamp,
    pub sample_position: AsioSamples,
    pub sample_rate: AsioSampleRate,
    pub flags: u32,
    pub reserved: [c_char; 12],
}

#[repr(C, packed(4))]
#[derive(Clone, Copy)]
pub struct AsioTimeCode {
    pub speed: f64,
    pub time_code_samples: AsioSamples,
    pub flags: u32,
    pub future: [c_char; 64],
}

#[repr(C, packed(4))]
#[derive(Clone, Copy)]
pub struct AsioTime {
    pub reserved: [i32; 4],
    pub time_info: AsioTimeInfo,
    pub time_code: AsioTimeCode,
}

#[repr(C)]
pub struct AsioCallbacks {
    pub buffer_switch: unsafe extern "C" fn(double_buffer_index: i32, direct_process: AsioBool),
    pub sample_rate_did_change: unsafe extern "C" fn(rate: AsioSampleRate),
    pub asio_message:
        unsafe extern "C" fn(selector: i32, value: i32, message: *mut c_void, opt: *mut f64) -> i32,
    pub buffer_switch_time_info: unsafe extern "C" fn(
        params: *mut AsioTime,
        double_buffer_index: i32,
        direct_process: AsioBool,
    ) -> *mut AsioTime,
}

/// A driver object: `IASIO`, a COM interface.
#[repr(C)]
pub struct IAsio {
    pub vtbl: *const IAsioVtbl,
}

/// `IUnknown`'s three, then `IASIO`'s in the header's order.
#[repr(C)]
pub struct IAsioVtbl {
    pub query_interface: unsafe extern "system" fn(
        this: *mut IAsio,
        iid: *const windows_sys::core::GUID,
        out: *mut *mut c_void,
    ) -> i32,
    pub add_ref: unsafe extern "system" fn(this: *mut IAsio) -> u32,
    pub release: unsafe extern "system" fn(this: *mut IAsio) -> u32,
    pub init: unsafe extern "C" fn(this: *mut IAsio, sys_handle: *mut c_void) -> AsioBool,
    pub get_driver_name: unsafe extern "C" fn(this: *mut IAsio, name: *mut c_char),
    pub get_driver_version: unsafe extern "C" fn(this: *mut IAsio) -> i32,
    pub get_error_message: unsafe extern "C" fn(this: *mut IAsio, string: *mut c_char),
    pub start: unsafe extern "C" fn(this: *mut IAsio) -> AsioError,
    pub stop: unsafe extern "C" fn(this: *mut IAsio) -> AsioError,
    pub get_channels:
        unsafe extern "C" fn(this: *mut IAsio, inputs: *mut i32, outputs: *mut i32) -> AsioError,
    pub get_latencies:
        unsafe extern "C" fn(this: *mut IAsio, input: *mut i32, output: *mut i32) -> AsioError,
    pub get_buffer_size: unsafe extern "C" fn(
        this: *mut IAsio,
        min: *mut i32,
        max: *mut i32,
        preferred: *mut i32,
        granularity: *mut i32,
    ) -> AsioError,
    pub can_sample_rate: unsafe extern "C" fn(this: *mut IAsio, rate: AsioSampleRate) -> AsioError,
    pub get_sample_rate:
        unsafe extern "C" fn(this: *mut IAsio, rate: *mut AsioSampleRate) -> AsioError,
    pub set_sample_rate: unsafe extern "C" fn(this: *mut IAsio, rate: AsioSampleRate) -> AsioError,
    pub get_clock_sources: unsafe extern "C" fn(
        this: *mut IAsio,
        clocks: *mut AsioClockSource,
        count: *mut i32,
    ) -> AsioError,
    pub set_clock_source: unsafe extern "C" fn(this: *mut IAsio, reference: i32) -> AsioError,
    pub get_sample_position: unsafe extern "C" fn(
        this: *mut IAsio,
        position: *mut AsioSamples,
        stamp: *mut AsioTimeStamp,
    ) -> AsioError,
    pub get_channel_info:
        unsafe extern "C" fn(this: *mut IAsio, info: *mut AsioChannelInfo) -> AsioError,
    pub create_buffers: unsafe extern "C" fn(
        this: *mut IAsio,
        infos: *mut AsioBufferInfo,
        channels: i32,
        buffer_size: i32,
        callbacks: *mut AsioCallbacks,
    ) -> AsioError,
    pub dispose_buffers: unsafe extern "C" fn(this: *mut IAsio) -> AsioError,
    pub control_panel: unsafe extern "C" fn(this: *mut IAsio) -> AsioError,
    pub future:
        unsafe extern "C" fn(this: *mut IAsio, selector: i32, opt: *mut c_void) -> AsioError,
    pub output_ready: unsafe extern "C" fn(this: *mut IAsio) -> AsioError,
}

// The layouts `#pragma pack(4)` gives in the SDK.
const _: () = assert!(std::mem::size_of::<AsioChannelInfo>() == 52);
const _: () = assert!(std::mem::size_of::<AsioBufferInfo>() == 24);
const _: () = assert!(std::mem::size_of::<AsioClockSource>() == 48);
const _: () = assert!(std::mem::size_of::<AsioTimeInfo>() == 48);
const _: () = assert!(std::mem::size_of::<AsioTimeCode>() == 84);
const _: () = assert!(std::mem::size_of::<AsioTime>() == 148);
// IUnknown's 3 and IASIO's 21.
const _: () = assert!(std::mem::size_of::<IAsioVtbl>() == 24 * std::mem::size_of::<usize>());

/// An `ASIOError` in words.
pub fn error_words(code: AsioError) -> &'static str {
    match code {
        ASE_OK | ASE_SUCCESS => "no error",
        ASE_NOT_PRESENT => "the hardware is not there",
        ASE_HW_MALFUNCTION => "the hardware is malfunctioning",
        ASE_INVALID_PARAMETER => "the driver refused a setting",
        ASE_INVALID_MODE => "the hardware is in a mode that cannot do this",
        ASE_SP_NOT_ADVANCING => "the hardware is not running",
        ASE_NO_CLOCK => "the sample clock is not there or will not take this rate",
        ASE_NO_MEMORY => "the driver ran out of memory",
        _ => "an error the driver did not name",
    }
}

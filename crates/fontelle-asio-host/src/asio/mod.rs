// fontelle-asio — hosts an ASIO driver for Fontelle in a separate process.
// Copyright (C) 2026 Fopull LLC. GPL-3.0-only; see `LICENSE` beside
// `Cargo.toml`.
//
// How drivers are found and loaded follows Steinberg's ASIO SDK 2.3.4
// (`host/pc/asiolist.cpp`, `host/asiodrivers.cpp`), used under the SDK's
// GPLv3 option; see `STEINBERG-ASIO-SDK-LICENSE.txt`. ASIO is a registered
// trademark of Steinberg Media Technologies GmbH.

//! ASIO® drivers on Windows: finding them, loading one, and running it.
//!
//! A driver is a COM object listed under `HKLM\SOFTWARE\ASIO\<name>` by its
//! `CLSID`. It is made on this program's main thread, which is a
//! single-threaded COM apartment and pumps window messages while it waits
//! ([`AsioDevice::idle`]) — some drivers' control panels and reset
//! requests need that. Every call into the driver is made on that thread,
//! except the two ASIO makes on its own: `bufferSwitch`, which runs the
//! [`Pump`], and `outputReady`, which the SDK's own host calls there too.
//!
//! ASIO's callbacks carry no pointer of the host's, so the running state is
//! a process-wide static ([`LIVE`]): one driver per process, which is what
//! ASIO allows anyway.

pub mod sys;

use std::cell::UnsafeCell;
use std::ffi::{CStr, c_void};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};
use std::time::Duration;

use fontelle_audio_ipc::ReadyInfo;
use windows_sys::Win32::Foundation::{HWND, LPARAM, WAIT_OBJECT_0};
use windows_sys::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CLSIDFromString, COINIT_APARTMENTTHREADED, CoCreateInstance,
    CoInitializeEx,
};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW, RegGetValueW,
    RegOpenKeyExW,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcessId, INFINITE, OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EnumWindows, GetDesktopWindow, GetWindowThreadProcessId, IsWindowVisible,
    MSG, MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW, QS_ALLINPUT, TranslateMessage,
};
use windows_sys::core::GUID;

use self::sys::*;
use crate::convert::{SampleType, read_channel, write_channel};
use crate::device::{Device, Notice};
use crate::pump::Pump;
use crate::sizes::{COMMON_RATES, fit_buffer_size, rate_refusal};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A registry string value under `key\sub`.
fn reg_string(key: HKEY, sub: &[u16], value: &str) -> Option<String> {
    let value = wide(value);
    let mut buf = [0u16; 512];
    let mut bytes = (buf.len() * 2) as u32;
    // SAFETY: a bounded read into `buf`, whose size `bytes` says.
    let r = unsafe {
        RegGetValueW(
            key,
            sub.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if r != 0 {
        return None;
    }
    let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

/// Every driver under `HKLM\SOFTWARE\ASIO`: its name (the `description`
/// value, or the key's name, as the SDK's list does) and its CLSID.
fn drivers() -> Vec<(String, GUID)> {
    let mut found = Vec::new();
    let path = wide("SOFTWARE\\ASIO");
    let mut key: HKEY = std::ptr::null_mut();
    // SAFETY: opened read-only and closed below.
    if unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, path.as_ptr(), 0, KEY_READ, &mut key) } != 0 {
        return found;
    }
    for index in 0.. {
        let mut name = [0u16; 256];
        let mut len = name.len() as u32;
        // SAFETY: a bounded read into `name`.
        let r = unsafe {
            RegEnumKeyExW(
                key,
                index,
                name.as_mut_ptr(),
                &mut len,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if r != 0 {
            break;
        }
        let sub: Vec<u16> = name[..len as usize]
            .iter()
            .copied()
            .chain(std::iter::once(0))
            .collect();
        let key_name = String::from_utf16_lossy(&name[..len as usize]);
        let Some(clsid_text) = reg_string(key, &sub, "CLSID") else {
            continue;
        };
        let clsid_w = wide(&clsid_text);
        let mut clsid: GUID = unsafe { std::mem::zeroed() };
        // SAFETY: a NUL-terminated string and a GUID to fill.
        if unsafe { CLSIDFromString(clsid_w.as_ptr(), &mut clsid) } != 0 {
            continue;
        }
        let shown = reg_string(key, &sub, "Description")
            .filter(|d| !d.trim().is_empty())
            .unwrap_or(key_name);
        found.push((fontelle_audio_ipc::control::clean(&shown), clsid));
    }
    // SAFETY: opened above.
    unsafe { RegCloseKey(key) };
    found
}

/// The installed drivers' names.
pub fn driver_names() -> Vec<String> {
    drivers().into_iter().map(|(name, _)| name).collect()
}

/// Calls `gone` (on a thread of its own) when process `pid` ends.
pub fn watch_parent(pid: u32, gone: impl FnOnce() + Send + 'static) {
    // SAFETY: a handle only waited on, by the thread that owns it.
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if handle.is_null() {
        return;
    }
    let handle = handle as usize;
    std::thread::spawn(move || {
        // SAFETY: the handle opened above.
        if unsafe { WaitForSingleObject(handle as _, INFINITE) } == WAIT_OBJECT_0 {
            gone();
        }
    });
}

// What the driver's callbacks reach.

/// One exchanged channel's driver buffers and format.
struct Channel {
    buffers: [*mut u8; 2],
    ty: SampleType,
}

/// Everything `bufferSwitch` needs, alive from `start` to `stop`.
struct Live {
    pump: UnsafeCell<Pump>,
    busy: AtomicBool,
    driver: *mut IAsio,
    outputs: Vec<Channel>,
    inputs: Vec<Channel>,
    /// Channels per frame in the shared block's slots.
    block_outputs: usize,
    block_inputs: usize,
    frames: usize,
    output_ready: bool,
}

static LIVE: AtomicPtr<Live> = AtomicPtr::new(std::ptr::null_mut());

const NOTICE_RESET: u32 = 1;
const NOTICE_RATE: u32 = 2;
const NOTICE_LATENCIES: u32 = 4;
static NOTICES: AtomicU32 = AtomicU32::new(0);
static LOST: AtomicU32 = AtomicU32::new(0);

static CALLBACKS: AsioCallbacks = AsioCallbacks {
    buffer_switch,
    sample_rate_did_change,
    asio_message,
    buffer_switch_time_info,
};

unsafe extern "C" fn buffer_switch(index: i32, _direct: AsioBool) {
    on_buffer(index);
}

unsafe extern "C" fn buffer_switch_time_info(
    params: *mut AsioTime,
    index: i32,
    _direct: AsioBool,
) -> *mut AsioTime {
    on_buffer(index);
    params
}

unsafe extern "C" fn sample_rate_did_change(_rate: AsioSampleRate) {
    NOTICES.fetch_or(NOTICE_RATE, Ordering::AcqRel);
}

unsafe extern "C" fn asio_message(
    selector: i32,
    value: i32,
    _message: *mut c_void,
    _opt: *mut f64,
) -> i32 {
    match selector {
        K_ASIO_SELECTOR_SUPPORTED => i32::from(matches!(
            value,
            K_ASIO_RESET_REQUEST
                | K_ASIO_ENGINE_VERSION
                | K_ASIO_RESYNC_REQUEST
                | K_ASIO_LATENCIES_CHANGED
                | K_ASIO_SUPPORTS_TIME_INFO
                | K_ASIO_OVERLOAD
        )),
        K_ASIO_ENGINE_VERSION => 2,
        K_ASIO_RESET_REQUEST => {
            NOTICES.fetch_or(NOTICE_RESET, Ordering::AcqRel);
            1
        }
        // The driver lost sync — samples were dropped — or saw an overload.
        // Both are the device's account of a dropout.
        K_ASIO_RESYNC_REQUEST | K_ASIO_OVERLOAD => {
            LOST.fetch_add(1, Ordering::AcqRel);
            1
        }
        K_ASIO_LATENCIES_CHANGED => {
            NOTICES.fetch_or(NOTICE_LATENCIES, Ordering::AcqRel);
            1
        }
        K_ASIO_SUPPORTS_TIME_INFO => 1,
        _ => 0,
    }
}

/// One of the driver's buffers: Fontelle's audio in, the driver's format
/// out.
fn on_buffer(index: i32) {
    let live = LIVE.load(Ordering::Acquire);
    if live.is_null() {
        return;
    }
    // SAFETY: `LIVE` is only cleared after the driver's `stop` returned,
    // and then waited on `busy` (`AsioDevice::stop`).
    let live = unsafe { &*live };
    // A driver calling in again before the last call returned: let the
    // outer one finish rather than hand the pump out twice.
    if live.busy.swap(true, Ordering::Acquire) {
        return;
    }
    let half = (index & 1) as usize;
    let frames = live.frames;
    // SAFETY: the driver's buffers, `frames` samples of their type each,
    // valid for this half until the next call; the pump is ours while
    // `busy` is.
    unsafe {
        let pump = &mut *live.pump.get();
        pump.cycle(
            |input| {
                input.fill(0.0);
                for (c, ch) in live.inputs.iter().enumerate() {
                    let src = std::slice::from_raw_parts(ch.buffers[half], frames * ch.ty.bytes());
                    read_channel(ch.ty, src, input, live.block_inputs, c);
                }
            },
            |output| {
                for (c, ch) in live.outputs.iter().enumerate() {
                    let dst =
                        std::slice::from_raw_parts_mut(ch.buffers[half], frames * ch.ty.bytes());
                    match output {
                        Some(out) => write_channel(ch.ty, out, live.block_outputs, c, dst),
                        None => dst.fill(0),
                    }
                }
            },
        );
        if live.output_ready {
            ((*(*live.driver).vtbl).output_ready)(live.driver);
        }
    }
    live.busy.store(false, Ordering::Release);
}

/// What `open` made.
struct Opened {
    infos: Vec<AsioBufferInfo>,
    types: Vec<SampleType>,
    outputs: usize,
    block_outputs: usize,
    block_inputs: usize,
    frames: usize,
    output_ready: bool,
}

/// A loaded ASIO driver.
pub struct AsioDevice {
    name: String,
    driver: *mut IAsio,
    opened: Option<Opened>,
}

impl AsioDevice {
    /// Loads and initialises the driver called `name`.
    pub fn load(name: &str) -> Result<Self, String> {
        // SAFETY: once per thread; a second call or another model is fine
        // to ignore — the driver is made on this thread either way.
        unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
        let Some((_, clsid)) = drivers().into_iter().find(|(n, _)| n == name) else {
            return Err(format!("no ASIO driver called {name} is installed"));
        };
        let mut object: *mut c_void = std::ptr::null_mut();
        // SAFETY: the SDK's own way to make a driver: its CLSID as the
        // interface id too.
        let hr = unsafe {
            CoCreateInstance(
                &clsid,
                std::ptr::null_mut(),
                CLSCTX_INPROC_SERVER,
                &clsid,
                &mut object,
            )
        };
        if hr != 0 || object.is_null() {
            return Err(format!(
                "{name} is installed but would not load (error {hr:#010x}); reinstalling \
                 its driver may help"
            ));
        }
        let device = Self {
            name: name.to_string(),
            driver: object.cast(),
            opened: None,
        };
        // SAFETY: a live driver object; the desktop window as the system
        // handle, as hosts without a window of their own pass.
        let ok = unsafe { device.call(|v, d| (v.init)(d, GetDesktopWindow())) };
        if ok != ASIO_TRUE {
            let mut message = [0 as std::ffi::c_char; 256];
            // SAFETY: the SDK gives 124 bytes for this; 256 is room to spare.
            unsafe { device.call(|v, d| (v.get_error_message)(d, message.as_mut_ptr())) };
            // SAFETY: NUL-terminated within the buffer, which was zeroed.
            let words = unsafe { CStr::from_ptr(message.as_ptr()) }
                .to_string_lossy()
                .trim()
                .to_string();
            return Err(if words.is_empty() {
                format!("{name} would not start; is its device plugged in?")
            } else {
                format!("{name} would not start: {words}")
            });
        }
        Ok(device)
    }

    /// Calls the driver's vtable.
    ///
    /// # Safety
    /// `f` makes a call the ASIO interface allows in the driver's state.
    unsafe fn call<R>(&self, f: impl FnOnce(&IAsioVtbl, *mut IAsio) -> R) -> R {
        // SAFETY: `driver` is a live IASIO for the device's life.
        unsafe { f(&*(*self.driver).vtbl, self.driver) }
    }

    fn sample_rate(&self) -> Option<f64> {
        let mut rate = 0.0;
        // SAFETY: an out-parameter.
        let r = unsafe { self.call(|v, d| (v.get_sample_rate)(d, &mut rate)) };
        (r == ASE_OK).then_some(rate)
    }
}

impl Device for AsioDevice {
    fn open(
        &mut self,
        rate: u32,
        frames: Option<u32>,
        outputs: u32,
        inputs: u32,
    ) -> Result<ReadyInfo, String> {
        let name = self.name.clone();
        let (mut ins, mut outs) = (0i32, 0i32);
        // SAFETY: out-parameters.
        let r = unsafe { self.call(|v, d| (v.get_channels)(d, &mut ins, &mut outs)) };
        if r != ASE_OK || outs <= 0 {
            return Err(format!("{name} has no outputs ({})", error_words(r)));
        }
        let (mut min, mut max, mut preferred, mut granularity) = (0i32, 0i32, 0i32, 0i32);
        // SAFETY: out-parameters.
        let r = unsafe {
            self.call(|v, d| {
                (v.get_buffer_size)(d, &mut min, &mut max, &mut preferred, &mut granularity)
            })
        };
        if r != ASE_OK {
            return Err(format!(
                "{name} would not say its buffer sizes ({})",
                error_words(r)
            ));
        }

        // ASIO converts nothing: the driver runs at the studio's rate or
        // Fontelle cannot use it until it does.
        let wanted = f64::from(rate);
        if self
            .sample_rate()
            .is_none_or(|now| (now - wanted).abs() > 0.5)
        {
            // SAFETY: plain calls with a rate.
            let can = unsafe { self.call(|v, d| (v.can_sample_rate)(d, wanted)) };
            let set = can == ASE_OK
                && unsafe { self.call(|v, d| (v.set_sample_rate)(d, wanted)) } == ASE_OK;
            if !set {
                let offered: Vec<u32> = COMMON_RATES
                    .into_iter()
                    // SAFETY: as above.
                    .filter(|r| unsafe {
                        self.call(|v, d| (v.can_sample_rate)(d, f64::from(*r)))
                    } == ASE_OK)
                    .collect();
                return Err(rate_refusal(&name, &offered, rate));
            }
        }

        let frames = fit_buffer_size(
            frames,
            min.max(1) as u32,
            max.max(1) as u32,
            preferred.max(1) as u32,
            granularity,
        );
        let n_out = (outputs as i32).min(outs).max(0) as usize;
        let n_in = (inputs as i32).min(ins).max(0) as usize;
        let mut infos = Vec::with_capacity(n_out + n_in);
        let mut types = Vec::with_capacity(n_out + n_in);
        for (is_input, count) in [(ASIO_FALSE, n_out), (ASIO_TRUE, n_in)] {
            for channel in 0..count as i32 {
                let mut info = AsioChannelInfo {
                    channel,
                    is_input,
                    is_active: ASIO_FALSE,
                    channel_group: 0,
                    sample_type: 0,
                    name: [0; 32],
                };
                // SAFETY: an in/out struct the driver fills.
                let r = unsafe { self.call(|v, d| (v.get_channel_info)(d, &mut info)) };
                let ty = info.sample_type;
                let Some(ty) = (r == ASE_OK).then(|| SampleType::from_asio(ty)).flatten() else {
                    return Err(format!(
                        "{name} plays a sample format fontelle-asio does not know ({ty})"
                    ));
                };
                types.push(ty);
                infos.push(AsioBufferInfo {
                    is_input,
                    channel_num: channel,
                    buffers: [std::ptr::null_mut(); 2],
                });
            }
        }
        NOTICES.store(0, Ordering::Release);
        LOST.store(0, Ordering::Release);
        // SAFETY: `infos` outlives the buffers (kept in `opened`), and the
        // callbacks are a static.
        let r = unsafe {
            self.call(|v, d| {
                (v.create_buffers)(
                    d,
                    infos.as_mut_ptr(),
                    infos.len() as i32,
                    frames as i32,
                    (&CALLBACKS as *const AsioCallbacks).cast_mut(),
                )
            })
        };
        if r != ASE_OK {
            return Err(format!(
                "{name} would not make {frames}-frame buffers ({})",
                error_words(r)
            ));
        }
        // Silence in both halves before the start, as the SDK asks.
        for (info, ty) in infos.iter().zip(&types).take(n_out) {
            let buffers = info.buffers;
            for half in buffers {
                if !half.is_null() {
                    // SAFETY: a driver buffer of `frames` samples.
                    unsafe {
                        std::ptr::write_bytes(half.cast::<u8>(), 0, frames as usize * ty.bytes())
                    };
                }
            }
        }
        // SAFETY: a plain call; ASE_OK means the driver wants it.
        let output_ready = unsafe { self.call(|v, d| (v.output_ready)(d)) } == ASE_OK;
        let (input_latency, output_latency) = self.latencies().unwrap_or((0, 0));
        let format = types[0].name().to_string();
        self.opened = Some(Opened {
            infos,
            types,
            outputs: n_out,
            block_outputs: outputs as usize,
            block_inputs: inputs as usize,
            frames: frames as usize,
            output_ready,
        });
        Ok(ReadyInfo {
            driver: name,
            frames,
            rate,
            inputs: ins as u32,
            outputs: outs as u32,
            input_latency,
            output_latency,
            min_frames: min.max(0) as u32,
            max_frames: max.max(0) as u32,
            preferred_frames: preferred.max(0) as u32,
            granularity,
            format,
        })
    }

    fn start(&mut self, pump: Pump) -> Result<(), (String, Box<Pump>)> {
        let Some(opened) = &self.opened else {
            return Err(("the driver is not open".to_string(), Box::new(pump)));
        };
        let channel = |i: usize| {
            let info = opened.infos[i];
            let buffers = info.buffers;
            Channel {
                buffers: [buffers[0].cast(), buffers[1].cast()],
                ty: opened.types[i],
            }
        };
        let live = Box::new(Live {
            pump: UnsafeCell::new(pump),
            busy: AtomicBool::new(false),
            driver: self.driver,
            outputs: (0..opened.outputs).map(channel).collect(),
            inputs: (opened.outputs..opened.infos.len()).map(channel).collect(),
            block_outputs: opened.block_outputs,
            block_inputs: opened.block_inputs,
            frames: opened.frames,
            output_ready: opened.output_ready,
        });
        LIVE.store(Box::into_raw(live), Ordering::Release);
        // SAFETY: buffers made, callbacks live.
        let r = unsafe { self.call(|v, d| (v.start)(d)) };
        if r != ASE_OK {
            let pump = self.take_live().expect("stored just above");
            return Err((
                format!("{} would not start ({})", self.name, error_words(r)),
                Box::new(pump),
            ));
        }
        Ok(())
    }

    fn stop(&mut self) -> Option<Pump> {
        if LIVE.load(Ordering::Acquire).is_null() {
            return None;
        }
        // SAFETY: running; after `stop` returns the driver calls no more.
        unsafe { self.call(|v, d| (v.stop)(d)) };
        self.take_live()
    }

    fn close(&mut self) {
        if self.opened.take().is_some() && !self.driver.is_null() {
            // SAFETY: stopped, with buffers made.
            unsafe { self.call(|v, d| (v.dispose_buffers)(d)) };
        }
    }

    fn reload(&mut self) -> Result<(), String> {
        let _ = self.stop();
        self.close();
        // Let go first: many drivers allow one instance at a time.
        if !self.driver.is_null() {
            // SAFETY: the driver object made in `load`, released once.
            unsafe { self.call(|v, d| (v.release)(d)) };
            self.driver = std::ptr::null_mut();
        }
        let mut fresh = AsioDevice::load(&self.name)?;
        self.driver = std::mem::replace(&mut fresh.driver, std::ptr::null_mut());
        Ok(())
    }

    fn notices(&mut self) -> Vec<Notice> {
        let bits = NOTICES.swap(0, Ordering::AcqRel);
        let lost = LOST.swap(0, Ordering::AcqRel);
        let mut notices = Vec::new();
        if lost > 0 {
            notices.push(Notice::Lost(lost));
        }
        if bits & NOTICE_LATENCIES != 0 {
            notices.push(Notice::LatenciesChanged);
        }
        if bits & NOTICE_RESET != 0 {
            notices.push(Notice::Reset);
        } else if bits & NOTICE_RATE != 0 {
            notices.push(Notice::RateChanged);
        }
        notices
    }

    fn latencies(&mut self) -> Option<(u32, u32)> {
        let (mut input, mut output) = (0i32, 0i32);
        // SAFETY: out-parameters.
        let r = unsafe { self.call(|v, d| (v.get_latencies)(d, &mut input, &mut output)) };
        (r == ASE_OK).then(|| (input.max(0) as u32, output.max(0) as u32))
    }

    fn rate(&mut self) -> Option<u32> {
        self.sample_rate().map(|r| r.round() as u32)
    }

    fn control_panel(&mut self) -> Result<(), String> {
        // SAFETY: a plain call; the panel may run its own message loop.
        let r = unsafe { self.call(|v, d| (v.control_panel)(d)) };
        if r == ASE_OK {
            Ok(())
        } else {
            Err(format!(
                "{} has no control panel of its own ({})",
                self.name,
                error_words(r)
            ))
        }
    }

    fn windows_open(&self) -> bool {
        unsafe extern "system" fn count(hwnd: HWND, found: LPARAM) -> i32 {
            let mut pid = 0u32;
            // SAFETY: `found` is the `bool` below, for the call's length.
            unsafe {
                GetWindowThreadProcessId(hwnd, &mut pid);
                if pid == GetCurrentProcessId() && IsWindowVisible(hwnd) != 0 {
                    *(found as *mut bool) = true;
                    return 0;
                }
            }
            1
        }
        let mut found = false;
        // SAFETY: the callback writes only `found`, which outlives the call.
        unsafe { EnumWindows(Some(count), &mut found as *mut bool as LPARAM) };
        found
    }

    fn idle(&mut self, wait: Duration) {
        // SAFETY: this thread's own message queue.
        unsafe {
            MsgWaitForMultipleObjects(
                0,
                std::ptr::null(),
                0,
                wait.as_millis().min(u32::MAX as u128) as u32,
                QS_ALLINPUT,
            );
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

impl AsioDevice {
    /// Takes the running state back from the callbacks, once none is inside.
    fn take_live(&mut self) -> Option<Pump> {
        let live = LIVE.swap(std::ptr::null_mut(), Ordering::AcqRel);
        if live.is_null() {
            return None;
        }
        // SAFETY: made by `Box::into_raw` in `start`; no new callback can
        // reach it now, and one already inside is waited out.
        let live = unsafe { Box::from_raw(live) };
        while live.busy.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
        let mut pump = live.pump.into_inner();
        pump.pause();
        Some(pump)
    }
}

impl Drop for AsioDevice {
    fn drop(&mut self) {
        if self.driver.is_null() {
            return;
        }
        let _ = self.stop();
        self.close();
        // SAFETY: the driver object made in `load`, released once.
        unsafe { self.call(|v, d| (v.release)(d)) };
    }
}

//! WASAPI as Windows users with clicks need it: **exclusive** mode, and
//! **low-latency** shared mode.
//!
//! Reported: *clicky/crackly audio on Windows*. `cpal`'s WASAPI is shared
//! mode only, and in shared mode the callback comes once per **engine
//! period** — Windows's 10 ms, whatever buffer was asked for — through
//! Windows's mixer and its resampler. Two things it cannot do, done here on
//! a thread of Fontelle's own, event-driven, with the same [`Renderer`]:
//!
//! - **Exclusive** (`AUDCLNT_SHAREMODE_EXCLUSIVE`): Fontelle alone on the
//!   device, at the buffer chosen, in the device's own format
//!   ([`DeviceFormat`]), nothing mixed or converted on the way. Other
//!   programs are silent meanwhile — Windows's rule, said in the help line.
//! - **Low latency** (`IAudioClient3::InitializeSharedAudioStream`): still
//!   shared, but at the smallest engine period the driver allows rather
//!   than 10 ms ([`low_latency_period`]). The stream plays the engine's own
//!   mix format and rate, so a device set to another rate than the studio's
//!   is refused in words rather than played at the wrong speed.
//!
//! Dropouts are counted by whose they are ([`OutputStats`]): the device's
//! when it did not wake the thread on time ([`GapDetector`]) or, shared, its
//! buffer had run dry with Fontelle on time; Fontelle's when rendering took
//! longer than the buffer lasts (the [`Renderer`] times it).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use fontelle_audio_ipc::GapDetector;
use windows::Win32::Devices::Properties::DEVPKEY_Device_FriendlyName;
use windows::Win32::Foundation::{CloseHandle, HANDLE, PROPERTYKEY, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED, AUDCLNT_E_DEVICE_IN_USE,
    AUDCLNT_E_DEVICE_INVALIDATED, AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED,
    AUDCLNT_E_UNSUPPORTED_FORMAT, AUDCLNT_SHAREMODE_EXCLUSIVE, AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
    DEVICE_STATE_ACTIVE, IAudioClient, IAudioClient3, IAudioRenderClient, IMMDevice,
    IMMDeviceEnumerator, MMDeviceEnumerator, WAVE_FORMAT_PCM, WAVEFORMATEX, WAVEFORMATEXTENSIBLE,
    WAVEFORMATEXTENSIBLE_0, eConsole, eRender,
};
use windows::Win32::Media::KernelStreaming::{KSDATAFORMAT_SUBTYPE_PCM, WAVE_FORMAT_EXTENSIBLE};
use windows::Win32::Media::Multimedia::{KSDATAFORMAT_SUBTYPE_IEEE_FLOAT, WAVE_FORMAT_IEEE_FLOAT};
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize, STGM_READ,
};
use windows::Win32::System::Threading::{CreateEventW, SetEvent, WaitForMultipleObjects};
use windows::core::PCWSTR;

use crate::device::DeviceError;
use crate::device_format::DeviceFormat;
use crate::output::{
    EXCLUSIVE_AUTOMATIC, OutputChoice, OutputStats, OutputStatus, Renderer, WASAPI_EXCLUSIVE_HOST,
    WASAPI_LOW_LATENCY_HOST, low_latency_period,
};

/// Front left and right: the stereo the studio plays.
const SPEAKERS_STEREO: u32 = 0x3;

/// The most frames one wake-up may ask for; past it the thread's buffer is
/// not big enough and the device is refused, rather than reallocating on
/// the audio thread.
const MAX_FRAMES: usize = 16_384;

/// How long the thread waits for the device before deciding it has gone.
const DEVICE_SILENCE_MS: u32 = 2_000;

/// A HANDLE that may cross to the audio thread: kernel handles are
/// process-wide.
#[derive(Clone, Copy)]
struct Handle(HANDLE);
// SAFETY: a kernel handle, valid on any thread of the process.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

/// A running WASAPI stream of Fontelle's own.
pub(crate) struct WasapiStream {
    stop: Arc<AtomicBool>,
    wake: Handle,
    thread: Option<JoinHandle<()>>,
}

impl Drop for WasapiStream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // SAFETY: our event, closed below once the thread is done with it.
        unsafe {
            let _ = SetEvent(self.wake.0);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        // SAFETY: ours, and nobody waits on it now.
        unsafe {
            let _ = CloseHandle(self.wake.0);
        }
    }
}

/// What the audio thread reports once the device is open.
struct Opened {
    device: String,
    frames: u32,
    latency_frames: Option<u32>,
}

/// Opens `choice`'s device (the default endpoint for none) in exclusive or
/// low-latency mode, on a thread of its own that then plays `renderer`.
pub(crate) fn open(
    exclusive: bool,
    choice: &OutputChoice,
    sample_rate: u32,
    renderer: Renderer,
    stats: Arc<OutputStats>,
) -> Result<(WasapiStream, OutputStatus), DeviceError> {
    let stop = Arc::new(AtomicBool::new(false));
    // SAFETY: an unnamed auto-reset event, closed by the stream.
    let wake = unsafe { CreateEventW(None, false, false, PCWSTR::null()) }
        .map_err(|e| DeviceError(format!("no event for the audio thread: {e}")))?;
    let wake = Handle(wake);
    let (tx, rx) = channel();
    let device = choice.device.clone();
    let frames = choice.buffer_frames;
    let thread = {
        let stop = Arc::clone(&stop);
        std::thread::Builder::new()
            .name(if exclusive {
                "fontelle audio (WASAPI exclusive)".to_string()
            } else {
                "fontelle audio (WASAPI low latency)".to_string()
            })
            .spawn(move || {
                run(
                    exclusive,
                    device,
                    frames,
                    sample_rate,
                    renderer,
                    stats,
                    stop,
                    wake,
                    tx,
                )
            })
            .map_err(|e| DeviceError(format!("no thread for the audio: {e}")))?
    };
    let stream = WasapiStream {
        stop,
        wake,
        thread: Some(thread),
    };
    let opened = rx
        .recv_timeout(Duration::from_secs(10))
        .map_err(|_| DeviceError("the device did not open in time".to_string()))?
        .map_err(DeviceError)?;
    let status = OutputStatus {
        host: if exclusive {
            WASAPI_EXCLUSIVE_HOST
        } else {
            WASAPI_LOW_LATENCY_HOST
        }
        .to_string(),
        device: opened.device,
        sample_rate,
        buffer_frames: Some(opened.frames),
        latencies: opened.latency_frames.map(|out| (0, out)),
        fell_back: None,
    };
    Ok((stream, status))
}

/// A device's name as Windows shows it, which is also `cpal`'s name for it.
fn friendly_name(device: &IMMDevice) -> Option<String> {
    // SAFETY: COM calls on a live device; the string is freed after copying.
    unsafe {
        let store = device.OpenPropertyStore(STGM_READ).ok()?;
        let key = &DEVPKEY_Device_FriendlyName as *const _ as *const PROPERTYKEY;
        let value = store.GetValue(key).ok()?;
        let text = PropVariantToStringAlloc(&value).ok()?;
        let name = text.to_string().ok();
        CoTaskMemFree(Some(text.0 as *const _));
        name
    }
}

/// The render endpoint called `name`, or the default one.
fn find_device(name: Option<&str>) -> Result<(IMMDevice, String), String> {
    // SAFETY: COM calls on this thread, which initialised COM.
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| format!("Windows's device list would not open: {e}"))?;
        let Some(name) = name else {
            let device = enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .map_err(|_| "Windows has no default output".to_string())?;
            let shown = friendly_name(&device).unwrap_or_else(|| "the default output".into());
            return Ok((device, shown));
        };
        let all = enumerator
            .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
            .map_err(|e| format!("Windows's device list would not open: {e}"))?;
        for i in 0..all.GetCount().unwrap_or(0) {
            if let Ok(device) = all.Item(i)
                && friendly_name(&device).as_deref() == Some(name)
            {
                return Ok((device, name.to_string()));
            }
        }
        Err(format!("{name} is not there"))
    }
}

/// A WAVEFORMATEXTENSIBLE for stereo `format` at `rate`.
fn wave_format(format: DeviceFormat, rate: u32) -> WAVEFORMATEXTENSIBLE {
    let (container, valid, sub) = match format {
        DeviceFormat::F32 => (32, 32, KSDATAFORMAT_SUBTYPE_IEEE_FLOAT),
        DeviceFormat::I16 => (16, 16, KSDATAFORMAT_SUBTYPE_PCM),
        DeviceFormat::I24 => (24, 24, KSDATAFORMAT_SUBTYPE_PCM),
        DeviceFormat::I24In32 => (32, 24, KSDATAFORMAT_SUBTYPE_PCM),
        DeviceFormat::I32 => (32, 32, KSDATAFORMAT_SUBTYPE_PCM),
    };
    let block = 2 * container / 8;
    WAVEFORMATEXTENSIBLE {
        Format: WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_EXTENSIBLE as u16,
            nChannels: 2,
            nSamplesPerSec: rate,
            nAvgBytesPerSec: rate * u32::from(block),
            nBlockAlign: block,
            wBitsPerSample: container,
            cbSize: (std::mem::size_of::<WAVEFORMATEXTENSIBLE>()
                - std::mem::size_of::<WAVEFORMATEX>()) as u16,
        },
        Samples: WAVEFORMATEXTENSIBLE_0 {
            wValidBitsPerSample: valid,
        },
        dwChannelMask: SPEAKERS_STEREO,
        SubFormat: sub,
    }
}

/// What a mix format (`GetMixFormat`) holds, as a format this can write.
///
/// # Safety
/// `format` points at a live WAVEFORMATEX, extensible when its tag says so.
unsafe fn mix_format(format: *const WAVEFORMATEX) -> Option<DeviceFormat> {
    // SAFETY: the caller's promise; read unaligned, as Windows packs these.
    let f = unsafe { std::ptr::read_unaligned(format) };
    let tag = u32::from(f.wFormatTag);
    let (float, valid) = if tag == WAVE_FORMAT_EXTENSIBLE {
        // SAFETY: an extensible format, by its tag.
        let x = unsafe { std::ptr::read_unaligned(format.cast::<WAVEFORMATEXTENSIBLE>()) };
        let sub = x.SubFormat;
        // SAFETY: a plain union of one u16.
        let valid = unsafe { x.Samples.wValidBitsPerSample };
        (sub == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT, valid)
    } else {
        (tag == WAVE_FORMAT_IEEE_FLOAT, f.wBitsPerSample)
    };
    if !float && tag != WAVE_FORMAT_EXTENSIBLE && tag != WAVE_FORMAT_PCM {
        return None;
    }
    match (float, f.wBitsPerSample, valid) {
        (true, 32, _) => Some(DeviceFormat::F32),
        (false, 16, _) => Some(DeviceFormat::I16),
        (false, 24, _) => Some(DeviceFormat::I24),
        (false, 32, 24) => Some(DeviceFormat::I24In32),
        (false, 32, _) => Some(DeviceFormat::I32),
        _ => None,
    }
}

/// An `AUDCLNT_E_…` in words a person can act on.
fn words(device: &str, e: &windows::core::Error) -> String {
    let code = e.code();
    if code == AUDCLNT_E_DEVICE_IN_USE {
        format!("{device} is in use by another program in exclusive mode")
    } else if code == AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED {
        format!(
            "Windows does not let programs take {device} exclusively — Sound settings \u{2192} \
             the device's properties \u{2192} Advanced \u{2192} \"Allow applications to take \
             exclusive control\""
        )
    } else if code == AUDCLNT_E_UNSUPPORTED_FORMAT {
        format!("{device} will not play this format")
    } else if code == AUDCLNT_E_DEVICE_INVALIDATED {
        format!("{device} went away")
    } else {
        format!("{device}: {e}")
    }
}

const fn hns(frames: u32, rate: u32) -> i64 {
    (frames as i64 * 10_000_000 + rate as i64 / 2) / rate as i64
}

/// The device, opened.
struct Device {
    client: IAudioClient,
    render: IAudioRenderClient,
    event: Handle,
    format: DeviceFormat,
    channels: usize,
    /// The whole endpoint buffer, frames.
    buffer: u32,
    /// The period the device wakes the thread at, frames.
    period: u32,
    exclusive: bool,
}

impl Drop for Device {
    fn drop(&mut self) {
        // SAFETY: ours.
        unsafe {
            let _ = self.client.Stop();
            let _ = CloseHandle(self.event.0);
        }
    }
}

fn open_exclusive(
    device: &IMMDevice,
    name: &str,
    frames: Option<u32>,
    rate: u32,
) -> Result<Device, String> {
    // SAFETY: COM calls on this thread; formats outlive the calls that read
    // them.
    unsafe {
        let mut client: IAudioClient = device
            .Activate(CLSCTX_ALL, None)
            .map_err(|e| words(name, &e))?;
        // Float first — the mix is float — then the integers devices play.
        let format = [
            DeviceFormat::F32,
            DeviceFormat::I32,
            DeviceFormat::I24In32,
            DeviceFormat::I24,
            DeviceFormat::I16,
        ]
        .into_iter()
        .find(|f| {
            let wf = wave_format(*f, rate);
            client
                .IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE, &wf.Format, None)
                .is_ok()
        })
        .ok_or_else(|| {
            format!(
                "{name} will not play {} kHz stereo in exclusive mode; set it to that rate in \
                 Sound settings, or use plain WASAPI, which converts",
                rate / 1000
            )
        })?;
        let wf = wave_format(format, rate);
        let (mut default_period, mut min_period) = (0i64, 0i64);
        client
            .GetDevicePeriod(Some(&mut default_period), Some(&mut min_period))
            .map_err(|e| words(name, &e))?;
        let mut period = hns(frames.unwrap_or(EXCLUSIVE_AUTOMATIC), rate).max(min_period);
        let flags = AUDCLNT_STREAMFLAGS_EVENTCALLBACK;
        match client.Initialize(
            AUDCLNT_SHAREMODE_EXCLUSIVE,
            flags,
            period,
            period,
            &wf.Format,
            None,
        ) {
            Ok(()) => {}
            // The device wants a buffer on its own boundary: it says which,
            // and a fresh client is asked for exactly that.
            Err(e) if e.code() == AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED => {
                let aligned = client.GetBufferSize().map_err(|e| words(name, &e))?;
                period = hns(aligned, rate);
                client = device
                    .Activate(CLSCTX_ALL, None)
                    .map_err(|e| words(name, &e))?;
                client
                    .Initialize(
                        AUDCLNT_SHAREMODE_EXCLUSIVE,
                        flags,
                        period,
                        period,
                        &wf.Format,
                        None,
                    )
                    .map_err(|e| words(name, &e))?;
            }
            Err(e) => return Err(words(name, &e)),
        }
        finish(client, format, 2, None, true, name)
    }
}

fn open_low_latency(
    device: &IMMDevice,
    name: &str,
    frames: Option<u32>,
    rate: u32,
) -> Result<Device, String> {
    // SAFETY: COM calls on this thread; the mix format is freed once read.
    unsafe {
        let client: IAudioClient3 = device
            .Activate(CLSCTX_ALL, None)
            .map_err(|e| words(name, &e))?;
        let mix = client.GetMixFormat().map_err(|e| words(name, &e))?;
        let mix_rate = std::ptr::read_unaligned(mix).nSamplesPerSec;
        let channels = usize::from(std::ptr::read_unaligned(mix).nChannels);
        let result = (|| {
            if mix_rate != rate {
                return Err(format!(
                    "{name} runs at {:.1} kHz in Windows and Fontelle at {} kHz; low-latency \
                     mode plays the device's own rate, so set it to {} kHz in Sound settings, \
                     or use plain WASAPI, which converts",
                    f64::from(mix_rate) / 1000.0,
                    rate / 1000,
                    rate / 1000
                ));
            }
            let format = mix_format(mix)
                .ok_or_else(|| format!("{name} mixes in a format Fontelle cannot write"))?;
            let (mut default, mut fundamental, mut min, mut max) = (0u32, 0u32, 0u32, 0u32);
            client
                .GetSharedModeEnginePeriod(mix, &mut default, &mut fundamental, &mut min, &mut max)
                .map_err(|e| words(name, &e))?;
            let period = low_latency_period(frames, fundamental, min, max);
            client
                .InitializeSharedAudioStream(AUDCLNT_STREAMFLAGS_EVENTCALLBACK, period, mix, None)
                .map_err(|e| words(name, &e))?;
            Ok((format, period))
        })();
        CoTaskMemFree(Some(mix as *const _));
        let (format, period) = result?;
        let client: IAudioClient = client.into();
        finish(client, format, channels, Some(period), false, name)
    }
}

/// The event, the render client, a buffer of silence, and start.
///
/// # Safety
/// `client` is initialised, on this thread.
unsafe fn finish(
    client: IAudioClient,
    format: DeviceFormat,
    channels: usize,
    period: Option<u32>,
    exclusive: bool,
    name: &str,
) -> Result<Device, String> {
    // SAFETY: the caller's promise; the event is the device's, closed with it.
    unsafe {
        let event = CreateEventW(None, false, false, PCWSTR::null())
            .map_err(|e| format!("no event for {name}: {e}"))?;
        let event = Handle(event);
        let close = |why: String| {
            let _ = CloseHandle(event.0);
            why
        };
        client
            .SetEventHandle(event.0)
            .map_err(|e| close(words(name, &e)))?;
        let buffer = client.GetBufferSize().map_err(|e| close(words(name, &e)))?;
        if buffer as usize > MAX_FRAMES || channels == 0 || channels > 8 {
            return Err(close(format!(
                "{name} wants a buffer of {buffer} frames of {channels} channels, past what \
                 Fontelle's audio thread holds"
            )));
        }
        let render: IAudioRenderClient = client.GetService().map_err(|e| close(words(name, &e)))?;
        // Exclusive event-driven mode: silence in the whole buffer before the
        // start, so the first event finds it playing.
        if exclusive && let Ok(data) = render.GetBuffer(buffer) {
            let _ = data;
            let _ = render.ReleaseBuffer(buffer, AUDCLNT_BUFFERFLAGS_SILENT.0 as u32);
        }
        client.Start().map_err(|e| close(words(name, &e)))?;
        Ok(Device {
            client,
            render,
            event,
            format,
            channels,
            buffer,
            period: period.unwrap_or(buffer),
            exclusive,
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn run(
    exclusive: bool,
    device_name: Option<String>,
    frames: Option<u32>,
    rate: u32,
    mut renderer: Renderer,
    stats: Arc<OutputStats>,
    stop: Arc<AtomicBool>,
    wake: Handle,
    opened: Sender<Result<Opened, String>>,
) {
    // SAFETY: once, for this thread, undone at its end.
    let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    let device = find_device(device_name.as_deref()).and_then(|(device, name)| {
        let open = if exclusive {
            open_exclusive(&device, &name, frames, rate)
        } else {
            open_low_latency(&device, &name, frames, rate)
        };
        open.map(|d| (d, name))
    });
    let (device, name) = match device {
        Ok(device) => device,
        Err(why) => {
            let _ = opened.send(Err(why));
            if com {
                // SAFETY: paired with the initialise above.
                unsafe { CoUninitialize() };
            }
            return;
        }
    };
    // SAFETY: a plain query.
    let latency = unsafe { device.client.GetStreamLatency() }
        .ok()
        .map(|hns| (hns as f64 * f64::from(rate) / 10_000_000.0).round() as u32);
    let _ = opened.send(Ok(Opened {
        device: name,
        frames: device.period,
        latency_frames: latency.map(|l| l + device.buffer),
    }));
    play(&device, &mut renderer, &stats, &stop, wake, rate);
    drop(device);
    if com {
        // SAFETY: paired with the initialise above.
        unsafe { CoUninitialize() };
    }
}

/// The audio thread's loop: wait for the device, render, hand over.
fn play(
    device: &Device,
    renderer: &mut Renderer,
    stats: &OutputStats,
    stop: &AtomicBool,
    wake: Handle,
    rate: u32,
) {
    let channels = device.channels;
    // Every buffer the loop needs, made before it starts (INVARIANT 1).
    let mut mix = vec![0.0f32; MAX_FRAMES * channels];
    let period = Duration::from_secs_f64(f64::from(device.period) / f64::from(rate));
    let mut gaps = GapDetector::new(period.as_nanos() as u64);
    let origin = Instant::now();
    let mut was_late = false;
    let mut started = false;
    let handles = [device.event.0, wake.0];
    while !stop.load(Ordering::Relaxed) {
        // SAFETY: two live events.
        let woke = unsafe { WaitForMultipleObjects(&handles, false, DEVICE_SILENCE_MS) };
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if woke != WAIT_OBJECT_0 {
            // Neither the device nor a stop in two seconds: it has gone.
            stats.set_lost();
            break;
        }
        let lost = gaps.tick(origin.elapsed().as_nanos() as u64);
        if lost > 0 && !was_late {
            stats.note_device_xruns(u64::from(lost));
        }
        // SAFETY: COM calls on the thread that opened the device; the buffer
        // GetBuffer hands out is ours until ReleaseBuffer.
        let frames = unsafe {
            if device.exclusive {
                device.buffer
            } else {
                let Ok(padding) = device.client.GetCurrentPadding() else {
                    stats.set_lost();
                    break;
                };
                // Shared: an empty buffer at a wake-up is one the engine ran
                // dry on — the device's doing if Fontelle was on time.
                if started && padding == 0 && !was_late {
                    stats.note_device_xruns(1);
                }
                device.buffer.saturating_sub(padding)
            }
        };
        if frames == 0 {
            continue;
        }
        let frames = (frames as usize).min(MAX_FRAMES);
        let samples = frames * channels;
        let t0 = Instant::now();
        // SAFETY: as above.
        unsafe {
            let data = match device.render.GetBuffer(frames as u32) {
                Ok(data) => data,
                Err(e) if e.code() == AUDCLNT_E_DEVICE_INVALIDATED => {
                    stats.set_lost();
                    break;
                }
                Err(_) => continue,
            };
            let bytes = std::slice::from_raw_parts_mut(data, samples * device.format.bytes());
            if device.format == DeviceFormat::F32 && data.cast::<f32>().is_aligned() {
                // The device plays the mix as it is: rendered straight in.
                let out = std::slice::from_raw_parts_mut(data.cast::<f32>(), samples);
                renderer.render(out, channels);
            } else {
                renderer.render(&mut mix[..samples], channels);
                device.format.write(&mix[..samples], bytes);
            }
            let _ = device.render.ReleaseBuffer(frames as u32, 0);
        }
        // Late is the renderer's call (it times itself); remembered so a gap
        // it caused is not blamed on the device.
        was_late = t0.elapsed() >= period;
        started = true;
    }
}

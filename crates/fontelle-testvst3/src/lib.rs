//! Three tiny VST 3 plugins, built so `fontelle-host` has a real module of
//! the format to load in its tests.
//!
//! The twin of `fontelle-testplug` (CLAP) and `fontelle-testlv2`, in VST 3
//! form, so every test that holds the CLAP arm can be asked again of this
//! one (`docs/vst-plan.md` §2.3):
//!
//! - **Gain**: a stereo effect with a normalised gain (four times at full),
//!   an invert switch, a hidden parameter and a read-only one; an *aux*
//!   input bus it ducks by (the sidechain) and an aux mono output it also
//!   renders; a fixed latency; state on both halves; and an editor that
//!   registers a timer with the host's run loop, asks to be resized on its
//!   first fire and performs an edit on its third.
//! - **Sine**: an instrument with a main pair and a mono *sub* output that
//!   renders nothing when handed fewer buses than it declared — the lesson
//!   the OneTrick drum synths taught the CLAP arm. Its level is scaled by
//!   the mod wheel, ducked by aftertouch and bent by the bend, all three
//!   reached as the parameters `IMidiMapping` maps them to; a note
//!   expression of `kTuningTypeID` bends one note.
//! - **Combined**: one object that is both component and controller, which
//!   the specification allows and some plugins do.
//!
//! Everything is `unsafe` at the boundary because the binding is: the crate
//! wraps the SDK's C ABI and nothing else.

#![allow(non_snake_case)]
#![allow(clippy::missing_safety_doc)]
// The SDK's enums are `c_uint` here and `c_int` on Windows, so a cast that
// is a no-op on one platform is the conversion on the other.
#![allow(clippy::unnecessary_cast)]

use std::cell::Cell;
use std::ffi::{CStr, c_void};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use vst3::Steinberg::Linux::*;
use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{Class, ComPtr, ComRef, ComWrapper, Interface, uid};

/// The class ids, as the host writes them: the four words of the `uid`, as
/// eight hex digits each — the SDK's own `FUID::toString` spelling, which
/// is also what `moduleinfo.json` carries.
pub const GAIN_ID: &str = "464F50554C4C00015445535447414942";
pub const GAIN_CONTROLLER_ID: &str = "464F50554C4C00025445535447414943";
pub const SINE_ID: &str = "464F50554C4C000354455354534E4542";
pub const SINE_CONTROLLER_ID: &str = "464F50554C4C000454455354534E4543";
pub const COMBINED_ID: &str = "464F50554C4C000554455354434F4D42";

const GAIN_CID: TUID = uid(0x464F5055, 0x4C4C0001, 0x54455354, 0x47414942);
const GAIN_CONTROLLER_CID: TUID = uid(0x464F5055, 0x4C4C0002, 0x54455354, 0x47414943);
const SINE_CID: TUID = uid(0x464F5055, 0x4C4C0003, 0x54455354, 0x534E4542);
const SINE_CONTROLLER_CID: TUID = uid(0x464F5055, 0x4C4C0004, 0x54455354, 0x534E4543);
const COMBINED_CID: TUID = uid(0x464F5055, 0x4C4C0005, 0x54455354, 0x434F4D42);

pub const GAIN_NAME: &str = "Fontelle Test Gain (VST3)";
pub const SINE_NAME: &str = "Fontelle Test Sine (VST3)";
pub const COMBINED_NAME: &str = "Fontelle Test Combined (VST3)";
pub const VENDOR: &str = "Fopull LLC";
pub const VERSION: &str = "1.0.0";

/// What the gain declares as its latency.
pub const GAIN_LATENCY_SAMPLES: u32 = 32;
/// How big the gain's view says it is.
pub const VIEW_WIDTH: u32 = 320;
pub const VIEW_HEIGHT: u32 = 200;
/// The marker the gain's **controller** writes into its own state stream,
/// beside the component's — so a host that saved only one half is found out.
pub const CONTROLLER_MAGIC: &[u8; 4] = b"FGC1";
const COMPONENT_MAGIC: &[u8; 4] = b"FGN1";
/// The gain's read-only parameter, which reads `1.0` once the controller
/// has been handed a state stream carrying [`CONTROLLER_MAGIC`].
pub const SEEN_CONTROLLER_STATE_PARAM: u32 = 3;
/// How far a full bend goes, in semitones.
pub const BEND_RANGE_SEMITONES: f32 = 2.0;

// The sine's parameters, and the controllers its `IMidiMapping` maps.
const LEVEL: ParamID = 7;
const WHEEL: ParamID = 8;
const BEND: ParamID = 9;
const PRESSURE: ParamID = 10;

/// The `moduleinfo.json` for this module, plus one class the library does
/// not hold, named `phantom`. Trailing commas included, because the SDK's
/// own writer leaves them and its reader accepts them.
pub fn moduleinfo_json_with_phantom(phantom: &str) -> String {
    let class = |cid: &str, name: &str, sub: &str| {
        format!(
            r#"    {{
      "CID": "{cid}",
      "Category": "Audio Module Class",
      "Name": "{name}",
      "Vendor": "{VENDOR}",
      "Version": "{VERSION}",
      "SDKVersion": "VST 3.7.9",
      "Sub Categories": [
        {sub}
      ],
      "Class Flags": 0,
      "Cardinality": 2147483647,
      "Snapshots": [
      ],
    }},
"#
        )
    };
    format!(
        r#"{{
  "Name": "fontelle-testvst3",
  "Version": "{VERSION}",
  "Factory Info": {{
    "Vendor": "{VENDOR}",
    "URL": "https://fopull.com",
    "E-Mail": "",
    "Flags": {{
      "Unicode": true,
      "Classes Discardable": false,
      "Component Non Discardable": false,
    }},
  }},
  "Classes": [
{}{}{}{}    {{
      "CID": "{GAIN_CONTROLLER_ID}",
      "Category": "Component Controller Class",
      "Name": "{GAIN_NAME}",
      "Vendor": "{VENDOR}",
      "Version": "{VERSION}",
      "SDKVersion": "VST 3.7.9",
      "Class Flags": 0,
      "Cardinality": 2147483647,
      "Snapshots": [
      ],
    }},
  ],
}}
"#,
        class(GAIN_ID, GAIN_NAME, r#""Fx", "Tools""#),
        class(SINE_ID, SINE_NAME, r#""Instrument", "Synth""#),
        class(COMBINED_ID, COMBINED_NAME, r#""Fx""#),
        class("00000000000000000000000000000001", phantom, r#""Fx""#),
    )
}

// ------------------------------------------------------------------ strings

fn copy_cstring(src: &str, dst: &mut [char8]) {
    let bytes = src.as_bytes();
    let n = bytes.len().min(dst.len().saturating_sub(1));
    for (d, s) in dst.iter_mut().zip(&bytes[..n]) {
        *d = *s as char8;
    }
    dst[n] = 0;
}

fn copy_wstring(src: &str, dst: &mut [TChar]) {
    let mut n = 0;
    for (d, s) in dst.iter_mut().zip(src.encode_utf16()) {
        *d = s;
        n += 1;
    }
    if n < dst.len() {
        dst[n] = 0;
    } else if let Some(last) = dst.last_mut() {
        *last = 0;
    }
}

unsafe fn bus_info(
    bus: *mut BusInfo,
    media: MediaType,
    dir: BusDirection,
    channels: i32,
    name: &str,
    aux: bool,
) -> tresult {
    let bus = unsafe { &mut *bus };
    bus.mediaType = media;
    bus.direction = dir;
    bus.channelCount = channels;
    copy_wstring(name, &mut bus.name);
    bus.busType = if aux {
        BusTypes_::kAux as BusType
    } else {
        BusTypes_::kMain as BusType
    };
    bus.flags = if aux {
        0
    } else {
        BusInfo_::BusFlags_::kDefaultActive as u32
    };
    kResultOk
}

const K_AUDIO: MediaType = MediaTypes_::kAudio as MediaType;
const K_EVENT: MediaType = MediaTypes_::kEvent as MediaType;
const K_INPUT: BusDirection = BusDirections_::kInput as BusDirection;
const K_OUTPUT: BusDirection = BusDirections_::kOutput as BusDirection;

/// Reads every point off the block's parameter changes, last value per id.
unsafe fn each_change(data: &ProcessData, mut apply: impl FnMut(ParamID, f64)) {
    let Some(changes) = (unsafe { ComRef::from_raw(data.inputParameterChanges) }) else {
        return;
    };
    for index in 0..unsafe { changes.getParameterCount() } {
        let Some(queue) = (unsafe { ComRef::from_raw(changes.getParameterData(index)) }) else {
            continue;
        };
        let id = unsafe { queue.getParameterId() };
        let points = unsafe { queue.getPointCount() };
        if points <= 0 {
            continue;
        }
        let mut offset = 0;
        let mut value = 0.0;
        if unsafe { queue.getPoint(points - 1, &mut offset, &mut value) } == kResultOk {
            apply(id, value);
        }
    }
}

unsafe fn channel<'a>(
    buses: *mut AudioBusBuffers,
    bus: usize,
    ch: usize,
    n: usize,
) -> &'a mut [f32] {
    let bus = unsafe { &*buses.add(bus) };
    let ptr = unsafe { *bus.__field0.channelBuffers32.add(ch) };
    unsafe { std::slice::from_raw_parts_mut(ptr, n) }
}

/// An `IBStream` written to or read from in whole.
unsafe fn write_all(stream: *mut IBStream, bytes: &[u8]) -> tresult {
    let Some(stream) = (unsafe { ComRef::from_raw(stream) }) else {
        return kInvalidArgument;
    };
    let mut written = 0;
    unsafe {
        stream.write(
            bytes.as_ptr() as *mut c_void,
            bytes.len() as i32,
            &mut written,
        )
    }
}

unsafe fn read_all(stream: *mut IBStream) -> Vec<u8> {
    let Some(stream) = (unsafe { ComRef::from_raw(stream) }) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut chunk = [0u8; 256];
    loop {
        let mut read = 0;
        let r = unsafe { stream.read(chunk.as_mut_ptr() as *mut c_void, 256, &mut read) };
        if r != kResultOk || read <= 0 {
            break;
        }
        out.extend_from_slice(&chunk[..read as usize]);
    }
    out
}

// ---------------------------------------------------------------- the gain

struct GainProcessor {
    /// Normalised: the gain is four times this.
    gain: AtomicU64,
    invert: AtomicBool,
}

impl Class for GainProcessor {
    type Interfaces = (
        IComponent,
        IAudioProcessor,
        IProcessContextRequirements,
        IConnectionPoint,
    );
}

impl GainProcessor {
    fn new() -> Self {
        Self {
            gain: AtomicU64::new(0.25f64.to_bits()),
            invert: AtomicBool::new(false),
        }
    }
    fn gain(&self) -> f32 {
        f64::from_bits(self.gain.load(Ordering::Relaxed)) as f32 * 4.0
    }
}

impl IPluginBaseTrait for GainProcessor {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }
    unsafe fn terminate(&self) -> tresult {
        kResultOk
    }
}

impl IConnectionPointTrait for GainProcessor {
    unsafe fn connect(&self, _other: *mut IConnectionPoint) -> tresult {
        kResultOk
    }
    unsafe fn disconnect(&self, _other: *mut IConnectionPoint) -> tresult {
        kResultOk
    }
    unsafe fn notify(&self, _message: *mut IMessage) -> tresult {
        kResultOk
    }
}

impl IComponentTrait for GainProcessor {
    unsafe fn getControllerClassId(&self, class_id: *mut TUID) -> tresult {
        unsafe { *class_id = GAIN_CONTROLLER_CID };
        kResultOk
    }
    unsafe fn setIoMode(&self, _mode: IoMode) -> tresult {
        kResultOk
    }
    unsafe fn getBusCount(&self, media: MediaType, _dir: BusDirection) -> i32 {
        // Two each way: the main pair and an aux — the key in, a mono aux out.
        if media == K_AUDIO { 2 } else { 0 }
    }
    unsafe fn getBusInfo(
        &self,
        media: MediaType,
        dir: BusDirection,
        index: i32,
        bus: *mut BusInfo,
    ) -> tresult {
        if media != K_AUDIO {
            return kInvalidArgument;
        }
        match (dir, index) {
            (K_INPUT, 0) => unsafe { bus_info(bus, media, dir, 2, "Input", false) },
            (K_INPUT, 1) => unsafe { bus_info(bus, media, dir, 2, "Key", true) },
            (K_OUTPUT, 0) => unsafe { bus_info(bus, media, dir, 2, "Output", false) },
            (K_OUTPUT, 1) => unsafe { bus_info(bus, media, dir, 1, "Aux", true) },
            _ => kInvalidArgument,
        }
    }
    unsafe fn getRoutingInfo(&self, _in: *mut RoutingInfo, _out: *mut RoutingInfo) -> tresult {
        kNotImplemented
    }
    unsafe fn activateBus(
        &self,
        _media: MediaType,
        _dir: BusDirection,
        _index: i32,
        _state: TBool,
    ) -> tresult {
        kResultOk
    }
    unsafe fn setActive(&self, _state: TBool) -> tresult {
        kResultOk
    }
    unsafe fn setState(&self, state: *mut IBStream) -> tresult {
        let bytes = unsafe { read_all(state) };
        if bytes.len() < 13 || &bytes[..4] != COMPONENT_MAGIC {
            return kResultFalse;
        }
        let gain = f64::from_le_bytes(bytes[4..12].try_into().unwrap());
        self.gain.store(gain.to_bits(), Ordering::Relaxed);
        self.invert.store(bytes[12] != 0, Ordering::Relaxed);
        kResultOk
    }
    unsafe fn getState(&self, state: *mut IBStream) -> tresult {
        let mut bytes = COMPONENT_MAGIC.to_vec();
        bytes.extend_from_slice(&f64::from_bits(self.gain.load(Ordering::Relaxed)).to_le_bytes());
        bytes.push(u8::from(self.invert.load(Ordering::Relaxed)));
        unsafe { write_all(state, &bytes) }
    }
}

impl IAudioProcessorTrait for GainProcessor {
    unsafe fn setBusArrangements(
        &self,
        _i: *mut SpeakerArrangement,
        _ni: i32,
        _o: *mut SpeakerArrangement,
        _no: i32,
    ) -> tresult {
        kResultOk
    }
    unsafe fn getBusArrangement(
        &self,
        dir: BusDirection,
        index: i32,
        arr: *mut SpeakerArrangement,
    ) -> tresult {
        unsafe {
            *arr = if dir == K_OUTPUT && index == 1 {
                SpeakerArr::kMono
            } else {
                SpeakerArr::kStereo
            }
        };
        kResultOk
    }
    unsafe fn canProcessSampleSize(&self, size: i32) -> tresult {
        if size == SymbolicSampleSizes_::kSample32 as i32 {
            kResultOk
        } else {
            kResultFalse
        }
    }
    unsafe fn getLatencySamples(&self) -> u32 {
        GAIN_LATENCY_SAMPLES
    }
    unsafe fn setupProcessing(&self, _setup: *mut ProcessSetup) -> tresult {
        kResultOk
    }
    unsafe fn setProcessing(&self, _state: TBool) -> tresult {
        kResultOk
    }
    unsafe fn process(&self, data: *mut ProcessData) -> tresult {
        let data = unsafe { &*data };
        unsafe {
            each_change(data, |id, value| match id {
                0 => self.gain.store(value.to_bits(), Ordering::Relaxed),
                1 => self.invert.store(value >= 0.5, Ordering::Relaxed),
                _ => {}
            })
        };
        // Handed fewer buses than declared, the fixture does nothing — the
        // rule several real plugins apply, and what the host is tested for.
        if data.numInputs < 2 || data.numOutputs < 2 {
            return kResultOk;
        }
        let n = data.numSamples as usize;
        let sign = if self.invert.load(Ordering::Relaxed) {
            -1.0
        } else {
            1.0
        };
        let gain = self.gain() * sign;
        for ch in 0..2 {
            let input = unsafe { channel(data.inputs, 0, ch, n) };
            let key = unsafe { channel(data.inputs, 1, ch, n) };
            let output = unsafe { channel(data.outputs, 0, ch, n) };
            for i in 0..n {
                output[i] = input[i] * gain * (1.0 - key[i]);
            }
        }
        let aux = unsafe { channel(data.outputs, 1, 0, n) };
        aux.fill(0.25);
        kResultOk
    }
    unsafe fn getTailSamples(&self) -> u32 {
        0
    }
}

impl IProcessContextRequirementsTrait for GainProcessor {
    unsafe fn getProcessContextRequirements(&self) -> u32 {
        0
    }
}

/// The gain's controller: four parameters, two state halves, a view.
struct GainController {
    values: Mutex<[f64; 4]>,
    handler: Mutex<Option<ComPtr<IComponentHandler>>>,
}

impl Class for GainController {
    type Interfaces = (IEditController, IConnectionPoint);
}

impl GainController {
    fn new() -> Self {
        Self {
            values: Mutex::new([0.25, 0.0, 0.0, 0.0]),
            handler: Mutex::new(None),
        }
    }
}

impl IPluginBaseTrait for GainController {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }
    unsafe fn terminate(&self) -> tresult {
        *self.handler.lock().unwrap() = None;
        kResultOk
    }
}

impl IConnectionPointTrait for GainController {
    unsafe fn connect(&self, _other: *mut IConnectionPoint) -> tresult {
        kResultOk
    }
    unsafe fn disconnect(&self, _other: *mut IConnectionPoint) -> tresult {
        kResultOk
    }
    unsafe fn notify(&self, _message: *mut IMessage) -> tresult {
        kResultOk
    }
}

impl IEditControllerTrait for GainController {
    unsafe fn setComponentState(&self, state: *mut IBStream) -> tresult {
        let bytes = unsafe { read_all(state) };
        if bytes.len() < 13 || &bytes[..4] != COMPONENT_MAGIC {
            return kResultFalse;
        }
        let mut values = self.values.lock().unwrap();
        values[0] = f64::from_le_bytes(bytes[4..12].try_into().unwrap());
        values[1] = f64::from(bytes[12]);
        kResultOk
    }
    unsafe fn setState(&self, state: *mut IBStream) -> tresult {
        let bytes = unsafe { read_all(state) };
        if bytes.len() >= 4 && &bytes[..4] == CONTROLLER_MAGIC {
            self.values.lock().unwrap()[3] = 1.0;
            kResultOk
        } else {
            kResultFalse
        }
    }
    unsafe fn getState(&self, state: *mut IBStream) -> tresult {
        unsafe { write_all(state, CONTROLLER_MAGIC) }
    }
    unsafe fn getParameterCount(&self) -> i32 {
        4
    }
    unsafe fn getParameterInfo(&self, index: i32, info: *mut ParameterInfo) -> tresult {
        let info = unsafe { &mut *info };
        let (title, units, steps, default, flags) = match index {
            0 => (
                "Gain",
                "x",
                0,
                0.25,
                ParameterInfo_::ParameterFlags_::kCanAutomate,
            ),
            1 => (
                "Invert",
                "",
                1,
                0.0,
                ParameterInfo_::ParameterFlags_::kCanAutomate,
            ),
            2 => (
                "Hidden",
                "",
                0,
                0.0,
                ParameterInfo_::ParameterFlags_::kIsHidden,
            ),
            3 => (
                "Seen controller state",
                "",
                1,
                0.0,
                ParameterInfo_::ParameterFlags_::kIsReadOnly,
            ),
            _ => return kInvalidArgument,
        };
        info.id = index as ParamID;
        copy_wstring(title, &mut info.title);
        copy_wstring(title, &mut info.shortTitle);
        copy_wstring(units, &mut info.units);
        info.stepCount = steps;
        info.defaultNormalizedValue = default;
        info.unitId = 0;
        info.flags = flags as i32;
        kResultOk
    }
    unsafe fn getParamStringByValue(
        &self,
        id: ParamID,
        value: f64,
        string: *mut String128,
    ) -> tresult {
        let text = match id {
            0 => format!("{:.2} x", value * 4.0),
            1 => if value >= 0.5 { "On" } else { "Off" }.to_string(),
            _ => format!("{value}"),
        };
        copy_wstring(&text, unsafe { &mut *string });
        kResultOk
    }
    unsafe fn getParamValueByString(
        &self,
        _id: ParamID,
        _string: *mut TChar,
        _value: *mut f64,
    ) -> tresult {
        kNotImplemented
    }
    unsafe fn normalizedParamToPlain(&self, id: ParamID, value: f64) -> f64 {
        if id == 0 { value * 4.0 } else { value }
    }
    unsafe fn plainParamToNormalized(&self, id: ParamID, plain: f64) -> f64 {
        if id == 0 { plain / 4.0 } else { plain }
    }
    unsafe fn getParamNormalized(&self, id: ParamID) -> f64 {
        self.values
            .lock()
            .unwrap()
            .get(id as usize)
            .copied()
            .unwrap_or(0.0)
    }
    unsafe fn setParamNormalized(&self, id: ParamID, value: f64) -> tresult {
        match self.values.lock().unwrap().get_mut(id as usize) {
            Some(slot) => *slot = value,
            None => return kInvalidArgument,
        }
        // The hidden parameter set to one is the fixture's way of asking
        // for a restart, so the host's recording of the request is testable.
        if id == 2
            && value >= 1.0
            && let Some(handler) = self.handler.lock().unwrap().as_ref()
        {
            unsafe { handler.restartComponent(RestartFlags_::kLatencyChanged as i32) };
        }
        kResultOk
    }
    unsafe fn setComponentHandler(&self, handler: *mut IComponentHandler) -> tresult {
        *self.handler.lock().unwrap() =
            unsafe { ComRef::from_raw(handler) }.map(|h| h.to_com_ptr());
        kResultOk
    }
    unsafe fn createView(&self, name: FIDString) -> *mut IPlugView {
        if name.is_null() || unsafe { CStr::from_ptr(name) }.to_bytes() != b"editor" {
            return std::ptr::null_mut();
        }
        let view = ComWrapper::new(GainView {
            handler: self.handler.lock().unwrap().clone(),
            frame: Mutex::new(None),
            run_loop: Mutex::new(None),
            fires: AtomicU32::new(0),
            attached: AtomicBool::new(false),
        });
        view.to_com_ptr::<IPlugView>().unwrap().into_raw()
    }
}

/// The gain's editor: it draws nothing, and it is the host's run loop that
/// is under test — a timer registered on attach, a resize asked of the
/// frame on the first fire, an edit performed on the third.
struct GainView {
    handler: Option<ComPtr<IComponentHandler>>,
    frame: Mutex<Option<ComPtr<IPlugFrame>>>,
    run_loop: Mutex<Option<ComPtr<IRunLoop>>>,
    fires: AtomicU32,
    attached: AtomicBool,
}

impl Class for GainView {
    type Interfaces = (IPlugView, ITimerHandler);
}

impl IPlugViewTrait for GainView {
    unsafe fn isPlatformTypeSupported(&self, r#type: FIDString) -> tresult {
        let wanted: &[u8] = if cfg!(target_os = "windows") {
            b"HWND"
        } else if cfg!(target_os = "macos") {
            b"NSView"
        } else {
            b"X11EmbedWindowID"
        };
        if !r#type.is_null() && unsafe { CStr::from_ptr(r#type) }.to_bytes() == wanted {
            kResultTrue
        } else {
            kResultFalse
        }
    }
    unsafe fn attached(&self, _parent: *mut c_void, _type: FIDString) -> tresult {
        self.attached.store(true, Ordering::Relaxed);
        self.fires.store(0, Ordering::Relaxed);
        let frame = self.frame.lock().unwrap().clone();
        if let Some(frame) = frame
            && let Some(run_loop) = frame.cast::<IRunLoop>()
        {
            let me = unsafe { self.as_timer_handler() };
            unsafe { run_loop.registerTimer(me, 10) };
            *self.run_loop.lock().unwrap() = Some(run_loop);
        }
        kResultOk
    }
    unsafe fn removed(&self) -> tresult {
        if let Some(run_loop) = self.run_loop.lock().unwrap().take() {
            let me = unsafe { self.as_timer_handler() };
            unsafe { run_loop.unregisterTimer(me) };
        }
        self.attached.store(false, Ordering::Relaxed);
        kResultOk
    }
    unsafe fn onWheel(&self, _distance: f32) -> tresult {
        kResultFalse
    }
    unsafe fn onKeyDown(&self, _key: char16, _code: i16, _mods: i16) -> tresult {
        kResultFalse
    }
    unsafe fn onKeyUp(&self, _key: char16, _code: i16, _mods: i16) -> tresult {
        kResultFalse
    }
    unsafe fn getSize(&self, size: *mut ViewRect) -> tresult {
        let size = unsafe { &mut *size };
        size.left = 0;
        size.top = 0;
        size.right = VIEW_WIDTH as i32;
        size.bottom = VIEW_HEIGHT as i32;
        kResultOk
    }
    unsafe fn onSize(&self, _new_size: *mut ViewRect) -> tresult {
        kResultOk
    }
    unsafe fn onFocus(&self, _state: TBool) -> tresult {
        kResultOk
    }
    unsafe fn setFrame(&self, frame: *mut IPlugFrame) -> tresult {
        *self.frame.lock().unwrap() = unsafe { ComRef::from_raw(frame) }.map(|f| f.to_com_ptr());
        kResultOk
    }
    unsafe fn canResize(&self) -> tresult {
        kResultTrue
    }
    unsafe fn checkSizeConstraint(&self, _rect: *mut ViewRect) -> tresult {
        kResultTrue
    }
}

impl ITimerHandlerTrait for GainView {
    unsafe fn onTimer(&self) {
        let fires = self.fires.fetch_add(1, Ordering::Relaxed) + 1;
        if fires == 1
            && let Some(frame) = self.frame.lock().unwrap().as_ref()
        {
            let mut rect = ViewRect {
                left: 0,
                top: 0,
                right: (VIEW_WIDTH * 2) as i32,
                bottom: (VIEW_HEIGHT * 2) as i32,
            };
            unsafe { frame.resizeView(self.as_plug_view(), &mut rect) };
        }
        if fires == 3
            && let Some(handler) = &self.handler
        {
            unsafe {
                handler.beginEdit(0);
                handler.performEdit(0, 0.5);
                handler.endEdit(0);
            }
        }
    }
}

// The view hands *itself* to the run loop and the frame, which needs the
// COM pointer for the object `self` sits in. `ComWrapper` lays the header
// out in front of the data at a fixed offset; this recovers it, without
// touching the reference count — the receiver takes its own.
unsafe fn self_interface<C: Class, I: Interface>(data: &C) -> *mut I {
    use vst3::com_scrape_types::{InterfaceList, Wrapper};
    let header =
        unsafe { <ComWrapper<C> as Wrapper<C>>::header_from_data(data as *const C as *mut C) };
    let offset = C::Interfaces::query(&I::IID).expect("the class implements the interface");
    unsafe { (header as *mut u8).offset(offset) as *mut I }
}

impl GainView {
    unsafe fn as_timer_handler(&self) -> *mut ITimerHandler {
        unsafe { self_interface(self) }
    }
    unsafe fn as_plug_view(&self) -> *mut IPlugView {
        unsafe { self_interface(self) }
    }
}

// ---------------------------------------------------------------- the sine

struct Voice {
    on: bool,
    phase: f32,
    /// Per-note tuning, in semitones — a note expression.
    tuning: f32,
}

struct SineProcessor {
    voices: Mutex<Vec<Voice>>,
    level: AtomicU64,
    wheel: AtomicU64,
    bend: AtomicU64,
    pressure: AtomicU64,
    sample_rate: AtomicU64,
}

impl Class for SineProcessor {
    type Interfaces = (IComponent, IAudioProcessor, IProcessContextRequirements);
}

impl SineProcessor {
    fn new() -> Self {
        Self {
            voices: Mutex::new(
                (0..128)
                    .map(|_| Voice {
                        on: false,
                        phase: 0.0,
                        tuning: 0.0,
                    })
                    .collect(),
            ),
            level: AtomicU64::new(0.5f64.to_bits()),
            wheel: AtomicU64::new(1.0f64.to_bits()),
            bend: AtomicU64::new(0.5f64.to_bits()),
            pressure: AtomicU64::new(0.0f64.to_bits()),
            sample_rate: AtomicU64::new(48_000.0f64.to_bits()),
        }
    }
}

fn load(a: &AtomicU64) -> f32 {
    f64::from_bits(a.load(Ordering::Relaxed)) as f32
}

impl IPluginBaseTrait for SineProcessor {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }
    unsafe fn terminate(&self) -> tresult {
        kResultOk
    }
}

impl IComponentTrait for SineProcessor {
    unsafe fn getControllerClassId(&self, class_id: *mut TUID) -> tresult {
        unsafe { *class_id = SINE_CONTROLLER_CID };
        kResultOk
    }
    unsafe fn setIoMode(&self, _mode: IoMode) -> tresult {
        kResultOk
    }
    unsafe fn getBusCount(&self, media: MediaType, dir: BusDirection) -> i32 {
        match (media, dir) {
            (K_AUDIO, K_OUTPUT) => 2,
            (K_EVENT, K_INPUT) => 1,
            _ => 0,
        }
    }
    unsafe fn getBusInfo(
        &self,
        media: MediaType,
        dir: BusDirection,
        index: i32,
        bus: *mut BusInfo,
    ) -> tresult {
        match (media, dir, index) {
            (K_AUDIO, K_OUTPUT, 0) => unsafe { bus_info(bus, media, dir, 2, "Output", false) },
            (K_AUDIO, K_OUTPUT, 1) => unsafe { bus_info(bus, media, dir, 1, "Sub", true) },
            (K_EVENT, K_INPUT, 0) => unsafe { bus_info(bus, media, dir, 16, "MIDI In", false) },
            _ => kInvalidArgument,
        }
    }
    unsafe fn getRoutingInfo(&self, _in: *mut RoutingInfo, _out: *mut RoutingInfo) -> tresult {
        kNotImplemented
    }
    unsafe fn activateBus(
        &self,
        _media: MediaType,
        _dir: BusDirection,
        _index: i32,
        _state: TBool,
    ) -> tresult {
        kResultOk
    }
    unsafe fn setActive(&self, state: TBool) -> tresult {
        if state == 0 {
            for voice in self.voices.lock().unwrap().iter_mut() {
                voice.on = false;
            }
        }
        kResultOk
    }
    unsafe fn setState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }
    unsafe fn getState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }
}

impl IAudioProcessorTrait for SineProcessor {
    unsafe fn setBusArrangements(
        &self,
        _i: *mut SpeakerArrangement,
        _ni: i32,
        _o: *mut SpeakerArrangement,
        _no: i32,
    ) -> tresult {
        kResultOk
    }
    unsafe fn getBusArrangement(
        &self,
        _dir: BusDirection,
        index: i32,
        arr: *mut SpeakerArrangement,
    ) -> tresult {
        unsafe {
            *arr = if index == 1 {
                SpeakerArr::kMono
            } else {
                SpeakerArr::kStereo
            }
        };
        kResultOk
    }
    unsafe fn canProcessSampleSize(&self, size: i32) -> tresult {
        if size == SymbolicSampleSizes_::kSample32 as i32 {
            kResultOk
        } else {
            kResultFalse
        }
    }
    unsafe fn getLatencySamples(&self) -> u32 {
        0
    }
    unsafe fn setupProcessing(&self, setup: *mut ProcessSetup) -> tresult {
        self.sample_rate
            .store(unsafe { (*setup).sampleRate }.to_bits(), Ordering::Relaxed);
        kResultOk
    }
    unsafe fn setProcessing(&self, _state: TBool) -> tresult {
        kResultOk
    }
    unsafe fn process(&self, data: *mut ProcessData) -> tresult {
        let data = unsafe { &*data };
        unsafe {
            each_change(data, |id, value| match id {
                LEVEL => self.level.store(value.to_bits(), Ordering::Relaxed),
                WHEEL => self.wheel.store(value.to_bits(), Ordering::Relaxed),
                BEND => self.bend.store(value.to_bits(), Ordering::Relaxed),
                PRESSURE => self.pressure.store(value.to_bits(), Ordering::Relaxed),
                _ => {}
            })
        };
        if data.numOutputs < 2 {
            return kResultOk;
        }
        let n = data.numSamples as usize;
        let sample_rate = load(&self.sample_rate);
        let amplitude =
            load(&self.level) * load(&self.wheel) * (1.0 - load(&self.pressure).clamp(0.0, 1.0));
        let bend = (load(&self.bend) - 0.5) * 2.0 * BEND_RANGE_SEMITONES;
        let mut voices = self.voices.lock().unwrap();
        // Events at their sample offsets, in order.
        let mut events: Vec<Event> = Vec::new();
        if let Some(list) = unsafe { ComRef::from_raw(data.inputEvents) } {
            for index in 0..unsafe { list.getEventCount() } {
                let mut event: Event = unsafe { std::mem::zeroed() };
                if unsafe { list.getEvent(index, &mut event) } == kResultOk {
                    events.push(event);
                }
            }
        }
        let left = unsafe { channel(data.outputs, 0, 0, n) };
        let right = unsafe { channel(data.outputs, 0, 1, n) };
        let mut next_event = 0;
        for i in 0..n {
            while next_event < events.len() && events[next_event].sampleOffset as usize <= i {
                let event = &events[next_event];
                next_event += 1;
                match event.r#type as Event_::EventTypes {
                    Event_::EventTypes_::kNoteOnEvent => {
                        let on = unsafe { event.__field0.noteOn };
                        if let Some(voice) = voices.get_mut(on.pitch as usize) {
                            voice.on = true;
                            voice.phase = 0.0;
                            voice.tuning = 0.0;
                        }
                    }
                    Event_::EventTypes_::kNoteOffEvent => {
                        let off = unsafe { event.__field0.noteOff };
                        if let Some(voice) = voices.get_mut(off.pitch as usize) {
                            voice.on = false;
                        }
                    }
                    Event_::EventTypes_::kNoteExpressionValueEvent => {
                        let expression = unsafe { event.__field0.noteExpressionValue };
                        if expression.typeId == NoteExpressionTypeIDs_::kTuningTypeID as u32 {
                            // The SDK's convention: 0.5 is no tuning, the
                            // ends are ±120 semitones.
                            let semitones = (expression.value - 0.5) as f32 * 240.0;
                            if let Some(voice) = voices.get_mut(expression.noteId as usize) {
                                voice.tuning = semitones;
                            }
                        }
                    }
                    _ => {}
                }
            }
            let mut sample = 0.0f32;
            for (key, voice) in voices.iter_mut().enumerate() {
                if !voice.on {
                    continue;
                }
                let hz = 440.0 * 2.0f32.powf((key as f32 - 69.0 + bend + voice.tuning) / 12.0);
                sample += (voice.phase * std::f32::consts::TAU).sin() * amplitude;
                voice.phase = (voice.phase + hz / sample_rate).fract();
            }
            left[i] = sample;
            right[i] = sample;
        }
        let sub = unsafe { channel(data.outputs, 1, 0, n) };
        sub.fill(0.0);
        kResultOk
    }
    unsafe fn getTailSamples(&self) -> u32 {
        0
    }
}

impl IProcessContextRequirementsTrait for SineProcessor {
    unsafe fn getProcessContextRequirements(&self) -> u32 {
        0
    }
}

struct SineController {
    values: Mutex<[(ParamID, f64); 4]>,
}

impl Class for SineController {
    type Interfaces = (IEditController, IMidiMapping);
}

impl IPluginBaseTrait for SineController {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }
    unsafe fn terminate(&self) -> tresult {
        kResultOk
    }
}

impl IMidiMappingTrait for SineController {
    unsafe fn getMidiControllerAssignment(
        &self,
        bus: i32,
        _channel: i16,
        controller: CtrlNumber,
        id: *mut ParamID,
    ) -> tresult {
        if bus != 0 {
            return kResultFalse;
        }
        let mapped = match controller as ControllerNumbers {
            ControllerNumbers_::kCtrlModWheel => WHEEL,
            ControllerNumbers_::kPitchBend => BEND,
            ControllerNumbers_::kAfterTouch => PRESSURE,
            _ => return kResultFalse,
        };
        unsafe { *id = mapped };
        kResultTrue
    }
}

impl IEditControllerTrait for SineController {
    unsafe fn setComponentState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }
    unsafe fn setState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }
    unsafe fn getState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }
    unsafe fn getParameterCount(&self) -> i32 {
        4
    }
    unsafe fn getParameterInfo(&self, index: i32, info: *mut ParameterInfo) -> tresult {
        let info = unsafe { &mut *info };
        let (id, title, default) = match index {
            0 => (LEVEL, "Level", 0.5),
            1 => (WHEEL, "Wheel", 1.0),
            2 => (BEND, "Bend", 0.5),
            3 => (PRESSURE, "Pressure", 0.0),
            _ => return kInvalidArgument,
        };
        info.id = id;
        copy_wstring(title, &mut info.title);
        copy_wstring(title, &mut info.shortTitle);
        copy_wstring("", &mut info.units);
        info.stepCount = 0;
        info.defaultNormalizedValue = default;
        info.unitId = 0;
        info.flags = ParameterInfo_::ParameterFlags_::kCanAutomate as i32;
        kResultOk
    }
    unsafe fn getParamStringByValue(
        &self,
        _id: ParamID,
        value: f64,
        string: *mut String128,
    ) -> tresult {
        copy_wstring(&format!("{value:.3}"), unsafe { &mut *string });
        kResultOk
    }
    unsafe fn getParamValueByString(
        &self,
        _id: ParamID,
        _string: *mut TChar,
        _value: *mut f64,
    ) -> tresult {
        kNotImplemented
    }
    unsafe fn normalizedParamToPlain(&self, _id: ParamID, value: f64) -> f64 {
        value
    }
    unsafe fn plainParamToNormalized(&self, _id: ParamID, plain: f64) -> f64 {
        plain
    }
    unsafe fn getParamNormalized(&self, id: ParamID) -> f64 {
        self.values
            .lock()
            .unwrap()
            .iter()
            .find(|(i, _)| *i == id)
            .map_or(0.0, |(_, v)| *v)
    }
    unsafe fn setParamNormalized(&self, id: ParamID, value: f64) -> tresult {
        match self
            .values
            .lock()
            .unwrap()
            .iter_mut()
            .find(|(i, _)| *i == id)
        {
            Some(slot) => {
                slot.1 = value;
                kResultOk
            }
            None => kInvalidArgument,
        }
    }
    unsafe fn setComponentHandler(&self, _handler: *mut IComponentHandler) -> tresult {
        kResultOk
    }
    unsafe fn createView(&self, _name: FIDString) -> *mut IPlugView {
        std::ptr::null_mut()
    }
}

// ------------------------------------------------------------ the combined

/// One object, both halves: mono in, mono out, one gain.
struct Combined {
    gain: AtomicU64,
    edit_gain: Cell<f64>,
}

// SAFETY: the `Cell` is the controller half, which is main-thread only.
unsafe impl Sync for Combined {}
unsafe impl Send for Combined {}

impl Class for Combined {
    type Interfaces = (
        IComponent,
        IAudioProcessor,
        IEditController,
        IProcessContextRequirements,
    );
}

impl IPluginBaseTrait for Combined {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }
    unsafe fn terminate(&self) -> tresult {
        kResultOk
    }
}

impl IComponentTrait for Combined {
    unsafe fn getControllerClassId(&self, _class_id: *mut TUID) -> tresult {
        kResultFalse
    }
    unsafe fn setIoMode(&self, _mode: IoMode) -> tresult {
        kResultOk
    }
    unsafe fn getBusCount(&self, media: MediaType, _dir: BusDirection) -> i32 {
        if media == K_AUDIO { 1 } else { 0 }
    }
    unsafe fn getBusInfo(
        &self,
        media: MediaType,
        dir: BusDirection,
        index: i32,
        bus: *mut BusInfo,
    ) -> tresult {
        if media != K_AUDIO || index != 0 {
            return kInvalidArgument;
        }
        unsafe { bus_info(bus, media, dir, 1, "Mono", false) }
    }
    unsafe fn getRoutingInfo(&self, _in: *mut RoutingInfo, _out: *mut RoutingInfo) -> tresult {
        kNotImplemented
    }
    unsafe fn activateBus(
        &self,
        _media: MediaType,
        _dir: BusDirection,
        _index: i32,
        _state: TBool,
    ) -> tresult {
        kResultOk
    }
    unsafe fn setActive(&self, _state: TBool) -> tresult {
        kResultOk
    }
    unsafe fn setState(&self, state: *mut IBStream) -> tresult {
        let bytes = unsafe { read_all(state) };
        if bytes.len() == 8 {
            self.gain.store(
                f64::from_le_bytes(bytes.try_into().unwrap()).to_bits(),
                Ordering::Relaxed,
            );
        }
        kResultOk
    }
    unsafe fn getState(&self, state: *mut IBStream) -> tresult {
        unsafe {
            write_all(
                state,
                &f64::from_bits(self.gain.load(Ordering::Relaxed)).to_le_bytes(),
            )
        }
    }
}

impl IAudioProcessorTrait for Combined {
    unsafe fn setBusArrangements(
        &self,
        _i: *mut SpeakerArrangement,
        _ni: i32,
        _o: *mut SpeakerArrangement,
        _no: i32,
    ) -> tresult {
        kResultOk
    }
    unsafe fn getBusArrangement(
        &self,
        _dir: BusDirection,
        _index: i32,
        arr: *mut SpeakerArrangement,
    ) -> tresult {
        unsafe { *arr = SpeakerArr::kMono };
        kResultOk
    }
    unsafe fn canProcessSampleSize(&self, size: i32) -> tresult {
        if size == SymbolicSampleSizes_::kSample32 as i32 {
            kResultOk
        } else {
            kResultFalse
        }
    }
    unsafe fn getLatencySamples(&self) -> u32 {
        0
    }
    unsafe fn setupProcessing(&self, _setup: *mut ProcessSetup) -> tresult {
        kResultOk
    }
    unsafe fn setProcessing(&self, _state: TBool) -> tresult {
        kResultOk
    }
    unsafe fn process(&self, data: *mut ProcessData) -> tresult {
        let data = unsafe { &*data };
        unsafe {
            each_change(data, |id, value| {
                if id == 0 {
                    self.gain.store(value.to_bits(), Ordering::Relaxed);
                }
            })
        };
        if data.numInputs < 1 || data.numOutputs < 1 {
            return kResultOk;
        }
        let n = data.numSamples as usize;
        let gain = load(&self.gain);
        let input = unsafe { channel(data.inputs, 0, 0, n) };
        let output = unsafe { channel(data.outputs, 0, 0, n) };
        for i in 0..n {
            output[i] = input[i] * gain;
        }
        kResultOk
    }
    unsafe fn getTailSamples(&self) -> u32 {
        0
    }
}

impl IProcessContextRequirementsTrait for Combined {
    unsafe fn getProcessContextRequirements(&self) -> u32 {
        0
    }
}

impl IEditControllerTrait for Combined {
    unsafe fn setComponentState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }
    unsafe fn setState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }
    unsafe fn getState(&self, _state: *mut IBStream) -> tresult {
        kResultOk
    }
    unsafe fn getParameterCount(&self) -> i32 {
        1
    }
    unsafe fn getParameterInfo(&self, index: i32, info: *mut ParameterInfo) -> tresult {
        if index != 0 {
            return kInvalidArgument;
        }
        let info = unsafe { &mut *info };
        info.id = 0;
        copy_wstring("Gain", &mut info.title);
        copy_wstring("Gain", &mut info.shortTitle);
        copy_wstring("", &mut info.units);
        info.stepCount = 0;
        info.defaultNormalizedValue = 1.0;
        info.unitId = 0;
        info.flags = ParameterInfo_::ParameterFlags_::kCanAutomate as i32;
        kResultOk
    }
    unsafe fn getParamStringByValue(
        &self,
        _id: ParamID,
        value: f64,
        string: *mut String128,
    ) -> tresult {
        copy_wstring(&format!("{value:.2}"), unsafe { &mut *string });
        kResultOk
    }
    unsafe fn getParamValueByString(
        &self,
        _id: ParamID,
        _string: *mut TChar,
        _value: *mut f64,
    ) -> tresult {
        kNotImplemented
    }
    unsafe fn normalizedParamToPlain(&self, _id: ParamID, value: f64) -> f64 {
        value
    }
    unsafe fn plainParamToNormalized(&self, _id: ParamID, plain: f64) -> f64 {
        plain
    }
    unsafe fn getParamNormalized(&self, _id: ParamID) -> f64 {
        self.edit_gain.get()
    }
    unsafe fn setParamNormalized(&self, id: ParamID, value: f64) -> tresult {
        if id != 0 {
            return kInvalidArgument;
        }
        self.edit_gain.set(value);
        kResultOk
    }
    unsafe fn setComponentHandler(&self, _handler: *mut IComponentHandler) -> tresult {
        kResultOk
    }
    unsafe fn createView(&self, _name: FIDString) -> *mut IPlugView {
        std::ptr::null_mut()
    }
}

// ---------------------------------------------------------------- factory

struct Factory;

impl Class for Factory {
    type Interfaces = (IPluginFactory, IPluginFactory2);
}

struct ClassRow {
    cid: TUID,
    category: &'static str,
    name: &'static str,
    sub: &'static str,
}

const CLASSES: [ClassRow; 5] = [
    ClassRow {
        cid: GAIN_CID,
        category: "Audio Module Class",
        name: GAIN_NAME,
        sub: "Fx|Tools",
    },
    ClassRow {
        cid: GAIN_CONTROLLER_CID,
        category: "Component Controller Class",
        name: GAIN_NAME,
        sub: "",
    },
    ClassRow {
        cid: SINE_CID,
        category: "Audio Module Class",
        name: SINE_NAME,
        sub: "Instrument|Synth",
    },
    ClassRow {
        cid: SINE_CONTROLLER_CID,
        category: "Component Controller Class",
        name: SINE_NAME,
        sub: "",
    },
    ClassRow {
        cid: COMBINED_CID,
        category: "Audio Module Class",
        name: COMBINED_NAME,
        sub: "Fx",
    },
];

impl IPluginFactoryTrait for Factory {
    unsafe fn getFactoryInfo(&self, info: *mut PFactoryInfo) -> tresult {
        let info = unsafe { &mut *info };
        copy_cstring(VENDOR, &mut info.vendor);
        copy_cstring("https://fopull.com", &mut info.url);
        copy_cstring("", &mut info.email);
        info.flags = PFactoryInfo_::FactoryFlags_::kUnicode as int32;
        kResultOk
    }
    unsafe fn countClasses(&self) -> i32 {
        CLASSES.len() as i32
    }
    unsafe fn getClassInfo(&self, index: i32, info: *mut PClassInfo) -> tresult {
        let Some(row) = CLASSES.get(index as usize) else {
            return kInvalidArgument;
        };
        let info = unsafe { &mut *info };
        info.cid = row.cid;
        info.cardinality = PClassInfo_::ClassCardinality_::kManyInstances as int32;
        copy_cstring(row.category, &mut info.category);
        copy_cstring(row.name, &mut info.name);
        kResultOk
    }
    unsafe fn createInstance(
        &self,
        cid: FIDString,
        iid: FIDString,
        obj: *mut *mut c_void,
    ) -> tresult {
        let cid = unsafe { *(cid as *const TUID) };
        let instance: Option<ComPtr<FUnknown>> = match cid {
            GAIN_CID => ComWrapper::new(GainProcessor::new()).to_com_ptr(),
            GAIN_CONTROLLER_CID => ComWrapper::new(GainController::new()).to_com_ptr(),
            SINE_CID => ComWrapper::new(SineProcessor::new()).to_com_ptr(),
            SINE_CONTROLLER_CID => ComWrapper::new(SineController {
                values: Mutex::new([(LEVEL, 0.5), (WHEEL, 1.0), (BEND, 0.5), (PRESSURE, 0.0)]),
            })
            .to_com_ptr(),
            COMBINED_CID => ComWrapper::new(Combined {
                gain: AtomicU64::new(1.0f64.to_bits()),
                edit_gain: Cell::new(1.0),
            })
            .to_com_ptr(),
            _ => None,
        };
        match instance {
            Some(instance) => {
                let ptr = instance.as_ptr();
                unsafe { ((*(*ptr).vtbl).queryInterface)(ptr, iid as *mut TUID, obj) }
            }
            None => kInvalidArgument,
        }
    }
}

impl IPluginFactory2Trait for Factory {
    unsafe fn getClassInfo2(&self, index: i32, info: *mut PClassInfo2) -> tresult {
        let Some(row) = CLASSES.get(index as usize) else {
            return kInvalidArgument;
        };
        let info = unsafe { &mut *info };
        info.cid = row.cid;
        info.cardinality = PClassInfo_::ClassCardinality_::kManyInstances as int32;
        copy_cstring(row.category, &mut info.category);
        copy_cstring(row.name, &mut info.name);
        info.classFlags = 0;
        copy_cstring(row.sub, &mut info.subCategories);
        copy_cstring(VENDOR, &mut info.vendor);
        copy_cstring(VERSION, &mut info.version);
        copy_cstring("VST 3.7.9", &mut info.sdkVersion);
        kResultOk
    }
}

#[cfg(target_os = "windows")]
#[unsafe(no_mangle)]
extern "system" fn InitDll() -> bool {
    true
}

#[cfg(target_os = "windows")]
#[unsafe(no_mangle)]
extern "system" fn ExitDll() -> bool {
    true
}

#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
extern "system" fn bundleEntry(_bundle_ref: *mut c_void) -> bool {
    true
}

#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
extern "system" fn bundleExit() -> bool {
    true
}

#[cfg(target_os = "linux")]
#[unsafe(no_mangle)]
extern "system" fn ModuleEntry(_library_handle: *mut c_void) -> bool {
    true
}

#[cfg(target_os = "linux")]
#[unsafe(no_mangle)]
extern "system" fn ModuleExit() -> bool {
    true
}

#[unsafe(no_mangle)]
extern "system" fn GetPluginFactory() -> *mut IPluginFactory {
    ComWrapper::new(Factory)
        .to_com_ptr::<IPluginFactory>()
        .unwrap()
        .into_raw()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spelled(cid: TUID) -> String {
        // The host's spelling: the four words of the uid, eight hex digits
        // each. On Windows the SDK lays the bytes out COM-style, which the
        // host un-swaps; here the words are recovered the same way.
        let b: [u8; 16] = cid.map(|c| c as u8);
        let words = if cfg!(windows) {
            [
                u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                u32::from_be_bytes([b[5], b[4], b[7], b[6]]),
                u32::from_be_bytes([b[8], b[9], b[10], b[11]]),
                u32::from_be_bytes([b[12], b[13], b[14], b[15]]),
            ]
        } else {
            [
                u32::from_be_bytes([b[0], b[1], b[2], b[3]]),
                u32::from_be_bytes([b[4], b[5], b[6], b[7]]),
                u32::from_be_bytes([b[8], b[9], b[10], b[11]]),
                u32::from_be_bytes([b[12], b[13], b[14], b[15]]),
            ]
        };
        words.iter().map(|w| format!("{w:08X}")).collect()
    }

    #[test]
    fn the_ids_are_spelled_the_way_the_host_spells_them() {
        assert_eq!(spelled(GAIN_CID), GAIN_ID);
        assert_eq!(spelled(GAIN_CONTROLLER_CID), GAIN_CONTROLLER_ID);
        assert_eq!(spelled(SINE_CID), SINE_ID);
        assert_eq!(spelled(SINE_CONTROLLER_CID), SINE_CONTROLLER_ID);
        assert_eq!(spelled(COMBINED_CID), COMBINED_ID);
    }
}

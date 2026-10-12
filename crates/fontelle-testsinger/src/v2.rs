//! The test singer's ABI 2 self: capabilities, the whole phrase, and a
//! window of its own.
//!
//! Loaded from a file whose name has `abi2` in it, the library is a
//! version-2 engine called [`ENGINE2_NAME`](crate::ENGINE2_NAME): it honours
//! pitch bend, dynamics and breathiness (not gender, so a host has a lane
//! to grey out), vibrato and glides (through `fontelle_sing::note_pitch`),
//! hand-placed phonemes with their timings, a lane of its own
//! ([`GROWL_LANE`](crate::GROWL_LANE)), a setting
//! ([`BRIGHTNESS_SETTING`](crate::BRIGHTNESS_SETTING)) and a per-note option
//! ([`COLOUR_OPTION`](crate::COLOUR_OPTION)). Its window paints one colour,
//! and edits the part when a note's lyric is one of the test levers.

use std::collections::{BTreeMap, HashSet};
use std::ffi::{CStr, CString, c_char, c_void};
use std::sync::atomic::{AtomicU32, Ordering};

use fontelle_sing::{
    Capabilities, Curve, GlideShape, Lane, LaneInfo, NoteOptionInfo, PartEdit, PartView, Phrase,
    SettingInfo, Transport,
};
use fontelle_singer_abi::{
    EngineInstance, HostCallbacks, PhraseResult, QUALITY_FINAL, STATUS_CANCELLED, STATUS_ERROR,
    STATUS_OK, SingerEngineV2,
};

use crate::{
    BAD_LYRIC, BRIGHTNESS_SETTING, COLOUR_OPTION, EDITOR_SIZE, FAIL_LYRIC, GROWL_LANE, LEAD, Noise,
    PITCH_STEP, PLAY_LYRIC, POKE_LYRIC, RELEASE, SAMPLE_RATE, SLOW_LYRIC, SLOW_SECONDS, TAKE_LYRIC,
    VOICES, instance, phonemes, vowel_weight,
};

/// Whether this copy of the library is the version-2 engine: its file name
/// says so. One build, two engines, so a host's tests load both.
pub(crate) fn mode_is_v2() -> bool {
    library_path().is_some_and(|path| {
        std::path::Path::new(&path)
            .file_name()
            .is_some_and(|name| name.to_string_lossy().contains("abi2"))
    })
}

static ANCHOR: u8 = 0;

#[cfg(unix)]
fn library_path() -> Option<String> {
    // SAFETY: dladdr fills `info` for an address inside a loaded object; a
    // private static's address is never interposed by another object.
    unsafe {
        let mut info: libc::Dl_info = std::mem::zeroed();
        if libc::dladdr(&ANCHOR as *const u8 as *const c_void, &mut info) == 0
            || info.dli_fname.is_null()
        {
            return None;
        }
        Some(
            CStr::from_ptr(info.dli_fname)
                .to_string_lossy()
                .into_owned(),
        )
    }
}

#[cfg(windows)]
fn library_path() -> Option<String> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleExW(flags: u32, name: *const u16, module: *mut *mut c_void) -> i32;
        fn GetModuleFileNameW(module: *mut c_void, name: *mut u16, size: u32) -> u32;
    }
    const FROM_ADDRESS: u32 = 0x4;
    const UNCHANGED_REFCOUNT: u32 = 0x2;
    // SAFETY: both calls only read the module table and fill our buffer.
    unsafe {
        let mut module = std::ptr::null_mut();
        if GetModuleHandleExW(
            FROM_ADDRESS | UNCHANGED_REFCOUNT,
            &ANCHOR as *const u8 as *const u16,
            &mut module,
        ) == 0
        {
            return None;
        }
        let mut buf = vec![0u16; 2048];
        let n = GetModuleFileNameW(module, buf.as_mut_ptr(), buf.len() as u32) as usize;
        (n > 0).then(|| String::from_utf16_lossy(&buf[..n]))
    }
}

#[cfg(not(any(unix, windows)))]
fn library_path() -> Option<String> {
    None
}

/// What it answers to, for every voice it has.
pub fn capabilities() -> Capabilities {
    Capabilities {
        lanes: vec![Lane::PitchBend, Lane::Dynamics, Lane::Breathiness],
        vibrato: true,
        portamento: true,
        velocity: true,
        phoneme_timing: true,
        phonemes: [
            "a", "i", "u", "e", "o", "k", "s", "t", "n", "h", "m", "y", "r", "w", "l",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
        extra_lanes: vec![LaneInfo {
            id: GROWL_LANE.into(),
            name: "Growl".into(),
            min: 0.0,
            max: 1.0,
            default: 0.0,
            unit: "%".into(),
        }],
        settings: vec![SettingInfo {
            id: BRIGHTNESS_SETTING.into(),
            name: "Brightness".into(),
            min: -1.0,
            max: 1.0,
            default: 0.0,
            unit: String::new(),
            choices: Vec::new(),
        }],
        note_options: vec![NoteOptionInfo {
            id: COLOUR_OPTION.into(),
            name: "Voice colour".into(),
            choices: vec![
                ("plain".into(), "Plain".into()),
                ("soft".into(), "Soft".into()),
            ],
        }],
    }
}

fn text(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

unsafe extern "C" fn v2_capabilities(
    _p: *mut EngineInstance,
    singer: *const c_char,
) -> *mut c_char {
    let id = text(singer);
    if !VOICES.iter().any(|v| v.id.to_str() == Ok(id.as_str())) {
        return std::ptr::null_mut();
    }
    let json = serde_json::to_string(&capabilities()).unwrap_or_default();
    CString::new(json).map_or(std::ptr::null_mut(), CString::into_raw)
}

unsafe extern "C" fn v2_free_string(_p: *mut EngineInstance, s: *mut c_char) {
    if !s.is_null() {
        drop(unsafe { CString::from_raw(s) });
    }
}

enum Failed {
    Cancelled,
    Error(String),
}

fn cancelled(flag: *const AtomicU32) -> bool {
    !flag.is_null() && unsafe { &*flag }.load(Ordering::Relaxed) != 0
}

struct Sung {
    samples: Vec<f32>,
    start: f64,
    pitch: Vec<f32>,
    phonemes: String,
}

fn hz(midi: f64) -> f64 {
    440.0 * 2f64.powf((midi - 69.0) / 12.0)
}

fn sing(
    phrase: &Phrase,
    quality: u32,
    seed: u64,
    cancel: *const AtomicU32,
) -> Result<Sung, Failed> {
    if cancelled(cancel) {
        return Err(Failed::Cancelled);
    }
    let Some(voice) = VOICES
        .iter()
        .find(|v| v.id.to_str() == Ok(phrase.voice.as_str()))
    else {
        return Err(Failed::Error(format!(
            "no singer called {:?}",
            phrase.voice
        )));
    };
    let languages = voice.languages.to_str().unwrap_or("");
    if !languages.split(',').any(|l| l == phrase.language) {
        return Err(Failed::Error(format!(
            "{} does not sing {:?}",
            voice.name.to_string_lossy(),
            phrase.language
        )));
    }
    for note in &phrase.notes {
        if note.lyric == FAIL_LYRIC {
            return Err(Failed::Error("asked to fail".to_string()));
        }
        if note.lyric == SLOW_LYRIC {
            let until =
                std::time::Instant::now() + std::time::Duration::from_secs_f64(SLOW_SECONDS);
            while std::time::Instant::now() < until {
                if cancelled(cancel) {
                    return Err(Failed::Cancelled);
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
    }
    let notes = &phrase.notes;
    let rate = f64::from(SAMPLE_RATE);
    let start = -LEAD;
    let end = notes.iter().map(|n| n.end()).fold(0.0, f64::max) + RELEASE;
    let line = fontelle_sing::note_pitch(notes, start, end);
    let bend = phrase.curves.get(&Lane::PitchBend);
    let dynamics = phrase.curves.get(&Lane::Dynamics);
    let breath = phrase.curves.get(&Lane::Breathiness);
    let growl = phrase.extra_curves.get(GROWL_LANE);
    let brightness = phrase
        .settings
        .get(BRIGHTNESS_SETTING)
        .copied()
        .unwrap_or(0.0)
        .clamp(-1.0, 1.0);
    let pitch_at = |t: f64| {
        let cents = bend.map_or(0.0, |c| c.at(t, 0.0));
        f64::from(line.at(t, 60.0)) + f64::from(cents) / 100.0
    };
    let frames = ((end - start) * rate).ceil().max(0.0) as usize;
    let mut samples = vec![0.0f32; frames];
    let harmonics = if quality == QUALITY_FINAL { 8 } else { 3 };
    let mut noise = Noise(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut phase = 0.0f64;
    let mut gain = 0.0f32;
    for (i, out) in samples.iter_mut().enumerate() {
        if i % 512 == 0 && cancelled(cancel) {
            return Err(Failed::Cancelled);
        }
        let t = start + i as f64 / rate;
        let sounding = notes.iter().rev().find(|n| t >= n.start && t < n.end());
        let soft = sounding
            .is_some_and(|n| n.options.get(COLOUR_OPTION).map(String::as_str) == Some("soft"));
        let level = 10f32.powf(dynamics.map_or(0.0, |c| c.at(t, 0.0)) / 20.0);
        let target = if sounding.is_some() {
            0.25 * level * if soft { 0.5 } else { 1.0 }
        } else {
            0.0
        };
        let rate_of = if target > gain { 0.015 } else { 0.04 };
        gain += (target - gain) / (rate_of * rate) as f32;
        let f = hz(pitch_at(t));
        phase += f / rate;
        phase -= phase.floor();
        let mut v = 0.0f32;
        for k in 1..=harmonics {
            let tilt = 1.0 + brightness * 0.3 * (k as f32 - 1.0);
            v += vowel_weight("a", k)
                * tilt.max(0.0)
                * (std::f64::consts::TAU * phase * k as f64).sin() as f32;
        }
        *out += v * gain;
        let air = breath.map_or(0.0, |c| c.at(t, 0.0)).max(0.0) * 0.05
            + growl.map_or(0.0, |c| c.at(t, 0.0)).max(0.0) * 0.1;
        *out += noise.next() * (0.002 + air * gain * 4.0);
    }
    let steps = ((end - start) / PITCH_STEP).ceil() as usize;
    let pitch = (0..steps)
        .map(|i| {
            let t = start + i as f64 * PITCH_STEP;
            if notes.iter().any(|n| t >= n.start && t < n.end()) {
                hz(pitch_at(t)) as f32
            } else {
                0.0
            }
        })
        .collect();
    // What each note was sung as: the hand-placed phonemes, timings and all,
    // else its reading of the lyric.
    let mut report = Vec::new();
    for (i, note) in notes.iter().enumerate() {
        let (symbols, offsets): (Vec<String>, Vec<Option<f64>>) = match &note.phonemes {
            Some(edits) => edits.iter().map(|e| (e.symbol.clone(), e.offset)).unzip(),
            None => match phonemes(&note.lyric) {
                Some(p) => {
                    let n = p.len();
                    (p, vec![None; n])
                }
                None => continue,
            },
        };
        report.push(serde_json::json!({
            "note": i,
            "phonemes": symbols,
            "offsets": offsets,
        }));
    }
    Ok(Sung {
        samples,
        start,
        pitch,
        phonemes: serde_json::Value::Array(report).to_string(),
    })
}

unsafe extern "C" fn v2_render_phrase(
    p: *mut EngineInstance,
    json: *const c_char,
    quality: u32,
    seed: u64,
    cancel: *const AtomicU32,
    result: *mut PhraseResult,
) -> i32 {
    let me = instance(p);
    me.last_error = None;
    if result.is_null() {
        me.last_error = CString::new("no result").ok();
        return STATUS_ERROR;
    }
    let phrase: Phrase = match serde_json::from_str(&text(json)) {
        Ok(p) => p,
        Err(e) => {
            me.last_error = CString::new(format!("not a phrase: {e}")).ok();
            return STATUS_ERROR;
        }
    };
    match sing(&phrase, quality, seed, cancel) {
        Ok(sung) => {
            let frame_count = sung.samples.len() as u64;
            let pitch_count = sung.pitch.len() as u64;
            let samples = Box::into_raw(sung.samples.into_boxed_slice());
            let pitch = Box::into_raw(sung.pitch.into_boxed_slice());
            unsafe {
                *result = PhraseResult {
                    sample_rate: SAMPLE_RATE,
                    frame_count,
                    samples: samples as *mut f32,
                    start: sung.start,
                    pitch_count,
                    pitch: pitch as *mut f32,
                    pitch_step: PITCH_STEP,
                    phonemes: CString::new(sung.phonemes)
                        .map_or(std::ptr::null_mut(), CString::into_raw),
                };
            }
            STATUS_OK
        }
        Err(Failed::Cancelled) => STATUS_CANCELLED,
        Err(Failed::Error(why)) => {
            me.last_error = CString::new(why).ok();
            STATUS_ERROR
        }
    }
}

// --------------------------------------------------------------- window

/// An open window: the host's callbacks, whether the part changed since it
/// was last read, which levers have been pulled, and what it paints.
pub(crate) struct Editor {
    host: *const HostCallbacks,
    dirty: bool,
    done: HashSet<(u64, String)>,
    #[cfg(target_os = "linux")]
    _paint: Option<paint::Painted>,
}

unsafe extern "C" fn v2_has_editor(_p: *mut EngineInstance) -> bool {
    true
}

unsafe extern "C" fn v2_editor_open(
    p: *mut EngineInstance,
    parent: u64,
    host: *const HostCallbacks,
    width: *mut u32,
    height: *mut u32,
) -> bool {
    if host.is_null() {
        return false;
    }
    let me = instance(p);
    unsafe {
        if !width.is_null() {
            *width = EDITOR_SIZE.0;
        }
        if !height.is_null() {
            *height = EDITOR_SIZE.1;
        }
    }
    let _ = parent;
    me.editor = Some(Editor {
        host,
        dirty: true,
        done: HashSet::new(),
        #[cfg(target_os = "linux")]
        _paint: (parent != 0)
            .then(|| paint::Painted::new(parent as u32, EDITOR_SIZE))
            .flatten(),
    });
    true
}

unsafe extern "C" fn v2_editor_tick(p: *mut EngineInstance) {
    let me = instance(p);
    let Some(editor) = me.editor.as_mut() else {
        return;
    };
    if !std::mem::take(&mut editor.dirty) {
        return;
    }
    let host = unsafe { &*editor.host };
    let raw = unsafe { (host.get_part)(host.context) };
    if raw.is_null() {
        return;
    }
    let json = text(raw);
    unsafe { (host.free_string)(host.context, raw) };
    let Ok(view) = serde_json::from_str::<PartView>(&json) else {
        return;
    };
    let apply = |edits: &[PartEdit]| -> bool {
        let json = serde_json::to_string(edits).unwrap_or_default();
        let c = CString::new(json).unwrap_or_default();
        unsafe { (host.apply_edits)(host.context, c.as_ptr()) }
    };
    for note in &view.notes {
        let lever = note.note.lyric.as_str();
        let key = (note.id, lever.to_string());
        if ![POKE_LYRIC, BAD_LYRIC, PLAY_LYRIC, TAKE_LYRIC].contains(&lever)
            || editor.done.contains(&key)
        {
            continue;
        }
        // A take not sung yet is asked for again when the part next changes.
        if lever != TAKE_LYRIC {
            editor.done.insert(key.clone());
        }
        if lever == POKE_LYRIC {
            apply(&[
                PartEdit::Lyrics(vec![(note.id, "poked".into())]),
                PartEdit::Settings(BTreeMap::from([(BRIGHTNESS_SETTING.to_string(), 0.5)])),
                PartEdit::ExtraCurve {
                    id: GROWL_LANE.into(),
                    curve: Some(Curve {
                        start: note.note.start,
                        step: 0.01,
                        values: vec![0.2, 0.8],
                    }),
                },
                PartEdit::Portamento {
                    notes: vec![note.id],
                    portamento: Some(fontelle_sing::Portamento {
                        length: 0.1,
                        position: 0.5,
                        shape: GlideShape::Linear,
                    }),
                },
            ]);
        } else if lever == BAD_LYRIC {
            if !apply(&[PartEdit::Lyrics(vec![(u64::MAX, "x".into())])]) {
                apply(&[PartEdit::Lyrics(vec![(note.id, "refused".into())])]);
            }
        } else if lever == PLAY_LYRIC {
            let t = serde_json::to_string(&Transport::Play).unwrap_or_default();
            let c = CString::new(t).unwrap_or_default();
            unsafe { (host.transport)(host.context, c.as_ptr()) };
        } else if lever == TAKE_LYRIC {
            let mut take = PhraseResult::EMPTY;
            if unsafe { (host.take_at)(host.context, note.note.start + 0.01, &mut take) } {
                editor.done.insert(key);
                // Its length, and how many notes its phoneme report names.
                let sung = serde_json::from_str::<serde_json::Value>(&text(take.phonemes))
                    .ok()
                    .and_then(|v| v.as_array().map(Vec::len))
                    .unwrap_or(0);
                apply(&[PartEdit::Lyrics(vec![(
                    note.id,
                    format!("take {} {sung}", take.frame_count),
                )])]);
            }
        }
    }
}

unsafe extern "C" fn v2_editor_resized(p: *mut EngineInstance, _w: u32, _h: u32) {
    let _ = instance(p);
}

unsafe extern "C" fn v2_editor_part_changed(p: *mut EngineInstance) {
    if let Some(editor) = instance(p).editor.as_mut() {
        editor.dirty = true;
    }
}

unsafe extern "C" fn v2_editor_close(p: *mut EngineInstance) {
    instance(p).editor = None;
}

/// The colour it paints, so a screenshot can find it: a deep teal.
pub const EDITOR_COLOUR: u32 = 0x1f_6f_8f;

#[cfg(target_os = "linux")]
mod paint {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt as _, CreateWindowAux, WindowClass};
    use x11rb::rust_connection::RustConnection;

    /// A child of the host's window, filled with one colour by the X server
    /// itself (its background), so there is no drawing loop to run.
    pub struct Painted {
        conn: RustConnection,
        window: u32,
    }

    impl Painted {
        pub fn new(parent: u32, size: (u32, u32)) -> Option<Self> {
            let (conn, _) = RustConnection::connect(None).ok()?;
            let window = conn.generate_id().ok()?;
            conn.create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                window,
                parent,
                0,
                0,
                size.0 as u16,
                size.1 as u16,
                0,
                WindowClass::INPUT_OUTPUT,
                x11rb::COPY_FROM_PARENT,
                &CreateWindowAux::new().background_pixel(super::EDITOR_COLOUR),
            )
            .ok()?;
            conn.map_window(window).ok()?;
            conn.flush().ok()?;
            Some(Self { conn, window })
        }
    }

    impl Drop for Painted {
        fn drop(&mut self) {
            let _ = self.conn.destroy_window(self.window);
            let _ = self.conn.flush();
        }
    }
}

pub(crate) struct Table(pub SingerEngineV2);
// SAFETY: function pointers only.
unsafe impl Sync for Table {}

pub(crate) static V2: Table = Table(SingerEngineV2 {
    capabilities: v2_capabilities,
    render_phrase: v2_render_phrase,
    free_string: v2_free_string,
    has_editor: v2_has_editor,
    editor_open: v2_editor_open,
    editor_tick: v2_editor_tick,
    editor_resized: v2_editor_resized,
    editor_part_changed: v2_editor_part_changed,
    editor_close: v2_editor_close,
});

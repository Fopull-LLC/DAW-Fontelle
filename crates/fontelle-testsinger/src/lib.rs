//! A singer engine with no voice in it.
//!
//! **This is a test fixture, not a product.** It implements
//! [`fontelle_singer_abi`] by humming each note as a soft vowel at the
//! note's pitch, with a short breath of noise *before* every new syllable —
//! a consonant, so its audio starts ahead of the phrase's first note and
//! [`PhraseResult::start`] is negative the way a real engine's is. What it
//! proves is the vocal track's plumbing: that Fontelle finds an engine,
//! reads its singers, segments a part into phrases, renders them off the
//! audio thread, caches them and plays them in time. A real engine is built
//! in a repository of its own and never touches this tree.
//!
//! Its two singers are an ordinary one and one flagged
//! [`SINGER_RESEARCH_ONLY`], so the picker's badge has something to show.
//! Two lyrics are levers for tests: [`SLOW_LYRIC`] makes a render take a
//! while (checking the cancel flag as it waits) and [`FAIL_LYRIC`] makes it
//! fail with a message.

use std::ffi::{CStr, CString, c_char};
use std::sync::atomic::{AtomicU32, Ordering};

use fontelle_singer_abi::{
    ABI_VERSION, EngineInstance, FontelleSingerEngine, LYRIC_SLUR, PhraseNote, PhraseRequest,
    PhraseResult, QUALITY_FINAL, SINGER_RESEARCH_ONLY, STATUS_CANCELLED, STATUS_ERROR, STATUS_OK,
    SingerInfo,
};

pub const ENGINE_NAME: &str = "Fontelle Test Singer";
pub const ENGINE_VERSION: &str = "1.0.0";
pub const ALTO_ID: &str = "test.alto";
pub const ALTO_NAME: &str = "Test Alto";
pub const RESEARCH_ID: &str = "test.research";
pub const RESEARCH_NAME: &str = "Research Voice";
/// The rate it sings at: not a device's, so the host has to resample.
pub const SAMPLE_RATE: u32 = 24_000;
/// How long the consonant before a syllable lasts, in seconds.
pub const LEAD: f64 = 0.06;
/// How long the voice rings on after the phrase's last note.
pub const RELEASE: f64 = 0.08;
/// The pitch curve's step, in seconds.
pub const PITCH_STEP: f64 = 0.01;
/// A lyric that makes the render take [`SLOW_SECONDS`], polling cancel.
pub const SLOW_LYRIC: &str = "slowly";
pub const SLOW_SECONDS: f64 = 0.3;
/// A lyric that makes the render fail.
pub const FAIL_LYRIC: &str = "!fail";

const NAME: &CStr = c"Fontelle Test Singer";
const VERSION: &CStr = c"1.0.0";

struct Voice {
    id: &'static CStr,
    name: &'static CStr,
    languages: &'static CStr,
    lowest: u8,
    highest: u8,
    flags: u32,
}

const VOICES: [Voice; 2] = [
    Voice {
        id: c"test.alto",
        name: c"Test Alto",
        languages: c"en,ja",
        lowest: 53,
        highest: 77,
        flags: 0,
    },
    Voice {
        id: c"test.research",
        name: c"Research Voice",
        languages: c"ja,en",
        lowest: 57,
        highest: 81,
        flags: SINGER_RESEARCH_ONLY,
    },
];

struct Instance {
    last_error: Option<CString>,
}

fn instance<'a>(p: *mut EngineInstance) -> &'a mut Instance {
    // SAFETY: every instance pointer handed out is a `Box<Instance>`, and
    // the ABI promises one call at a time per instance.
    unsafe { &mut *(p as *mut Instance) }
}

unsafe extern "C" fn create() -> *mut EngineInstance {
    Box::into_raw(Box::new(Instance { last_error: None })) as *mut EngineInstance
}

unsafe extern "C" fn destroy(p: *mut EngineInstance) {
    if !p.is_null() {
        drop(unsafe { Box::from_raw(p as *mut Instance) });
    }
}

unsafe extern "C" fn singer_count(_p: *mut EngineInstance) -> u32 {
    VOICES.len() as u32
}

unsafe extern "C" fn singer_info(
    _p: *mut EngineInstance,
    index: u32,
    info: *mut SingerInfo,
) -> bool {
    let Some(voice) = VOICES.get(index as usize) else {
        return false;
    };
    if info.is_null() {
        return false;
    }
    unsafe {
        *info = SingerInfo {
            id: voice.id.as_ptr(),
            name: voice.name.as_ptr(),
            languages: voice.languages.as_ptr(),
            lowest: voice.lowest,
            highest: voice.highest,
            flags: voice.flags,
        };
    }
    true
}

unsafe extern "C" fn last_error(p: *mut EngineInstance) -> *const c_char {
    instance(p)
        .last_error
        .as_ref()
        .map_or(std::ptr::null(), |e| e.as_ptr())
}

fn text(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

struct Note {
    start: f64,
    duration: f64,
    hz: f64,
    lyric: String,
}

impl Note {
    fn end(&self) -> f64 {
        self.start + self.duration
    }
    fn slur(&self) -> bool {
        self.lyric == LYRIC_SLUR
    }
}

enum Failed {
    Cancelled,
    Error(String),
}

fn cancelled(flag: *const AtomicU32) -> bool {
    !flag.is_null() && unsafe { &*flag }.load(Ordering::Relaxed) != 0
}

/// The phonemes a lyric is shown as: brackets as given, a word split into
/// its leading consonants and the rest, a slur nothing.
fn phonemes(lyric: &str) -> Option<Vec<String>> {
    let lyric = lyric.trim().trim_end_matches('-');
    if lyric == LYRIC_SLUR {
        return None;
    }
    if let Some(inner) = lyric.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
        return Some(inner.split_whitespace().map(str::to_string).collect());
    }
    let word = if lyric.is_empty() { "la" } else { lyric };
    if !word.is_ascii() {
        return Some(vec![word.to_string()]);
    }
    let split = word
        .find(|c: char| "aeiouy".contains(c.to_ascii_lowercase()))
        .unwrap_or(word.len());
    let mut out: Vec<String> = word[..split].chars().map(|c| c.to_string()).collect();
    if split < word.len() {
        out.push(word[split..].to_string());
    }
    Some(out)
}

/// How much of harmonic `k` a vowel lets through: a crude formant.
fn vowel_weight(lyric: &str, k: usize) -> f32 {
    let vowel = lyric
        .chars()
        .rev()
        .find(|c| "aeiou".contains(*c))
        .unwrap_or('a');
    let peak = match vowel {
        'i' | 'e' => 3.0,
        'o' | 'u' => 1.0,
        _ => 2.0,
    };
    let d = k as f32 - peak;
    (1.0 / (1.0 + d * d * 0.6)) / k as f32
}

struct Noise(u64);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
    }
}

struct Sung {
    samples: Vec<f32>,
    start: f64,
    pitch: Vec<f32>,
    phonemes: String,
}

fn sing(request: &PhraseRequest) -> Result<Sung, Failed> {
    if cancelled(request.cancel) {
        return Err(Failed::Cancelled);
    }
    let singer = text(request.singer_id);
    let Some(voice) = VOICES.iter().find(|v| v.id.to_str() == Ok(singer.as_str())) else {
        return Err(Failed::Error(format!("no singer called {singer:?}")));
    };
    let language = text(request.language);
    let languages = voice.languages.to_str().unwrap_or("");
    if !languages.split(',').any(|l| l == language) {
        return Err(Failed::Error(format!(
            "{} does not sing {language:?} (only {languages})",
            voice.name.to_string_lossy()
        )));
    }
    let raw: &[PhraseNote] = if request.notes.is_null() || request.note_count == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(request.notes, request.note_count as usize) }
    };
    let notes: Vec<Note> = raw
        .iter()
        .map(|n| Note {
            start: n.start,
            duration: n.duration.max(0.0),
            hz: 440.0 * 2f64.powf((f64::from(n.pitch) - 69.0) / 12.0),
            lyric: text(n.lyric),
        })
        .collect();
    for note in &notes {
        if note.lyric == FAIL_LYRIC {
            return Err(Failed::Error("asked to fail".to_string()));
        }
        if note.lyric == SLOW_LYRIC {
            let until =
                std::time::Instant::now() + std::time::Duration::from_secs_f64(SLOW_SECONDS);
            while std::time::Instant::now() < until {
                if cancelled(request.cancel) {
                    return Err(Failed::Cancelled);
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
    }
    let rate = f64::from(SAMPLE_RATE);
    let start = -LEAD;
    let end = notes.iter().map(Note::end).fold(0.0, f64::max) + RELEASE;
    let frames = ((end - start) * rate).ceil().max(0.0) as usize;
    let mut samples = vec![0.0f32; frames];
    let harmonics = if request.quality == QUALITY_FINAL {
        8
    } else {
        3
    };
    let mut noise = Noise(request.seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let attack = 0.015;
    let release = 0.04;
    let mut phase = 0.0f64;
    let mut gain = 0.0f32;
    for (i, out) in samples.iter_mut().enumerate() {
        if i % 512 == 0 && cancelled(request.cancel) {
            return Err(Failed::Cancelled);
        }
        let t = start + i as f64 / rate;
        // The voice: whichever note is sounding, its slurs joined.
        let sounding = notes.iter().rev().find(|n| t >= n.start && t < n.end());
        let target = if sounding.is_some() { 0.25f32 } else { 0.0 };
        let rate_of = if target > gain { attack } else { release };
        gain += (target - gain) / (rate_of * rate) as f32;
        if let Some(note) = sounding.or_else(|| notes.iter().rev().find(|n| t >= n.end())) {
            let vowel = notes
                .iter()
                .rev()
                .find(|n| n.start <= note.start && !n.slur())
                .map_or("a", |n| n.lyric.as_str());
            phase += note.hz / rate;
            phase -= phase.floor();
            let mut v = 0.0f32;
            for k in 1..=harmonics {
                v += vowel_weight(vowel, k)
                    * (std::f64::consts::TAU * phase * k as f64).sin() as f32;
            }
            *out += v * gain;
        }
        // A breath under it all, where the seed is heard.
        *out += noise.next() * 0.002;
        // The consonant before each new syllable.
        for note in notes.iter().filter(|n| !n.slur()) {
            let before = note.start - t;
            if before > 0.0 && before <= LEAD {
                let shape = (1.0 - before / LEAD) as f32;
                *out += noise.next() * 0.08 * shape;
            }
        }
    }
    let steps = ((end - start) / PITCH_STEP).ceil() as usize;
    let pitch = (0..steps)
        .map(|i| {
            let t = start + i as f64 * PITCH_STEP;
            notes
                .iter()
                .rev()
                .find(|n| t >= n.start && t < n.end())
                .map_or(0.0, |n| n.hz as f32)
        })
        .collect();
    let mut json = String::from("[");
    for (i, note) in notes.iter().enumerate() {
        let Some(ph) = phonemes(&note.lyric) else {
            continue;
        };
        if json.len() > 1 {
            json.push(',');
        }
        json.push_str(&format!("{{\"note\":{i},\"phonemes\":["));
        for (j, p) in ph.iter().enumerate() {
            if j > 0 {
                json.push(',');
            }
            json.push('"');
            for c in p.chars() {
                match c {
                    '"' => json.push_str("\\\""),
                    '\\' => json.push_str("\\\\"),
                    c => json.push(c),
                }
            }
            json.push('"');
        }
        json.push_str("]}");
    }
    json.push(']');
    Ok(Sung {
        samples,
        start,
        pitch,
        phonemes: json,
    })
}

unsafe extern "C" fn render(
    p: *mut EngineInstance,
    request: *const PhraseRequest,
    result: *mut PhraseResult,
) -> i32 {
    let me = instance(p);
    me.last_error = None;
    if request.is_null() || result.is_null() {
        me.last_error = CString::new("no request").ok();
        return STATUS_ERROR;
    }
    match sing(unsafe { &*request }) {
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

unsafe extern "C" fn free_result(_p: *mut EngineInstance, result: *mut PhraseResult) {
    if result.is_null() {
        return;
    }
    let r = unsafe { &mut *result };
    if !r.samples.is_null() {
        let slice = std::ptr::slice_from_raw_parts_mut(r.samples, r.frame_count as usize);
        drop(unsafe { Box::from_raw(slice) });
    }
    if !r.pitch.is_null() {
        let slice = std::ptr::slice_from_raw_parts_mut(r.pitch, r.pitch_count as usize);
        drop(unsafe { Box::from_raw(slice) });
    }
    if !r.phonemes.is_null() {
        drop(unsafe { CString::from_raw(r.phonemes) });
    }
    *r = PhraseResult::EMPTY;
}

struct Table(FontelleSingerEngine);
// SAFETY: function pointers and static strings.
unsafe impl Sync for Table {}

static ENGINE: Table = Table(FontelleSingerEngine {
    abi_version: ABI_VERSION,
    name: NAME.as_ptr(),
    version: VERSION.as_ptr(),
    create,
    destroy,
    singer_count,
    singer_info,
    render,
    free_result,
    last_error,
});

/// The engine's one export.
///
/// # Safety
///
/// None to uphold: it returns a pointer to a static table.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fontelle_singer_entry() -> *const FontelleSingerEngine {
    &ENGINE.0
}

//! The test singer, called through its own table exactly as a host would —
//! what the vocal track's loader and scheduler tests lean on, checked on its
//! own first so a failure there is not a mystery.

use std::ffi::{CStr, CString};
use std::sync::atomic::AtomicU32;

use fontelle_singer_abi::{
    ABI_VERSION, FontelleSingerEngine, PhraseNote, PhraseRequest, PhraseResult, QUALITY_FINAL,
    QUALITY_PREVIEW, SINGER_RESEARCH_ONLY, STATUS_CANCELLED, STATUS_ERROR, STATUS_OK, SingerInfo,
};

fn table() -> &'static FontelleSingerEngine {
    // SAFETY: the entry point returns a pointer to a static table.
    unsafe { &*fontelle_testsinger::fontelle_singer_entry() }
}

fn text(p: *const std::ffi::c_char) -> String {
    assert!(!p.is_null());
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

struct Sung {
    rate: u32,
    samples: Vec<f32>,
    start: f64,
    pitch: Vec<f32>,
    step: f64,
    phonemes: Option<String>,
}

fn sing(
    notes: &[(f64, f64, f32, &str)],
    language: &str,
    quality: u32,
    seed: u64,
    cancel: Option<&AtomicU32>,
) -> Result<Sung, (i32, String)> {
    let t = table();
    let instance = unsafe { (t.create)() };
    assert!(!instance.is_null());
    let lyrics: Vec<CString> = notes.iter().map(|n| CString::new(n.3).unwrap()).collect();
    let phrase: Vec<PhraseNote> = notes
        .iter()
        .zip(&lyrics)
        .map(|(n, l)| PhraseNote {
            start: n.0,
            duration: n.1,
            pitch: n.2,
            lyric: l.as_ptr(),
        })
        .collect();
    let singer = CString::new(fontelle_testsinger::ALTO_ID).unwrap();
    let lang = CString::new(language).unwrap();
    let request = PhraseRequest {
        singer_id: singer.as_ptr(),
        language: lang.as_ptr(),
        notes: phrase.as_ptr(),
        note_count: phrase.len() as u32,
        seed,
        quality,
        cancel: cancel.map_or(std::ptr::null(), |c| c as *const AtomicU32),
    };
    let mut result = PhraseResult::EMPTY;
    let status = unsafe { (t.render)(instance, &request, &mut result) };
    let out = if status == STATUS_OK {
        let samples =
            unsafe { std::slice::from_raw_parts(result.samples, result.frame_count as usize) }
                .to_vec();
        let pitch = if result.pitch.is_null() {
            Vec::new()
        } else {
            unsafe { std::slice::from_raw_parts(result.pitch, result.pitch_count as usize) }
                .to_vec()
        };
        let phonemes = (!result.phonemes.is_null()).then(|| text(result.phonemes));
        let sung = Sung {
            rate: result.sample_rate,
            samples,
            start: result.start,
            pitch,
            step: result.pitch_step,
            phonemes,
        };
        unsafe { (t.free_result)(instance, &mut result) };
        Ok(sung)
    } else {
        let why = unsafe { (t.last_error)(instance) };
        let why = if why.is_null() {
            String::new()
        } else {
            text(why)
        };
        Err((status, why))
    };
    unsafe { (t.destroy)(instance) };
    out
}

/// The dominant period of `samples` between `lo` and `hi` Hz, by
/// autocorrelation — good enough to tell A4 from anything else.
fn fundamental(samples: &[f32], rate: u32) -> f32 {
    let (lo, hi) = (80.0, 1000.0);
    let min_lag = (rate as f32 / hi) as usize;
    let max_lag = (rate as f32 / lo) as usize;
    let mut best = (0usize, f32::MIN);
    for lag in min_lag..max_lag {
        let s: f32 = samples
            .iter()
            .zip(&samples[lag..])
            .map(|(a, b)| a * b)
            .sum();
        if s > best.1 {
            best = (lag, s);
        }
    }
    rate as f32 / best.0 as f32
}

#[test]
fn the_table_says_who_it_is() {
    let t = table();
    assert_eq!(t.abi_version, ABI_VERSION);
    assert_eq!(text(t.name), fontelle_testsinger::ENGINE_NAME);
    assert!(!text(t.version).is_empty());
}

#[test]
fn it_offers_an_alto_and_a_research_only_voice() {
    let t = table();
    let instance = unsafe { (t.create)() };
    let count = unsafe { (t.singer_count)(instance) };
    assert_eq!(count, 2);
    let mut seen = Vec::new();
    for i in 0..count {
        let mut info = SingerInfo {
            id: std::ptr::null(),
            name: std::ptr::null(),
            languages: std::ptr::null(),
            lowest: 0,
            highest: 0,
            flags: 0,
        };
        assert!(unsafe { (t.singer_info)(instance, i, &mut info) });
        seen.push((
            text(info.id),
            text(info.name),
            text(info.languages),
            info.flags,
        ));
        assert!(info.lowest < info.highest);
    }
    let mut info = unsafe { std::mem::zeroed::<SingerInfo>() };
    assert!(!unsafe { (t.singer_info)(instance, count, &mut info) });
    unsafe { (t.destroy)(instance) };
    assert_eq!(seen[0].0, fontelle_testsinger::ALTO_ID);
    assert_eq!(seen[0].2, "en,ja");
    assert_eq!(seen[0].3 & SINGER_RESEARCH_ONLY, 0);
    assert_eq!(seen[1].0, fontelle_testsinger::RESEARCH_ID);
    assert_ne!(seen[1].3 & SINGER_RESEARCH_ONLY, 0);
}

#[test]
fn a_note_is_sung_at_its_pitch_with_a_consonant_before_it() {
    let sung = sing(
        &[(0.0, 0.5, 69.0, "la"), (0.5, 0.5, 76.0, "-")],
        "en",
        QUALITY_PREVIEW,
        1,
        None,
    )
    .unwrap();
    assert!(
        sung.start < 0.0,
        "a consonant leads the note: {}",
        sung.start
    );
    let rate = sung.rate;
    assert!(rate > 0);
    let at = |seconds: f64| ((seconds - sung.start) * rate as f64) as usize;
    // The whole phrase is there, and a little release after it.
    assert!(sung.samples.len() >= at(1.0));
    let a4 = fundamental(&sung.samples[at(0.1)..at(0.4)], rate);
    assert!((a4 - 440.0).abs() < 8.0, "A4 sung as {a4} Hz");
    let e5 = fundamental(&sung.samples[at(0.6)..at(0.9)], rate);
    assert!((e5 - 659.3).abs() < 14.0, "E5 sung as {e5} Hz");
    // Before the note, the consonant: something, but no tone.
    let lead: f32 = sung.samples[..at(0.0)].iter().map(|s| s.abs()).sum();
    assert!(lead > 0.0);
    // Soft: never near full scale.
    assert!(sung.samples.iter().all(|s| s.abs() < 0.6));
    // The pitch curve says the same.
    assert!(sung.step > 0.0 && !sung.pitch.is_empty());
    let hz_at = |seconds: f64| sung.pitch[((seconds - sung.start) / sung.step) as usize];
    assert!((hz_at(0.25) - 440.0).abs() < 1.0);
    assert!((hz_at(0.75) - 659.26).abs() < 1.0);
    let phonemes = sung.phonemes.unwrap();
    assert!(phonemes.contains("\"note\":0"), "{phonemes}");
    assert!(phonemes.contains("\"l\""), "{phonemes}");
}

#[test]
fn the_same_request_and_seed_sing_the_same_take() {
    let a = sing(&[(0.0, 0.3, 60.0, "sa")], "ja", QUALITY_FINAL, 7, None).unwrap();
    let b = sing(&[(0.0, 0.3, 60.0, "sa")], "ja", QUALITY_FINAL, 7, None).unwrap();
    assert_eq!(a.samples, b.samples);
    let c = sing(&[(0.0, 0.3, 60.0, "sa")], "ja", QUALITY_FINAL, 8, None).unwrap();
    assert_ne!(a.samples, c.samples, "the seed moves the breath noise");
}

#[test]
fn a_raised_cancel_flag_is_honoured() {
    let cancel = AtomicU32::new(1);
    let err = sing(
        &[(0.0, 0.3, 60.0, "la")],
        "en",
        QUALITY_PREVIEW,
        1,
        Some(&cancel),
    )
    .err()
    .unwrap();
    assert_eq!(err.0, STATUS_CANCELLED);
}

#[test]
fn a_language_the_singer_lacks_is_an_error_that_says_so() {
    let err = sing(&[(0.0, 0.3, 60.0, "la")], "fr", QUALITY_PREVIEW, 1, None)
        .err()
        .unwrap();
    assert_eq!(err.0, STATUS_ERROR);
    assert!(err.1.contains("fr"), "{}", err.1);
}

#[test]
fn bracketed_phonemes_are_used_as_given() {
    let sung = sing(
        &[(0.0, 0.3, 60.0, "[h ə l]")],
        "en",
        QUALITY_PREVIEW,
        1,
        None,
    )
    .unwrap();
    let phonemes = sung.phonemes.unwrap();
    assert!(phonemes.contains("\"ə\""), "{phonemes}");
}

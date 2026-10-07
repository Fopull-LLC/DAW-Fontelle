//! The C ABI a **singer engine** implements.
//!
//! A vocal track is notes with lyrics. Turning them into a voice is the
//! job of a singer engine: a shared library Fontelle finds at run time
//! (`$XDG_DATA_HOME/fontelle/singers/*.so`, or the folders listed in
//! `FONTELLE_SINGERS`), built from its own repository under its own licence.
//! Fontelle never links against an engine and an engine never links against
//! Fontelle; this crate is **the whole of the shared surface** — `#[repr(C)]`
//! structs, constants and a version number, MIT/Apache like the rest of the
//! tree — exactly as `fontelle-bridge-abi` is for plugin bridges.
//!
//! # The shape
//!
//! The library exports one symbol, [`ENTRY_SYMBOL`], returning a
//! [`FontelleSingerEngine`]: a table of function pointers. The host makes an
//! instance, asks it which singers it has, and asks it to sing **phrases**:
//! a run of notes with no long rest inside, in phrase-relative seconds. The
//! engine answers with mono PCM at its own sample rate, which the host
//! caches, resamples and plays at the right place in the song. Nothing in
//! this ABI runs on the audio thread: rendering is offline work, done ahead
//! of the playhead.
//!
//! # Threads
//!
//! [`create`](FontelleSingerEngine::create),
//! [`destroy`](FontelleSingerEngine::destroy), the singer queries and
//! [`last_error`](FontelleSingerEngine::last_error) are called on one
//! thread at a time. [`render`](FontelleSingerEngine::render) is called on a
//! worker thread and may block for as long as it needs; the host calls it
//! for **one phrase at a time per instance**, so an engine need not be
//! re-entrant. [`free_result`](FontelleSingerEngine::free_result) may be
//! called from any thread. A render may be cancelled by the host setting
//! the request's [`cancel`](PhraseRequest::cancel) flag; the engine checks it
//! when convenient (between stages is enough) and returns
//! [`STATUS_CANCELLED`].
//!
//! # Lyrics
//!
//! Each note carries its lyric as UTF-8 text, in the phrase's language:
//!
//! - a word or syllable (`"hel"`, `"lo"`, `"さ"`, `"sa"`): the engine turns it
//!   into its own phonemes;
//! - [`LYRIC_SLUR`] (`"-"`): the previous note's syllable carries on to this
//!   pitch (a melisma). A word of several syllables written on one note and
//!   followed by slurs is spread across them, a syllable per note, until its
//!   syllables run out (`"amazing" "-" "-"` sings a-ma-zing); slurs beyond
//!   that hold the last syllable;
//! - a syllable ending in a hyphen (`"a-" "ma-" "zing"`) joins the next
//!   note's into one word, for writers who split words by hand;
//! - phonemes in square brackets (`"[h ə l oʊ]"`): used as given, for a word
//!   the engine does not know or a pronunciation the writer wants;
//! - empty: the engine's default syllable.
//!
//! The engine reports back what it sang ([`PhraseResult::phonemes`]) so the
//! piano roll can show it.

use core::ffi::c_char;
use core::sync::atomic::AtomicU32;

/// Bumped on any change to a struct or a function's meaning. An engine whose
/// number differs from the host's is not loaded.
pub const ABI_VERSION: u32 = 1;

/// The one symbol an engine exports, of type [`EntryFn`].
pub const ENTRY_SYMBOL: &str = "fontelle_singer_entry";

/// A note's lyric that continues the previous note's syllable.
pub const LYRIC_SLUR: &str = "-";

/// [`FontelleSingerEngine::render`]'s answers.
pub const STATUS_OK: i32 = 0;
/// The render failed; [`FontelleSingerEngine::last_error`] says why.
pub const STATUS_ERROR: i32 = 1;
/// The host's cancel flag was seen.
pub const STATUS_CANCELLED: i32 = 2;

/// [`SingerInfo::flags`]: a voice for testing only, whose output must not be
/// shared. The host says so wherever the singer is chosen.
pub const SINGER_RESEARCH_ONLY: u32 = 1;

/// [`PhraseRequest::quality`]: fast enough to hear an edit soon.
pub const QUALITY_PREVIEW: u32 = 0;
/// [`PhraseRequest::quality`]: the best the engine does, for playback and
/// export.
pub const QUALITY_FINAL: u32 = 1;

/// The entry point's type.
pub type EntryFn = unsafe extern "C" fn() -> *const FontelleSingerEngine;

/// An engine instance, opaque to the host.
#[repr(C)]
pub struct EngineInstance {
    _private: [u8; 0],
}

/// The table an engine exports. Every pointer is required.
#[repr(C)]
pub struct FontelleSingerEngine {
    /// [`ABI_VERSION`] as the engine was built.
    pub abi_version: u32,
    /// The engine's display name, UTF-8, NUL-terminated, static.
    pub name: *const c_char,
    /// The engine's version, UTF-8, NUL-terminated, static. Part of the
    /// host's cache key: a new version re-renders.
    pub version: *const c_char,
    /// A new instance, or null (then nothing else may be called).
    pub create: unsafe extern "C" fn() -> *mut EngineInstance,
    pub destroy: unsafe extern "C" fn(instance: *mut EngineInstance),
    /// How many singers this instance offers.
    pub singer_count: unsafe extern "C" fn(instance: *mut EngineInstance) -> u32,
    /// Fills `info` for singer `index`; false when out of range. The strings
    /// stay valid until `destroy`.
    pub singer_info: unsafe extern "C" fn(
        instance: *mut EngineInstance,
        index: u32,
        info: *mut SingerInfo,
    ) -> bool,
    /// Sings one phrase into `result`. Returns a `STATUS_*`. On
    /// [`STATUS_OK`] the host owns `result` until it hands it to
    /// `free_result`.
    pub render: unsafe extern "C" fn(
        instance: *mut EngineInstance,
        request: *const PhraseRequest,
        result: *mut PhraseResult,
    ) -> i32,
    /// Frees what `render` put in `result`.
    pub free_result: unsafe extern "C" fn(instance: *mut EngineInstance, result: *mut PhraseResult),
    /// The last failure on this instance, UTF-8, NUL-terminated, valid until
    /// the next call; null when there was none.
    pub last_error: unsafe extern "C" fn(instance: *mut EngineInstance) -> *const c_char,
}

/// One singer an engine offers.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SingerInfo {
    /// Stable id, saved in songs: UTF-8, NUL-terminated.
    pub id: *const c_char,
    /// Display name.
    pub name: *const c_char,
    /// The languages it sings, as comma-separated BCP 47 tags (`"en,ja"`);
    /// the first is its default.
    pub languages: *const c_char,
    /// Its comfortable range, as MIDI note numbers.
    pub lowest: u8,
    pub highest: u8,
    /// `SINGER_*` flags.
    pub flags: u32,
}

/// One note of a phrase.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PhraseNote {
    /// Seconds from the phrase's start (the first note starts at 0).
    pub start: f64,
    pub duration: f64,
    /// MIDI pitch; the fraction is cents (60.5 is C4 plus 50 cents).
    pub pitch: f32,
    /// See the crate's note on lyrics. UTF-8, NUL-terminated.
    pub lyric: *const c_char,
}

/// What the host asks for.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PhraseRequest {
    pub singer_id: *const c_char,
    /// BCP 47 tag, one of the singer's languages.
    pub language: *const c_char,
    pub notes: *const PhraseNote,
    pub note_count: u32,
    /// The same request with the same seed sings the same take.
    pub seed: u64,
    /// `QUALITY_*`.
    pub quality: u32,
    /// Set non-zero by the host to cancel; may be null.
    pub cancel: *const AtomicU32,
}

/// What the engine sang. Every pointer is the engine's, freed by
/// [`FontelleSingerEngine::free_result`].
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PhraseResult {
    pub sample_rate: u32,
    /// Mono samples.
    pub samples: *mut f32,
    pub frame_count: u64,
    /// Where the audio starts, in seconds from the phrase's first note: a
    /// sung consonant begins before its note, so this is usually negative.
    pub start: f64,
    /// The pitch it sang, one value in Hz (0 where unvoiced) every
    /// `pitch_step` seconds from `start`; may be null with `pitch_count` 0.
    pub pitch: *mut f32,
    pub pitch_count: u64,
    pub pitch_step: f64,
    /// What it sang, for display: UTF-8 JSON, NUL-terminated, an array of
    /// `{"note": i, "phonemes": ["s", "a"]}`, one per sung note; may be null.
    pub phonemes: *mut c_char,
}

impl PhraseResult {
    /// A result with nothing in it, for the engine to fill.
    pub const EMPTY: Self = Self {
        sample_rate: 0,
        samples: core::ptr::null_mut(),
        frame_count: 0,
        start: 0.0,
        pitch: core::ptr::null_mut(),
        pitch_count: 0,
        pitch_step: 0.0,
        phonemes: core::ptr::null_mut(),
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entry_symbol_and_version_are_what_engines_build_against() {
        assert_eq!(ENTRY_SYMBOL, "fontelle_singer_entry");
        assert_eq!(ABI_VERSION, 1);
        assert_eq!(LYRIC_SLUR, "-");
    }

    #[test]
    fn an_empty_result_owns_nothing() {
        let r = PhraseResult::EMPTY;
        assert!(r.samples.is_null() && r.pitch.is_null() && r.phonemes.is_null());
        assert_eq!(r.frame_count, 0);
    }
}

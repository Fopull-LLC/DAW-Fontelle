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
//! # Where an engine is installed
//!
//! Loading a library runs its initialisers, so the host loads only what is
//! plainly an engine. In each singers folder it loads:
//!
//! - every library directly in the folder (`singers/*.so`, `*.dll`,
//!   `*.dylib`), and
//! - in each subfolder `singers/<name>/`, only the library named after the
//!   folder: `<name>.so` or `lib<name>.so` (`<name>.dll` on Windows,
//!   `<name>.dylib` / `lib<name>.dylib` on macOS).
//!
//! An engine that ships helper libraries (an inference runtime, its
//! providers) installs as a subfolder, engine and helpers together; the host
//! never opens the helpers itself, and nothing in a subfolder is looked at
//! below that one level. A library that does not export [`ENTRY_SYMBOL`] is
//! reported as not an engine and not used.
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
//! [`last_error`](FontelleSingerEngine::last_error) describes the last
//! failing call **on that instance**, and the host reads it on the thread
//! that made the failing call, immediately after it returned and before
//! anything else is called on the instance: a render's error is read by the
//! render's worker right after `render` answers [`STATUS_ERROR`]. An engine
//! may therefore keep the message per instance, without locks.
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
//!
//! Recommended semantics, which hosts and engines both assume:
//!
//! - a [`LYRIC_SLUR`] after a rest (the first note of a phrase, or one
//!   that does not touch the note before it) sings the previous syllable's
//!   vowel again;
//! - a voice sings one note at a time. The host sends a phrase's notes in
//!   time order without overlaps — where two notes overlap, the later one
//!   wins and the earlier one is cut at its start, and of two starting
//!   together the higher is kept — and an engine handed overlapping notes
//!   anyway clips each at the next one's start.

use core::ffi::{c_char, c_void};
use core::sync::atomic::AtomicU32;

/// Bumped on any change to a struct or a function's meaning. The host loads
/// engines built for any version from [`MIN_ABI_VERSION`] to this one, and
/// reads [`FontelleSingerEngine::v2`] only from engines built for 2 or later.
pub const ABI_VERSION: u32 = 2;

/// The oldest engine ABI the host still loads.
pub const MIN_ABI_VERSION: u32 = 1;

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
    ///
    /// It may be filled in late — an engine that fingerprints the voice it
    /// loaded computes it once its first instance exists — so the host reads
    /// it **after** [`create`](Self::create) has returned an instance, never
    /// only from the bare table, and treats a null or empty string as "not
    /// known yet". Once non-empty it must not change for the life of the
    /// library.
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
    /// **ABI 2.** The engine's version-2 table, or null. Read only when
    /// [`abi_version`](Self::abi_version) is 2 or more: a version-1 engine's
    /// table ends before this field.
    pub v2: *const SingerEngineV2,
}

/// # Version 2: the whole phrase, and the engine's own window
///
/// Version 1 sings notes and lyrics. Version 2 lets an engine be an
/// instrument in its own right:
///
/// - **Capabilities.** The engine says, per singer, which of the vocal
///   track's standard lanes, note expression and phoneme timing it honours,
///   and declares lanes, whole-part settings and per-note options of its
///   own. Fontelle draws the declared lanes in its piano roll under the
///   engine's name and keeps their values in the part.
/// - **The whole phrase.** [`render_phrase`](SingerEngineV2::render_phrase)
///   takes the phrase as `fontelle-sing`'s `Phrase` in JSON: notes with
///   lyrics, phoneme edits, velocity, vibrato and portamento, every drawn
///   lane, the engine's own lanes, settings and its state blob.
/// - **Its own window.** An engine may draw its own editor, with its own
///   design, into a window Fontelle makes and owns (an X11 window id on
///   Linux, an `HWND` on Windows, an `NSView*` on macOS), exactly as a
///   plugin bridge's editor is embedded. The window reads the part it edits
///   and asks for edits through [`HostCallbacks`]; every edit is a Fontelle
///   command, so it is undoable, saved with the song and shared in a
///   collaboration like any other.
///
/// JSON is the format of everything that crosses here beyond version 1:
/// `fontelle-sing`'s types with serde's default representation. Unknown
/// keys are ignored on both sides, so either may grow.
///
/// Threads: [`capabilities`](SingerEngineV2::capabilities) and the editor
/// calls are made on the main thread; `render_phrase` like `render`. An
/// editor gets an instance of its own (made with `create` on the main
/// thread) and never shares it with a render worker.
#[repr(C)]
pub struct SingerEngineV2 {
    /// `fontelle-sing`'s `Capabilities` for `singer_id`, as JSON in a
    /// string the host frees with [`free_string`](Self::free_string); null
    /// when the singer is unknown.
    pub capabilities: unsafe extern "C" fn(
        instance: *mut EngineInstance,
        singer_id: *const c_char,
    ) -> *mut c_char,
    /// Sings `phrase_json` (a `Phrase`) into `result`, as `render` does.
    pub render_phrase: unsafe extern "C" fn(
        instance: *mut EngineInstance,
        phrase_json: *const c_char,
        quality: u32,
        seed: u64,
        cancel: *const AtomicU32,
        result: *mut PhraseResult,
    ) -> i32,
    /// Frees a string this table returned.
    pub free_string: unsafe extern "C" fn(instance: *mut EngineInstance, string: *mut c_char),
    /// Whether the engine has a window of its own; the rest of the editor
    /// calls are made only when it does.
    pub has_editor: unsafe extern "C" fn(instance: *mut EngineInstance) -> bool,
    /// Opens the editor inside `parent`, editing the part `host` describes,
    /// and writes the size it wants. False when it cannot.
    pub editor_open: unsafe extern "C" fn(
        instance: *mut EngineInstance,
        parent: u64,
        host: *const HostCallbacks,
        width: *mut u32,
        height: *mut u32,
    ) -> bool,
    /// Called once a frame while open; the editor does its drawing and event
    /// handling here (it has no thread of its own).
    pub editor_tick: unsafe extern "C" fn(instance: *mut EngineInstance),
    /// The host's window was resized.
    pub editor_resized:
        unsafe extern "C" fn(instance: *mut EngineInstance, width: u32, height: u32),
    /// The part changed (an edit anywhere, a render landed, the selection or
    /// the playhead moved): read it again.
    pub editor_part_changed: unsafe extern "C" fn(instance: *mut EngineInstance),
    pub editor_close: unsafe extern "C" fn(instance: *mut EngineInstance),
}

/// What the host offers an engine's window. Valid from `editor_open` to
/// `editor_close`; called only on the main thread, from inside
/// `editor_tick` or `editor_part_changed`.
#[repr(C)]
pub struct HostCallbacks {
    pub context: *mut c_void,
    /// The part as `fontelle-sing`'s `PartView`, JSON, freed with
    /// `free_string`.
    pub get_part: unsafe extern "C" fn(context: *mut c_void) -> *mut c_char,
    /// Applies `edits_json` (a JSON array of `PartEdit`) as one undo step.
    /// False, with nothing changed, when any edit is invalid.
    pub apply_edits: unsafe extern "C" fn(context: *mut c_void, edits_json: *const c_char) -> bool,
    /// `fontelle-sing`'s `Transport`, JSON.
    pub transport: unsafe extern "C" fn(context: *mut c_void, transport_json: *const c_char),
    /// The current take of the phrase at `seconds` (song time), as a
    /// `PhraseResult` the engine must not free, or false when none is sung
    /// yet: lets the window draw waveforms and sung pitch.
    pub take_at:
        unsafe extern "C" fn(context: *mut c_void, seconds: f64, take: *mut PhraseResult) -> bool,
    pub free_string: unsafe extern "C" fn(context: *mut c_void, string: *mut c_char),
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
    ///
    /// `note` is the index into [`PhraseRequest::notes`]. Objects may carry
    /// more keys; a host **ignores keys it does not know**, so an engine can
    /// add them without a new ABI version. Keys hosts understand today:
    ///
    /// - `"guessed": true` — the engine did not know the word and guessed
    ///   its pronunciation; the host marks the lyric so the writer can give
    ///   the phonemes in brackets;
    /// - `"word"` (string), `"source"` (string), `"slur"` (bool) —
    ///   informative; shown or ignored as the host likes.
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
        assert_eq!(ABI_VERSION, 2);
        assert_eq!(MIN_ABI_VERSION, 1);
        assert_eq!(LYRIC_SLUR, "-");
    }

    #[test]
    fn an_empty_result_owns_nothing() {
        let r = PhraseResult::EMPTY;
        assert!(r.samples.is_null() && r.pitch.is_null() && r.phonemes.is_null());
        assert_eq!(r.frame_count, 0);
    }
}

//! The Fontelle Singer's classic UTAU runtime.
//!
//! Sings UTAU voicebanks — WAV samples cut up by `oto.ini` — through the
//! [`fontelle_sing`] contract. Everything a producer used to need UTAU or
//! OpenUtau plus an external resampler and wavtool for is built in:
//!
//! - **Banks** ([`bank`]): `character.txt`/`character.yaml`, `readme.txt`
//!   (the author's terms), an `oto.ini` in every subfolder in Shift-JIS,
//!   GBK or UTF-8 ([`text`]), `prefix.map` and `character.yaml` subbanks
//!   (multi-pitch), voice colours (appends) as a per-note option.
//! - **Phonemizers** ([`phonemize`]): Japanese CV, VCV and CVVC (kana or
//!   romaji lyrics, [`kana`]), English ARPAsing (CMUdict, [`dict`]) and
//!   VCCV basics; picked from the bank's aliases, overridable.
//! - **Resampler and wavtool** ([`world`], [`render`]): every sample is
//!   analysed once into F0, spectral envelope and aperiodicity (a WORLD-style
//!   vocoder written here), cached in memory and on disk ([`cache`]); a
//!   phrase is assembled in that parameter domain — consonants kept,
//!   vowels stretched, units crossfaded over their overlaps — and
//!   synthesised once on the pitch line the notes make.

pub mod bank;
pub mod cache;
pub mod dict;
pub mod kana;
pub mod oto;
pub mod phonemize;
pub mod render;
pub mod text;
pub mod wav;
pub mod world;

mod backend;

pub use backend::{
    NOTE_OPTION_COLOUR, OUTPUT_RATE, SETTING_MODULATION, SETTING_PHONEMIZER, UtauBackend,
    UtauSession, sing_with,
};

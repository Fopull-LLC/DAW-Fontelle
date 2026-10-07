//! Language modules for singing: what a lyric written in a language's own
//! script says, in the sounds voicebanks are keyed by.
//!
//! - [`pinyin`]: Chinese characters to toneless pinyin (`你好` → `ni` `hao`),
//!   the most common reading of each character, common words that read a
//!   character otherwise (`银行` → `yin` `hang`), and a reading written in
//!   brackets after a character for anything else (`行[hang]`).
//! - [`hangul`]: Korean syllable blocks taken apart algorithmically
//!   (Unicode's own arithmetic) into onset, vowel and coda, romanised the
//!   way Korean voicebanks name their sounds, with the sound changes that
//!   matter most when it is sung (a coda carried into a following vowel,
//!   nasalisation before `ㄴ`/`ㅁ`).
//!
//! Nothing here knows a voicebank; the runtimes (`fontelle-utau`,
//! `fontelle-diffsinger`) look the results up in their banks.

pub mod hangul;
pub mod pinyin;

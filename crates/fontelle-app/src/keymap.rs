//! Which keys an instrument can actually play, and what each one is called.
//!
//! Reported from using the window: *"often I will use drum soundfonts however
//! the piano roll shows these the same as normal ones, making it so that you
//! have no idea which notes actually play anything."* On a kit with four hits
//! spread over two octaves, finding them meant clicking every row.
//!
//! Both facts this needs were already in the patch and neither had any way to
//! reach a canvas:
//!
//! - **What sounds.** `Layer::key_range` is not decoration — `Voice::
//!   trigger_note` tests every layer against the key and marks the ones that
//!   do not cover it inactive, so a note outside all of them starts a voice
//!   with nothing in it and is genuinely silent. Greying those rows shows a
//!   fact the engine already acts on.
//! - **What it is called.** Since the importer began keeping sample names
//!   (`ImportedPatch::names`, and the same on the reopen path), the layer on
//!   key 38 knows it is the snare.
//!
//! It lives here for the reason [`crate::instrument::describe`] does: the UI
//! may not see a `Patch` (INVARIANT 2), and `fontelle-app` is the one layer
//! that sees both it and the sample library.

use fontelle_core::{Patch, Source};
use fontelle_ui::document::{KeyInfo, KeyMap};

use crate::library::SampleLibrary;

/// The widest a zone may be and still count as a **hit** — one key of a key
/// map rather than one step of a pitched range.
///
/// Two, and it is measured rather than guessed. The soundfonts this was
/// written against put a melodic multisample's zones at three to seven keys
/// (`STR_Ensemble.sf2`'s string ensemble: 3, 4, 5, 6, 7, 31, 37) and a kit's
/// hits at exactly one (`SOM Percussion`: eleven zones, all one key). Three
/// would start labelling string sections; two never does.
pub const HIT_SPAN_KEYS: u8 = 2;

/// Words that say a sample, or a preset, is a drum — matched anywhere in a
/// name, case aside: `ClHi-Hat`, `BassDrum`, `GT01AKicks`.
const DRUM_PARTS: &[&str] = &[
    "kick",
    "snare",
    "hihat",
    "hi-hat",
    "hi hat",
    "hat",
    "cymbal",
    "crash",
    "cowbell",
    "conga",
    "bongo",
    "tamb",
    "shaker",
    "clap",
    "drum",
    "perc",
    "rimshot",
    "timbale",
    "cabasa",
    "maraca",
    "guiro",
    "clave",
    "woodblock",
    "cuica",
    "agogo",
    "808",
    "909",
];

/// Shorter ones, which only count as a whole word (a trailing number aside:
/// `tom2`, `bd1`) — `tom` inside `bottom` is not a drum.
const DRUM_WORDS: &[&str] = &[
    "bd", "sd", "hh", "oh", "ch", "tom", "toms", "ride", "rim", "kit", "cym", "splash", "china",
    "stick",
];

/// Whether `name` names a drum.
fn names_a_drum(name: &str) -> bool {
    let low = name.to_lowercase();
    DRUM_PARTS.iter().any(|part| low.contains(part))
        || low
            .split(|c: char| !c.is_ascii_alphanumeric())
            .flat_map(|word| [word, word.trim_end_matches(|c: char| c.is_ascii_digit())])
            .any(|word| DRUM_WORDS.contains(&word))
}

/// Whether `name` names a note: a letter A–G not inside a word, an optional
/// sharp or flat, and an octave — `B1(L)`, `G#3STAS`, `f#3L`, `A_2L` — or a
/// solfège syllable as a word (`1_Fa#_Kz`).
fn names_a_note(name: &str) -> bool {
    let b = name.as_bytes();
    let note_at = |i: usize| {
        let mut j = i + 1;
        if matches!(b.get(j), Some(b'#' | b'b' | b's')) {
            j += 1;
        }
        if matches!(b.get(j), Some(b'-' | b'_')) {
            j += 1;
        }
        b.get(j).is_some_and(u8::is_ascii_digit) && !b.get(j + 1).is_some_and(u8::is_ascii_digit)
    };
    let lettered = (0..b.len()).any(|i| {
        (b'a'..=b'g').contains(&b[i].to_ascii_lowercase())
            && (i == 0 || !b[i - 1].is_ascii_alphabetic())
            && note_at(i)
    });
    lettered
        || name
            .to_lowercase()
            .split(|c: char| !(c.is_ascii_alphabetic() || c == '#'))
            .map(|word| word.trim_end_matches('#'))
            .any(|word| ["do", "re", "mi", "fa", "sol", "la", "si", "ti"].contains(&word))
}

/// Whether a patch with narrow zones is a **kit** — keys that are different
/// sounds — or a melodic instrument sampled a key or two at a time.
///
/// > *"several soundfonts, even though mapped onto keys correctly it seems,
/// > are not counted as a correct keyboard for the scale / key / note stuff"*
/// > (Ty, 2026-10-27)
///
/// This was "one narrow zone and it is a kit" (`KEY_MAP_HITS = 1`), measured
/// on a bank of 617 presets where only two melodic ones had a narrow zone at
/// all. Ty's bank is 2,106 presets now, and that rule read **267** melodic
/// presets as kits — Deltarune's pianos, strings and guitars, a clarinet
/// sampled on every key — and a kit gets no scale. The shape cannot tell a
/// clarinet on every key from a kit on every key; the names can. First to
/// answer wins:
///
/// 1. the preset's name (`PRC Drumkit`, `SOM Percussion`, `TR-808 Kit`);
/// 2. the samples' names, a drum's word against a note's (`kick` against
///    `STW PIANO f B1`), and the drums must outnumber the notes;
/// 3. with nothing in a name, the shape: most narrow zones playing their
///    sample at one pitch whatever the key, over most of what plays — no
///    instrument does that;
/// 4. else melodic.
///
/// Measured over Ty's bank: 101 of the 2,106 presets read as kits, 55 of
/// them the 57 in bank 128 — the two left are GXSCC's `31` (samples named
/// `300`–`306`, nothing to go by) and one snare stretched over every key —
/// and the 46 outside it are all kits (`Metal Kit`, `TR-808 Kit`, `dj TW`'s
/// NES drum hits by their shape).
fn is_kit(
    patch: &Patch,
    library: &SampleLibrary,
    preset: Option<&str>,
    is_hit: impl Fn(&fontelle_core::Layer) -> bool,
) -> bool {
    let narrow: Vec<_> = patch.layers.iter().filter(|layer| is_hit(layer)).collect();
    if narrow.is_empty() {
        return false;
    }
    if preset.is_some_and(names_a_drum) {
        return true;
    }
    let names: Vec<&str> = patch
        .layers
        .iter()
        .filter_map(|layer| match layer.source {
            Source::Sample { file } | Source::Sf2Zone { file, .. } => library.name(file),
            Source::Drum(_) => Some("drum"),
            Source::Oscillator(_) | Source::Synth(_) => None,
        })
        .collect();
    let drums = names.iter().filter(|name| names_a_drum(name)).count();
    let notes = names.iter().filter(|name| names_a_note(name)).count();
    if drums > notes {
        return true;
    }
    if notes > 0 {
        return false;
    }
    let fixed = narrow
        .iter()
        .filter(|layer| layer.root_key < layer.key_range.0 || layer.root_key > layer.key_range.1)
        .count();
    let span = |layers: &mut dyn Iterator<Item = &fontelle_core::Layer>| {
        let mut keys = [false; 128];
        for layer in layers {
            for key in layer.key_range.0..=layer.key_range.1.min(127) {
                keys[usize::from(key)] = true;
            }
        }
        keys.iter().filter(|on| **on).count()
    };
    let narrow_keys = span(&mut narrow.iter().copied());
    let all_keys = span(&mut patch.layers.iter());
    fixed * 2 > narrow.len() && narrow_keys * 2 > all_keys
}

/// The widest a zone may be and still have its name put on the keys it covers,
/// even in a key map.
///
/// Thirty-two — a bit over two octaves — and measured like the rest. In the
/// bank this was developed against, the widest band in any real percussion
/// preset is 28 keys (`SMW Percussion`'s toms), while the only two melodic
/// presets that contain a hit at all carry zones of 48, 56 and 59. The cut
/// falls cleanly between them, so every kit keeps every band and no key ever
/// gets a label that is really the name of the whole instrument repeated down
/// forty rows.
pub const MAX_NAMED_BAND_KEYS: u8 = 32;

/// Builds the [`KeyMap`] for `patch`.
///
/// Velocity is deliberately not consulted. A layer's `vel_range` can make a
/// key silent at one velocity and loud at another, and a keyboard that greys
/// itself in and out as the pen pressure changes would be worse than one that
/// tells the truth about the key. A key counts as playable if any layer covers
/// it at any velocity.
pub fn key_map(patch: &Patch, library: &SampleLibrary) -> KeyMap {
    key_map_for(patch, library, None)
}

/// [`key_map`] for a patch loaded from a preset called `preset` — whose name
/// is the surest word on whether it is a kit (see [`is_kit`]).
pub fn key_map_for(patch: &Patch, library: &SampleLibrary, preset: Option<&str>) -> KeyMap {
    let mut keys = vec![KeyInfo::default(); KeyMap::KEYS];

    let is_hit = |layer: &fontelle_core::Layer| {
        layer.key_range.1.saturating_sub(layer.key_range.0) < HIT_SPAN_KEYS
    };
    // Decided once, for the whole patch, before any key is looked at — see
    // [`is_kit`]. Asking it per zone is what left real kits half labelled.
    let is_key_map = is_kit(patch, library, preset, is_hit);

    for layer in &patch.layers {
        let (low, high) = layer.key_range;
        let band = high.saturating_sub(low).saturating_add(1);
        // Nothing on a melodic instrument: a name on one key is what made the
        // roll take a whole piano for a kit and give it no scale.
        let nameable = is_key_map && (band <= MAX_NAMED_BAND_KEYS || is_hit(layer));
        let name = nameable
            .then(|| match layer.source {
                Source::Sample { file } | Source::Sf2Zone { file, .. } => library.name(file),
                // A drum machine's hits are named by the kit rather than by a
                // file, and that is the whole of what makes *"you can play
                // them all in the piano roll all labeled"* true: a kit's
                // layers are one-key zones, so this function already reads it
                // as a key map — it only had to be told where the names are.
                Source::Drum(_) => drum_name(layer),
                // A Flopsynth layer plays every key, so it is never a key
                // map and never nameable — the same answer an oscillator
                // gets, and for the same reason.
                Source::Oscillator(_) | Source::Synth(_) => None,
            })
            .flatten();

        for key in low..=high {
            let Some(info) = keys.get_mut(usize::from(key)) else {
                continue;
            };
            info.playable = true;
            // First layer wins. A kit that splits one hit across velocity
            // layers names them all much the same, and the alternative —
            // whichever happened to be last in the file — is not better, just
            // less predictable.
            if info.name.is_none()
                && let Some(name) = name
            {
                info.name = Some(name.to_string());
            }
        }
    }

    KeyMap::new(keys)
}

/// What a drum layer is called, by the key it sits on.
///
/// Read off [`fontelle_core::GM_DRUM_MAP`] rather than stored on the layer,
/// because a `Source::Drum` carries a *sound* and not a name — the same
/// separation every other source keeps, where the audio is the layer's and the
/// label comes from beside it. A hit moved off the General MIDI map is
/// unnamed rather than mislabelled: a wrong name on a row is worse than none.
fn drum_name(layer: &fontelle_core::Layer) -> Option<&'static str> {
    let (low, high) = layer.key_range;
    if low != high {
        return None;
    }
    fontelle_core::GM_DRUM_MAP
        .iter()
        .find(|slot| slot.key == low)
        .map(|slot| slot.name)
}

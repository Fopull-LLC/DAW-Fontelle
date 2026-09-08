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

/// How many hits a patch needs before it is read as a **key map** rather than
/// an instrument.
///
/// **This is the judgement call in the file**, and the per-zone rule above is
/// not enough on its own — which is the thing measuring real files taught. A
/// kit is not all one-key zones: `FZ Percussion` is five hits plus one
/// thirteen-key band, `MMX Percussion` three plus a 3, a 13, a 14 and two
/// 17s. Those wide zones are a percussion sample stretched across a band, and
/// judging each zone alone left F-Zero naming 5 of its 18 playable keys and
/// Mega Man X 3 of 67 — most of the trial and error this exists to remove.
///
/// So the question is asked of the **patch**: one hit and it is a key map,
/// and then every covered key takes the name of the zone covering it,
/// stretched bands included.
///
/// One rather than two, and that is the measurement, not a guess. Over the
/// 617 presets in the bank this was developed against: of 606 melodic
/// presets, **two** contain a one-key zone at all, and both contain five — so
/// they are read as key maps either way. Of 11 percussion presets, all 11
/// contain at least one hit and only 10 contain two. Requiring two would have
/// cost a real kit (`Z3 Percussion`, one hit and one eleven-key band) and
/// bought no protection that the corpus shows is needed. The safety is in
/// [`HIT_SPAN_KEYS`] being narrow, not in counting.
pub const KEY_MAP_HITS: usize = 1;

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
    let mut keys = vec![KeyInfo::default(); KeyMap::KEYS];

    let is_hit = |layer: &fontelle_core::Layer| {
        layer.key_range.1.saturating_sub(layer.key_range.0) < HIT_SPAN_KEYS
    };
    // Decided once, for the whole patch, before any key is looked at — see
    // [`KEY_MAP_HITS`]. Asking it per zone is what left real kits half
    // labelled.
    let is_key_map = patch.layers.iter().filter(|l| is_hit(l)).count() >= KEY_MAP_HITS;

    for layer in &patch.layers {
        let (low, high) = layer.key_range;
        let band = high.saturating_sub(low).saturating_add(1);
        let nameable = (is_key_map && band <= MAX_NAMED_BAND_KEYS) || is_hit(layer);
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

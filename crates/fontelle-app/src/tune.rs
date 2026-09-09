//! What the pitch corrector's window shows (`docs/tune-plan.md` §7).
//!
//! The layer allowed to see both halves: a `fontelle_types::TuneConfig` on one
//! side and `fontelle_ui`'s [`TuneView`] on the other. The window knows
//! nothing about a config and the config knows nothing about a window, which
//! is INVARIANT 4 and the reason this file exists — the sibling of
//! [`crate::flopsynth`], one crate over.

use fontelle_types::{
    ParamTarget, TUNE_NOTE_PARAMS, TuneConfig, TuneFrame, TuneRange, cents_of_hz,
};
use fontelle_ui::canvas::{FlopsynthCard, FlopsynthPicture, TuneView, effect_params};

/// How many hops of trace the viewport draws, at this mode's hop.
///
/// Four seconds of it, which at a 64-sample hop at 48 kHz is three thousand
/// frames — computed rather than fixed, because the hop is half that in Live
/// mode and the picture must still be four seconds wide.
pub fn trace_hops(config: &TuneConfig, sample_rate: f32) -> usize {
    let hop = config.mode.hop().max(1) as f32;
    ((fontelle_ui::canvas::TRACE_SECONDS * sample_rate / hop).round() as usize).max(1)
}

/// The shape of each card: which band of the window it belongs to, whether it
/// stands aside, and how many cells across it is (§7.2).
///
/// Read off the card's own heading rather than its index, for the reason
/// `flopsynth::shape_of` gives — this is the layer that knows what each card
/// *is*, and the shape of the page is a fact about that.
///
/// **Two bands**, in the order §3.1's diagram runs.
///
/// The first is what is being listened to, how hard it is pulled and how the
/// grains are laid: the three cards somebody actually works in. The second is
/// everything you set and leave — what the notes are, the vibrato on top of
/// them, the colour and the output.
///
/// The arrangement is a fitting problem as much as a taxonomy: eight cards on
/// two bands of three-and-five fill both rows across, where three bands of
/// three-three-two left a window's width of empty ground under the last one.
/// The order within each band is still the signal's.
///
/// The **Input card is five cells wide and not four**, because
/// "Baritone/Bass" is past `WIDE_CHOICE` and its chooser takes two of them.
pub fn shape_of(name: &str) -> (usize, bool, usize) {
    match name {
        "Input" => (0, false, 5),
        "Correction" => (0, false, 5),
        "Voice" => (0, false, 7),
        "Scale" => (1, false, 4),
        "Vibrato" => (1, false, 6),
        // The source and the bend switch (§7.2), beside the scale they are an
        // alternative to: three cards about one question — what the notes are.
        "MIDI" => (1, false, 2),
        // On the same band, not a third one: five cards across fills a row of
        // this window almost exactly, and a band of its own for two cards left
        // half the page empty under them.
        "Character" => (1, false, 4),
        "Output" => (1, false, 2),
        _ => (1, false, 2),
    }
}

/// The parameters a card does **not** draw.
///
/// The twelve note switches are controls of the keyboard rather than of the
/// Scale card (§7.2): they are `ParamSpec`s so a lane can automate the scale
/// by section, and they are drawn as keys because twelve switches in a row is
/// not a keyboard and a keyboard is what a person reads a scale off.
fn is_a_key(id: &str) -> bool {
    TUNE_NOTE_PARAMS.contains(&id)
}

/// The bend switch is declared in the Scale section — it is part of what the
/// notes mean — but §7.2 draws it in the **MIDI** card, beside the drop-down
/// that says where the notes come from. Two controls about MIDI in the card
/// called MIDI; the Scale card is then the scale and nothing else.
const MIDI_BEND: &str = "midi_bend";

/// Everything the window needs, from the config, the node and the rack.
///
/// `trace` is what the tap held; `held` is which classes are down on the
/// channel this insert listens to; `channels` is the rack, by name, for the
/// MIDI drop-down.
// Nine, and every one of them is a different source: the document (name,
// track, slot, config), the tap (trace, held), the rack (channels, source)
// and the engine (sample rate). Bundling them into a struct would make a type
// whose only job is to be unpacked one line later — see `flopsynth::describe`,
// which takes the same shape for the same reason.
#[allow(clippy::too_many_arguments)]
pub fn describe(
    track_name: &str,
    track: fontelle_types::MixerTrackId,
    slot: usize,
    config: &TuneConfig,
    trace: Vec<TuneFrame>,
    held: u16,
    channels: &[String],
    source: Option<usize>,
    sample_rate: f32,
) -> TuneView {
    let whole = fontelle_types::EffectConfig::Tune(*config);
    let params = effect_params(&whole, |id| {
        ParamTarget::Insert {
            track,
            slot,
            param: id.to_string(),
        }
        .address()
    });

    // One card per section the config declares, each taking the next run of
    // the table — the same rule the generic panel follows, so a control added
    // to §4 appears here without anybody writing it. The keys are pulled out
    // *after* the run is taken, so the counts still line up with the sections.
    let mut params = params.into_iter();
    let mut bend = None;
    let mut cards: Vec<FlopsynthCard> = whole
        .sections()
        .iter()
        .map(|section| {
            let taken: Vec<_> = params.by_ref().take(section.count).collect();
            let mut kept = Vec::with_capacity(taken.len());
            for param in taken {
                let id = ParamTarget::parse(&param.address).and_then(|target| match target {
                    ParamTarget::Insert { param, .. } => Some(param),
                    _ => None,
                });
                match id.as_deref() {
                    // Kept, not dropped: the MIDI card draws it below.
                    Some(MIDI_BEND) => bend = Some(param),
                    Some(id) if is_a_key(id) => {}
                    _ => kept.push(param),
                }
            }
            let (row, aside, columns) = shape_of(section.name);
            FlopsynthCard {
                group: fontelle_ui::canvas::InstrumentGroup {
                    name: section.name.to_string(),
                    params: kept,
                },
                picture: FlopsynthPicture::None,
                oscillator: None,
                row,
                aside,
                columns,
                removable: false,
            }
        })
        .collect();

    // The seventh card, which no section declares because only one of its two
    // controls is a parameter. `source` is the routing edge `EffectSlot.notes`
    // (§5) and carries `TUNE_SOURCE` rather than an address, so the window
    // sends it to `set_insert_notes`; the bend switch beside it is an ordinary
    // parameter that was declared under Scale and belongs, to a person
    // reading the console, here.
    let sources: Vec<String> = std::iter::once(fontelle_ui::canvas::NO_MIDI.to_string())
        .chain(channels.iter().cloned())
        .collect();
    let at = source.map_or(0, |index| index + 1);
    let (row, aside, columns) = shape_of("MIDI");
    cards.push(FlopsynthCard {
        group: fontelle_ui::canvas::InstrumentGroup {
            name: "MIDI".to_string(),
            params: std::iter::once(fontelle_ui::canvas::InstrumentParam {
                address: fontelle_types::ParamAddress::new(fontelle_ui::canvas::TUNE_SOURCE),
                // "Source", not "Notes from": the caption has one cell and
                // `cell_span` widens a chooser on its *options*, not on its
                // name, so a ten-character caption here runs into the bend
                // switch beside it. In the card called MIDI, "Source" says it.
                label: "Source".to_string(),
                // Normalised the way every chooser is, so `choice_index` reads
                // it back and the pips draw without a special case.
                value: choice_value(at, sources.len()),
                display: sources.get(at).cloned().unwrap_or_default(),
                kind: fontelle_ui::canvas::ParamKind::Choice(sources.clone()),
                automated: false,
            })
            .chain(bend)
            .collect(),
        },
        picture: FlopsynthPicture::None,
        oscillator: None,
        row,
        aside,
        columns,
        removable: false,
    });

    // The viewport's axis: the range's own two frequencies, so a note the
    // tracker cannot find is a note off the top or bottom of the picture
    // rather than one drawn in the middle of it.
    let range: TuneRange = config.range;
    let trimmed = {
        let want = trace_hops(config, sample_rate);
        let from = trace.len().saturating_sub(want);
        trace[from..].to_vec()
    };

    TuneView {
        title: format!("{track_name} \u{2014} TUNE \u{b7} pitch correction"),
        cards,
        mask: config.active_mask(),
        root: config.root,
        held,
        // Two octaves centred on the range, starting at a C so the picture
        // reads like a keyboard rather than like a slice of one.
        keyboard_from: keyboard_from(range),
        trace: trimmed,
        floor_cents: cents_of_hz(range.min_hz()),
        ceiling_cents: cents_of_hz(range.max_hz()),
        latency_ms: config.latency_samples(sample_rate) as f32 / sample_rate.max(1.0) * 1000.0,
        engine: config.engine.label().to_string(),
        mode: config.mode.label().to_string(),
        sources,
        source: at,
    }
}

/// The lowest key the keyboard draws: the C at or under the middle of the
/// range, so the two octaves straddle where the singing is.
fn keyboard_from(range: TuneRange) -> u8 {
    let middle = (cents_of_hz(range.min_hz()) + cents_of_hz(range.max_hz())) / 2.0;
    let semitone = (middle / 100.0).round() as i32;
    // Down to the C below, then down one more octave so the middle sits in the
    // upper half of the two the keyboard shows.
    let c = semitone - semitone.rem_euclid(12) - 12;
    c.clamp(0, 108) as u8
}

/// Where a chooser's row `at` sits on the 0..1 every control here carries.
///
/// The inverse of `fontelle_ui::canvas::choice_index`, which is what reads it
/// back; a list of one is 0 rather than a division by zero.
fn choice_value(at: usize, len: usize) -> f32 {
    if len < 2 {
        return 0.0;
    }
    (at.min(len - 1)) as f32 / (len - 1) as f32
}

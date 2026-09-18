//! What Flopsynth's window shows (`docs/flopsynth-plan.md` §8, §9.4).
//!
//! The layer allowed to see both halves: a `fontelle_core::Patch` on one side
//! and `fontelle_ui`'s [`FlopsynthView`] on the other. The window knows
//! nothing about a patch and the patch knows nothing about a window, which is
//! INVARIANT 4 and the reason this file exists.
//!
//! # The cards, and the pictures in them
//!
//! [`instrument::describe_flopsynth`](crate::instrument::describe_flopsynth)
//! already builds one group per card, in the order §8.3 draws them, with every
//! control at its §4 address. What is added here is the two things a *window*
//! needs and a list does not:
//!
//! - **which band** each card belongs to, so the layout is the signal path
//!   rather than a heap of boxes (§8.1 rule 1); and
//! - **the picture**, computed from the same numbers the voice reads — the
//!   oscillator's current frame, the filter's real response, the envelope's
//!   own stages, the LFO's own shape. `effect.rs` makes the argument for the
//!   EQ curve and it is the same one here: a picture that lies is believed.

use fontelle_core::Patch;
use fontelle_ui::canvas::{
    FlopsynthCard, FlopsynthPage, FlopsynthPicture, FlopsynthRoute, FlopsynthView, InstrumentGroup,
    InstrumentParam, KnobSize,
};

/// How many points a wave picture is drawn from.
///
/// A hundred and twenty-eight across sixty-odd pixels: more than the picture
/// can show, which is what keeps a saw's edge an edge rather than a stair.
const WAVE_POINTS: usize = 128;

/// And how many a filter's response is.
const RESPONSE_POINTS: usize = 96;

/// How many columns a recording's picture has.
const SOUND_COLUMNS: usize = 96;

/// How many harmonics a string's picture spans, and the frequency its
/// partials are drawn up to — middle C's thirty-second, which is what a
/// picture forty pixels tall can still separate.
const PARTIALS_DRAWN: usize = 32;
const PARTIALS_TOP_HZ: f32 = fontelle_core::OSC_ROOT_HZ * (PARTIALS_DRAWN as f32 + 0.5);

/// A recording's shape: the lowest and highest sample in each of
/// [`SOUND_COLUMNS`] slices of it. The picture the arrangement draws of a
/// clip, at card size.
fn sound_peaks(samples: &[f32]) -> Vec<(f32, f32)> {
    if samples.is_empty() {
        return Vec::new();
    }
    (0..SOUND_COLUMNS)
        .map(|column| {
            let from = column * samples.len() / SOUND_COLUMNS;
            let to = ((column + 1) * samples.len() / SOUND_COLUMNS).max(from + 1);
            samples[from..to.min(samples.len())]
                .iter()
                .fold((f32::MAX, f32::MIN), |(low, high), s| {
                    (low.min(*s), high.max(*s))
                })
        })
        .collect()
}

/// Which page a card of this name belongs on (§8.3–§8.6).
///
/// Read off the heading, like [`shape_of`], and for the same reason: the number
/// of cards changes with the patch, so a card's page cannot be its index.
///
/// The split is by **what the controls are about**. The Synth page is making
/// the sound; Modulation is what moves it, which is the LFOs, the two spare
/// envelopes and the matrix itself; Effects is the chain after the voice. The
/// amp and filter envelopes stay on the Synth page even though they are
/// modulators, because they are the two every patch uses to shape its own
/// sound and a synth page without an amp envelope is not one.
fn page_of(name: &str) -> FlopsynthPage {
    match name {
        // Every envelope is off the Synth page since 2026-09-18 (Ty's call
        // on `docs/flopsynth-next.md` §3.5): the page is two bands of
        // consoles over the mod strip, and the envelopes are edited in the
        // strip's inspector — here on the Modulation page until it lands.
        n if n.starts_with("LFO") || n.starts_with("ENV") => FlopsynthPage::Modulation,
        n if n.starts_with("Modulation") => FlopsynthPage::Modulation,
        n if n.starts_with("FX ") => FlopsynthPage::Effects,
        _ => FlopsynthPage::Synth,
    }
}

/// The shape of each card: which band of the window it belongs to, whether it
/// stands aside in the column down the right, and how many cells across it is.
///
/// Read off the card's own heading rather than its index, because the number
/// of cards changes: a patch with no effects has no effect cards, and one with
/// three has three. Declared here rather than worked out by the layout for
/// the reason `FlopsynthCard::row` gives — this is the layer that knows what
/// each card *is*, and the shape of the page is a fact about that.
///
/// The Synth page is two bands (§3.5, with the envelopes off it): the
/// sources, with the sub and the noise set aside so the three oscillators
/// can be five cells wide beside them; and the filters with the channel's
/// own two knobs, the voice and the macros. The widths are the fit's at
/// 1180×840 on the 56×72 grid: three oscillators at five and the aside at
/// four fill the width to twelve pixels, and two filters at four with the
/// three small cards leave a hundred over.
fn shape_of(name: &str) -> (usize, bool, usize) {
    match name {
        // The sources, across the top.
        "OSC A" | "OSC B" | "OSC C" => (0, false, 5),
        "SUB" | "NOISE" => (0, true, 4),
        // What they go through, the way out, and what shapes the voice.
        n if n.starts_with("Filter") => (1, false, 4),
        "Voice" => (1, false, 3),
        "Macros" => (1, false, 4),
        // The Modulation page's own bands: the LFOs, then the envelopes —
        // four across, so four of each fit a row.
        n if n.starts_with("LFO") => (0, false, 4),
        n if n.starts_with("ENV") => (1, false, 4),
        // The matrix and the chain, each on its own band so a long list of
        // routes does not push a chorus onto the same line. An effect's card
        // is as wide as its count says.
        n if n.starts_with("Modulation") => (4, false, 0),
        _ => (5, false, 0),
    }
}

/// How big each control's knob is (`docs/flopsynth-next.md` §3.1, §3.5),
/// read off the card's name and the control's caption — the layer that
/// knows what a control *is* says how big it is drawn. One Large knob per
/// card: the one a player reaches for first — an oscillator's position,
/// start or brightness, a filter's cutoff, an envelope's decay, an LFO's
/// rate. The continuous controls Medium. The fine adjustments — pan, fine,
/// semis, width, blend, phase, a filter's key tracking, an envelope's
/// shapes — Small, which is half a cell. The sub and the noise are set
/// aside and small, so nothing on them is Large.
fn knob_sizes(name: &str, params: &[InstrumentParam]) -> Vec<KnobSize> {
    let large: &[&str] = match name {
        n if n.starts_with("OSC") => &["pos", "start", "bright"],
        n if n.starts_with("Filter") => &["cutoff"],
        n if n.starts_with("ENV") => &["decay"],
        n if n.starts_with("LFO") => &["rate"],
        _ => &[],
    };
    let small: &[&str] = match name {
        n if n.starts_with("OSC") || n == "SUB" => &[
            "pan", "fine", "semis", "width", "blend", "phase", "pos", "ring", "loop in",
            "loop out", "grain", "spray",
        ],
        "NOISE" => &["pan", "fine", "semis"],
        n if n.starts_with("Filter") => &["key trk", "character"],
        n if n.starts_with("ENV") => &["a shape", "d shape", "r shape"],
        n if n.starts_with("LFO") => &["delay", "fade", "phase", "smooth"],
        "Voice" => &["bend"],
        _ => &[],
    };
    params
        .iter()
        .map(|param| {
            let label = param.label.as_str();
            // The sub's position is small: it is set aside. An oscillator's
            // is the knob the card is about.
            if name != "SUB" && large.contains(&label) {
                KnobSize::Large
            } else if small.contains(&label) {
                KnobSize::Small
            } else {
                KnobSize::Medium
            }
        })
        .collect()
}

/// Which of the patch's layers a card of this name is, if it is an
/// oscillator at all.
///
/// Read off the heading, like [`shape_of`] and [`page_of`], and against the
/// **patch** rather than against a fixed list: a card is an oscillator's when
/// a synth layer's role is named after it, so a patch with a sampled layer
/// appended does not acquire a sixth oscillator card by accident.
fn oscillator_of(patch: &Patch, name: &str) -> Option<usize> {
    use fontelle_core::Source;
    use fontelle_core::flopsynth::layer_role;
    (0..patch.layers.len()).find(|index| {
        layer_role(*index).label() == name
            && matches!(
                patch.layers.get(*index).map(|l| &l.source),
                Some(Source::Synth(_))
            )
    })
}

/// The picture a card of this name gets, if any.
fn picture_for(name: &str, patch: &Patch, phases: &[f32]) -> FlopsynthPicture {
    use fontelle_core::Source;
    use fontelle_core::flopsynth::layer_role;

    // An oscillator's card is named after its role, so the role says which
    // layer it is.
    if let Some(index) = (0..patch.layers.len()).find(|i| layer_role(*i).label() == name) {
        let Some(Source::Synth(osc)) = patch.layers.get(index).map(|l| &l.source) else {
            return FlopsynthPicture::None;
        };
        return match osc.source {
            // Noise has no cycle to draw: a picture of one realisation of it
            // would be a different picture every frame, which is worse than
            // none.
            fontelle_dsp::SynthSource::Noise => FlopsynthPicture::None,
            fontelle_dsp::SynthSource::Table(id) => FlopsynthPicture::Wave {
                points: wave_points(id, osc.position),
                position: osc.position.clamp(0.0, 1.0),
            },
            // A sound somebody dropped in, drawn from the patch's own samples
            // — the same picture, of a table that came from a file rather
            // than from a recipe.
            fontelle_dsp::SynthSource::User(at) => {
                match patch.wavetables.get(at as usize) {
                    Some(table) => FlopsynthPicture::Wave {
                        points: user_wave_points(table, osc.position),
                        position: osc.position.clamp(0.0, 1.0),
                    },
                    // Named but not carried: nothing to draw, which is what
                    // that layer sounds like too.
                    None => FlopsynthPicture::None,
                }
            }
            // A recording: its shape, where the note starts in it, and the
            // loop when there is one — drawn from the zone that serves
            // middle C, which is the one most notes will play.
            fontelle_dsp::SynthSource::Sample(at) => match patch
                .samples
                .get(at as usize)
                .and_then(|sample| sample.zone_for(60).map(|zone| (sample, zone)))
            {
                Some((sample, zone)) => FlopsynthPicture::Sound {
                    peaks: sound_peaks(&zone.samples),
                    start: osc.position.clamp(0.0, 1.0),
                    // The region: the loop for the two modes that have one,
                    // and for the grain cloud the span either side of the
                    // start where its grains may land.
                    loop_region: match osc.sample.loop_mode {
                        fontelle_dsp::SampleLoop::Forward | fontelle_dsp::SampleLoop::PingPong => {
                            Some((
                                osc.sample.loop_start.clamp(0.0, 1.0),
                                osc.sample.loop_end.clamp(0.0, 1.0),
                            ))
                        }
                        fontelle_dsp::SampleLoop::Grains => {
                            let start = osc.position.clamp(0.0, 1.0);
                            let spray = osc.sample.spray.clamp(0.0, 1.0);
                            Some(((start - spray).max(0.0), (start + spray).min(1.0)))
                        }
                        fontelle_dsp::SampleLoop::Off | fontelle_dsp::SampleLoop::Reverse => None,
                    },
                    name: if sample.zones.len() > 1 {
                        format!("{} ({} notes)", sample.name, sample.zones.len())
                    } else {
                        sample.name.clone()
                    },
                },
                // Nothing dropped yet: an empty picture that says so, since
                // an oscillator switched to Sample from its chooser is
                // silent until it has one, and silence reads as a bug.
                None => FlopsynthPicture::Sound {
                    peaks: Vec::new(),
                    start: osc.position.clamp(0.0, 1.0),
                    loop_region: None,
                    name: "drop a sound here".to_string(),
                },
            },
            // A string: its partials, off the same function the voice rings
            // them from, so the stretch the picture shows is the stretch
            // the note has.
            fontelle_dsp::SynthSource::String => {
                let partials = fontelle_dsp::string_partials(
                    &osc.string,
                    osc.position,
                    fontelle_core::OSC_ROOT_HZ,
                    PARTIALS_TOP_HZ,
                );
                let loudest = partials.amp[..partials.count]
                    .iter()
                    .fold(0.0f32, |a, b| a.max(*b))
                    .max(1e-9);
                FlopsynthPicture::Partials {
                    bars: (0..partials.count)
                        .map(|n| (partials.ratio[n], partials.amp[n] / loudest))
                        .collect(),
                    harmonics: PARTIALS_DRAWN,
                }
            }
        };
    }

    if let Some(slot) = name
        .strip_prefix("Filter ")
        .and_then(|n| n.parse::<usize>().ok())
        .and_then(|n| n.checked_sub(1))
        && let Some(filter) = patch.filters.get(slot)
    {
        return FlopsynthPicture::Response {
            points: response_points(filter),
            cutoff: fontelle_core::patch_params::unlerp_log(
                filter.cutoff_hz,
                fontelle_core::patch_params::CUTOFF_MIN_HZ,
                fontelle_core::patch_params::CUTOFF_MAX_HZ,
            ),
            resonance: filter.resonance.clamp(0.0, 1.0),
        };
    }

    if let Some(index) = envelope_of(name)
        && let Some(env) = patch.envelopes.get(index)
    {
        return FlopsynthPicture::Envelope {
            attack: fontelle_core::patch_params::unlerp_stage(env.attack_s),
            decay: fontelle_core::patch_params::unlerp_stage(env.decay_s),
            sustain: env.sustain_level.clamp(0.0, 1.0),
            release: fontelle_core::patch_params::unlerp_stage(env.release_s),
        };
    }

    if let Some(index) = name
        .strip_prefix("LFO ")
        .and_then(|n| n.parse::<usize>().ok())
        .and_then(|n| n.checked_sub(1))
        && let Some(lfo) = patch.lfos.get(index)
    {
        return FlopsynthPicture::Lfo {
            points: (0..64)
                .map(|i| {
                    lfo.wave
                        .value((i as f32 / 64.0 + lfo.phase).rem_euclid(1.0))
                })
                .collect(),
            // Where the LFO **is**, which is a fact about the audio thread and
            // not about the patch — see `VoiceMeter`. Relative to the shape
            // drawn above, which starts at the patch's own phase offset.
            phase: (phases.get(index).copied().unwrap_or(0.0) - lfo.phase).rem_euclid(1.0),
        };
    }

    FlopsynthPicture::None
}

/// Which envelope a card called "ENV 3" or "ENV 1 · amp" is.
fn envelope_of(name: &str) -> Option<usize> {
    let rest = name.strip_prefix("ENV ")?;
    let number = rest.split_whitespace().next()?;
    number.parse::<usize>().ok()?.checked_sub(1)
}

/// One cycle of the frame an oscillator is currently reading.
///
/// **The frame it is reading**, not frame zero: moving the position knob moves
/// the picture, which is the whole reason the picture is worth drawing.
///
/// Off the RT thread, so building a table here is allowed — see
/// `fontelle_core::WavetableSet`, which is why it is *not* allowed in
/// `render`.
fn wave_points(id: fontelle_dsp::WavetableId, position: f32) -> Vec<f32> {
    let table = fontelle_dsp::wavetables().get(id);
    (0..WAVE_POINTS)
        .map(|i| {
            table.read(
                position.clamp(0.0, 1.0),
                i as f32 / WAVE_POINTS as f32,
                // Level 0: the picture wants every harmonic the table has,
                // not the band-limited copy a high note would read.
                0,
            )
        })
        .collect()
}

/// One cycle of the frame a **dropped sound's** table is reading.
///
/// Built here rather than cached, like `wave_points`: this runs when the
/// window redraws its cards, off the RT thread, and a table is a few
/// milliseconds of arithmetic.
fn user_wave_points(table: &fontelle_core::UserWavetable, position: f32) -> Vec<f32> {
    let built = fontelle_dsp::Wavetable::from_samples(&table.samples, table.frames);
    (0..WAVE_POINTS)
        .map(|i| {
            built.read(
                position.clamp(0.0, 1.0),
                i as f32 / WAVE_POINTS as f32,
                // Level 0: the picture wants every harmonic the table has.
                0,
            )
        })
        .collect()
}

/// The filter's magnitude response over the EQ's log axis, in decibels.
fn response_points(filter: &fontelle_core::FilterSlot) -> Vec<f32> {
    // The rate the response is drawn at. The picture is a fact about the
    // *filter*, not about the device that happens to be open, and 48 kHz is
    // what every other curve in this program is drawn at.
    const SR: f32 = 48_000.0;
    let settings = fontelle_dsp::SynthFilterSettings {
        model: filter.model,
        mode: filter.mode,
        slope: filter.slope,
        cutoff_hz: filter.cutoff_hz,
        resonance: filter.resonance,
        // The drive is left out on purpose: a `tanh` has no magnitude
        // response, because what it does depends on how loud the signal is.
        drive: 0.0,
        character: filter.character,
    };
    if !filter.enabled {
        // A switched-off filter is a wire, and the picture says so rather than
        // showing the curve it *would* have.
        return vec![0.0; RESPONSE_POINTS];
    }
    let (low, high) = (
        fontelle_core::patch_params::CUTOFF_MIN_HZ,
        fontelle_core::patch_params::CUTOFF_MAX_HZ,
    );
    (0..RESPONSE_POINTS)
        .map(|i| {
            let t = i as f32 / (RESPONSE_POINTS - 1) as f32;
            let hz = low * (high / low).powf(t);
            fontelle_dsp::response_db(&settings, hz, SR)
        })
        .collect()
}

/// What the window reads off the **audio thread** rather than off the patch
/// (§11, phase 6): how many voices are sounding, and where the newest voice's
/// LFOs are in their cycles. Facts the document does not have.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Heard {
    pub voices: usize,
    pub lfo_phases: Vec<f32>,
}

/// Everything Flopsynth's window shows, for this patch and this page.
///
/// **The cards are filtered here**, not in the window: which controls belong
/// to which page is a fact about what they are, and the layout draws what it
/// is given (§8.1 rule 10, one level up).
///
/// `bank` is the device's presets, for the Presets page (§8.6): the caller
/// hands them over on that page and an empty list on every other, because a
/// hundred and twenty-eight rows built for a page that is not showing is work
/// nobody sees.
pub fn describe(
    title: &str,
    patch: &Patch,
    gain_db: f32,
    pan: f32,
    page: FlopsynthPage,
    heard: Heard,
    bank: Vec<fontelle_ui::canvas::PresetChoice>,
) -> FlopsynthView {
    let Heard {
        voices,
        lfo_phases: phases,
    } = heard;
    let mut view = crate::instrument::describe_flopsynth(title, patch, gain_db, pan);
    // The channel's two knobs ride on the Voice card in this window: the
    // aside column stands beside both bands, and five cards in the second
    // did not fit the room it leaves (`fontelle-ui/tests/flopsynth.rs`,
    // `synth_page`). The panel's own list keeps its Channel group — the
    // addresses are the same wherever the knobs are drawn.
    if let Some(channel) = view.groups.iter().position(|g| g.name == "Channel") {
        let channel = view.groups.remove(channel);
        if let Some(voice) = view.groups.iter_mut().find(|g| g.name == "Voice") {
            voice.params.extend(channel.params);
        } else {
            view.groups.push(channel);
        }
    }
    let sources: Vec<String> = match page {
        FlopsynthPage::Modulation => fontelle_core::flopsynth::sources(patch)
            .into_iter()
            .map(|(_, label)| label)
            .collect(),
        _ => Vec::new(),
    };
    let routes = match page {
        FlopsynthPage::Modulation => route_rows(patch),
        _ => Vec::new(),
    };
    FlopsynthView {
        title: view.title,
        cards: view
            .groups
            .into_iter()
            // The matrix has a page of its own now, drawn as rows rather than
            // as a card of knobs: `describe_flopsynth`'s depth group is what
            // an automation lane addresses, and this window shows the routes.
            .filter(|group: &InstrumentGroup| !group.name.starts_with("Modulation"))
            .filter(|group| page_of(&group.name) == page)
            .map(|group: InstrumentGroup| {
                let (row, aside, columns) = shape_of(&group.name);
                FlopsynthCard {
                    row,
                    aside,
                    columns,
                    oscillator: oscillator_of(patch, &group.name),
                    // An effect slot is the one card that can be taken off
                    // the window — see `Session::remove_patch_effect`.
                    removable: group.name.starts_with("FX "),
                    picture: picture_for(&group.name, patch, &phases),
                    sizes: knob_sizes(&group.name, &group.params),
                    group,
                }
            })
            .collect(),
        page,
        sources,
        routes,
        voices,
        bank: match page {
            FlopsynthPage::Presets => bank,
            _ => Vec::new(),
        },
        browse: Default::default(),
        matrix_scroll: 0.0,
        scale: 1.0,
        fx_room: page == FlopsynthPage::Effects && patch.fx.len() < fontelle_core::MAX_PATCH_FX,
    }
}

/// The matrix, as the rows §8.4 draws.
fn route_rows(patch: &Patch) -> Vec<FlopsynthRoute> {
    let sources = fontelle_core::flopsynth::sources(patch);
    let destinations = fontelle_core::flopsynth::destinations(patch);
    patch
        .mod_matrix
        .routes
        .iter()
        .map(|route| FlopsynthRoute {
            // A source or destination this build does not list — one a patch
            // from another version carries — is named rather than hidden: a
            // route you cannot see is a route you cannot remove.
            source: sources
                .iter()
                .find(|(source, _)| *source == route.source)
                .map(|(_, label)| label.clone())
                .unwrap_or_else(|| format!("{:?}", route.source)),
            destination: destinations
                .iter()
                .find(|(dest, _)| *dest == route.destination)
                .map(|(_, label)| label.clone())
                .unwrap_or_else(|| format!("{:?}", route.destination)),
            depth: route.depth,
        })
        .collect()
}

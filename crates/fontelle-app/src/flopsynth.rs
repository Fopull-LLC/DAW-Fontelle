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
    FlopsynthCard, FlopsynthPage, FlopsynthPicture, FlopsynthRoute, FlopsynthView, INSPECTOR_ROW,
    InstrumentGroup, InstrumentParam, KnobSize,
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

/// Which of the five inks a source's ring wears (`docs/flopsynth-next.md`
/// §3.3) — the window may not see a `ModSource` (INVARIANT 4), so the
/// host names the family.
pub(crate) fn source_family(
    source: fontelle_core::ModSource,
) -> fontelle_ui::document::SourceFamily {
    use fontelle_core::ModSource;
    use fontelle_ui::document::SourceFamily;
    match source {
        ModSource::Envelope(_) => SourceFamily::Envelope,
        ModSource::Lfo(_) => SourceFamily::Lfo,
        ModSource::Macro(_) => SourceFamily::Macro,
        ModSource::Aftertouch | ModSource::ModWheel | ModSource::PitchBend => {
            SourceFamily::Performance
        }
        _ => SourceFamily::Note,
    }
}

/// Which page a card of this name belongs on (§8.3–§8.6), or `None` for a
/// card reached through the inspector alone.
///
/// Read off the heading, like [`shape_of`], and for the same reason: the number
/// of cards changes with the patch, so a card's page cannot be its index.
///
/// The split is by **what the controls are about**. The Synth page is making
/// the sound; the Matrix page is the routes; Effects is the chain after the
/// voice. What moves the sound — the envelopes and the LFOs — is on the strip
/// under every page, and edited in the inspector.
fn page_of(name: &str) -> Option<FlopsynthPage> {
    Some(match name {
        // The envelopes and the LFOs are on no page since 2026-09-18 (Ty's
        // call on `docs/flopsynth-next.md` §3.4–3.5): the Synth page is two
        // bands of consoles over the mod strip, the Matrix page is the
        // inspector and the table, and an envelope or an LFO is edited in
        // the inspector — click its badge on the strip.
        n if n.starts_with("LFO") || n.starts_with("ENV") => return None,
        n if n.starts_with("Modulation") => FlopsynthPage::Modulation,
        n if n.starts_with("FX ") => FlopsynthPage::Effects,
        _ => FlopsynthPage::Synth,
    })
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
/// read off the card's name and the control's **address** — the stable
/// name (INVARIANT 7), so a caption can change its word without moving a
/// knob. The layer that knows what a control *is* says how big it is
/// drawn. One Large knob per card: the one a player reaches for first — an
/// oscillator's position (its start on a recording, its brightness on a
/// string), a filter's cutoff, an envelope's decay, an LFO's rate. The
/// continuous controls Medium. The fine adjustments — pan, fine, semis,
/// width, blend, phase, the loop points, a filter's key tracking and
/// character, an envelope's curves, an LFO's delay, fade, phase and
/// smoothing, the bend range — Small, which is half a cell. The sub and
/// the noise are set aside and small, so nothing on them is Large and the
/// sub's position is Small too.
fn knob_sizes(name: &str, params: &[InstrumentParam]) -> Vec<KnobSize> {
    const LARGE: &[&str] = &["synth/position", "filter/cutoff", "env/decay", "lfo/rate"];
    const SMALL: &[&str] = &[
        "/pan",
        "/tune",
        "synth/semitones",
        "unison/width",
        "unison/blend",
        "synth/phase",
        "sample/loop_start",
        "sample/loop_end",
        "sample/grain",
        "sample/spray",
        "string/decay",
        "filter/key_track",
        "/character",
        "attack_shape",
        "decay_shape",
        "release_shape",
        "lfo/delay",
        "lfo/fade",
        "lfo/phase",
        "lfo/smooth",
        "voice/bend_range",
        "synth/quality",
        "/oversampling",
    ];
    let aside = name == "SUB" || name == "NOISE";
    params
        .iter()
        .map(|param| {
            let address = param.address.as_str();
            // The tails are matched after the index, so "filter/key_track"
            // reads "filter[0]/key_track".
            let tail: String = address
                .chars()
                .filter(|c| !c.is_ascii_digit() && *c != '[' && *c != ']')
                .collect();
            if !aside && LARGE.iter().any(|large| tail.ends_with(large)) {
                KnobSize::Large
            } else if SMALL.iter().any(|small| tail.ends_with(small))
                || (aside && tail.ends_with("synth/position"))
            {
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
/// An effect's picture (`docs/flopsynth-next.md` §3.6), from the numbers
/// the effect plays: the delay's taps, the reverb's tail, the distortion's
/// transfer curve through the effect's own `curve_at`, the EQ's response
/// with a dot per band, the compressor's gain curve. The kinds with no
/// shape to draw — a chorus, the utility — have none.
fn effect_picture(config: &fontelle_types::EffectConfig) -> FlopsynthPicture {
    use fontelle_types::EffectConfig;
    const STEPS: usize = 96;
    match config {
        EffectConfig::Delay(delay) => {
            // The taps across a two-second window, each `feedback` of the
            // one before, until they are too quiet to draw.
            let time_s = if delay.sync {
                delay.division.beats() * 0.5
            } else {
                delay.time_ms / 1000.0
            }
            .max(0.005);
            let window = 2.0f32;
            let feedback = delay.feedback.clamp(0.0, 0.98);
            let mut marks = Vec::new();
            let mut level = 1.0f32;
            let mut at = time_s;
            while at <= window && level > 0.04 && marks.len() < 32 {
                marks.push((at / window, level));
                level *= feedback;
                at += time_s;
            }
            FlopsynthPicture::Curve {
                points: Vec::new(),
                marks,
                midline: false,
            }
        }
        EffectConfig::Reverb(reverb) => {
            // RT60 across a window a little longer than the tail.
            let decay = reverb.decay_s.max(0.05);
            let window = decay * 1.2;
            let points = (0..STEPS)
                .map(|i| {
                    let t = window * i as f32 / (STEPS - 1) as f32;
                    10f32.powf(-3.0 * t / decay)
                })
                .collect();
            FlopsynthPicture::Curve {
                points,
                marks: Vec::new(),
                midline: false,
            }
        }
        EffectConfig::Distortion(dist) => {
            let drive = 10f32.powf(dist.drive_db.clamp(0.0, 60.0) / 20.0);
            let points = (0..STEPS)
                .map(|i| {
                    let x = i as f32 / (STEPS - 1) as f32 * 2.0 - 1.0;
                    let y = fontelle_fx::distortion_curve_at(
                        x * drive,
                        dist.curve,
                        dist.shape.clamp(0.0, 1.0),
                        dist.bias.clamp(-1.0, 1.0),
                    );
                    (y.clamp(-1.0, 1.0) + 1.0) / 2.0
                })
                .collect();
            FlopsynthPicture::Curve {
                points,
                marks: Vec::new(),
                midline: true,
            }
        }
        EffectConfig::Bitcrush(crush) => {
            let steps = 2f32.powf(crush.bits.clamp(1.0, 16.0));
            let points = (0..STEPS)
                .map(|i| {
                    let x = i as f32 / (STEPS - 1) as f32 * 2.0 - 1.0;
                    let y = (x * steps / 2.0).round() / (steps / 2.0);
                    (y.clamp(-1.0, 1.0) + 1.0) / 2.0
                })
                .collect();
            FlopsynthPicture::Curve {
                points,
                marks: Vec::new(),
                midline: true,
            }
        }
        EffectConfig::Eq(eq) => {
            // ±24 dB across 20 Hz to 20 kHz, log; a dot per band that is on.
            let (lo, hi, span_db) = (20f32, 20_000f32, 24f32);
            let hz_at = |t: f32| lo * (hi / lo).powf(t);
            let points = (0..STEPS)
                .map(|i| {
                    let hz = hz_at(i as f32 / (STEPS - 1) as f32);
                    let db = eq.response_db(hz, fontelle_types::BandChannel::Stereo);
                    ((db / span_db).clamp(-1.0, 1.0) + 1.0) / 2.0
                })
                .collect();
            let marks = eq
                .bands
                .iter()
                .filter(|band| band.enabled)
                .map(|band| {
                    let t = (band.freq_hz.max(lo) / lo).ln() / (hi / lo).ln();
                    let db = eq.response_db(band.freq_hz, fontelle_types::BandChannel::Stereo);
                    (
                        t.clamp(0.0, 1.0),
                        ((db / span_db).clamp(-1.0, 1.0) + 1.0) / 2.0,
                    )
                })
                .collect();
            FlopsynthPicture::Curve {
                points,
                marks,
                midline: true,
            }
        }
        EffectConfig::Compressor(comp) => {
            // Output against input over −60..0 dB: the compressor's own
            // transfer, its knee included.
            let ratio = comp.ratio.max(1.0);
            let knee = comp.knee_db.max(0.0);
            let points = (0..STEPS)
                .map(|i| {
                    let level = -60.0 + 60.0 * i as f32 / (STEPS - 1) as f32;
                    let over = level - comp.threshold_db;
                    let reduction = if knee > 0.0 && over > -knee / 2.0 && over < knee / 2.0 {
                        let x = over + knee / 2.0;
                        -(1.0 / ratio - 1.0).abs() * x * x / (2.0 * knee)
                    } else if over > 0.0 {
                        -over * (1.0 - 1.0 / ratio)
                    } else {
                        0.0
                    };
                    ((level + reduction + comp.makeup_db + 60.0) / 60.0).clamp(0.0, 1.0)
                })
                .collect();
            FlopsynthPicture::Curve {
                points,
                marks: Vec::new(),
                midline: false,
            }
        }
        EffectConfig::Limiter(limiter) => {
            let points = (0..STEPS)
                .map(|i| {
                    let level = -60.0 + 60.0 * i as f32 / (STEPS - 1) as f32;
                    ((level.min(limiter.ceiling_db) + 60.0) / 60.0).clamp(0.0, 1.0)
                })
                .collect();
            FlopsynthPicture::Curve {
                points,
                marks: Vec::new(),
                midline: false,
            }
        }
        _ => FlopsynthPicture::None,
    }
}

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
        use fontelle_core::patch_params::unlerp_stage;
        // The loop as stage numbers, in `EnvStage`'s order — the picture
        // is the window's and knows no `EnvStage`.
        let stage = |stage: fontelle_dsp::EnvStage| -> u8 {
            fontelle_dsp::EnvStage::ALL
                .iter()
                .position(|s| *s == stage)
                .unwrap_or(0) as u8
        };
        return FlopsynthPicture::Envelope(fontelle_ui::canvas::EnvelopePicture {
            delay: unlerp_stage(env.delay_s),
            attack: unlerp_stage(env.attack_s),
            hold: unlerp_stage(env.hold_s),
            decay: unlerp_stage(env.decay_s),
            sustain: env.sustain_level.clamp(0.0, 1.0),
            release: unlerp_stage(env.release_s),
            attack_shape: env.attack_shape,
            decay_shape: env.decay_shape,
            release_shape: env.release_shape,
            loop_stages: env.loop_stages.map(|(from, to)| (stage(from), stage(to))),
        });
    }

    if let Some(index) = name
        .strip_prefix("LFO ")
        .and_then(|n| n.parse::<usize>().ok())
        .and_then(|n| n.checked_sub(1))
        && let Some(lfo) = patch.lfos.get(index)
    {
        // A drawn shape is edited on its picture (§3.4); a wave is read.
        if let Some(shape) = &lfo.shape {
            return FlopsynthPicture::LfoShape {
                shape: shape.clone(),
                phase: (phases.get(index).copied().unwrap_or(0.0) - lfo.phase).rem_euclid(1.0),
            };
        }
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

    // An effect slot's card (§3.6): the effect's own picture.
    if let Some(index) = name
        .strip_prefix("FX ")
        .and_then(|rest| rest.split(' ').next())
        .and_then(|n| n.parse::<usize>().ok())
        .and_then(|n| n.checked_sub(1))
        && let Some(slot) = patch.fx.get(index)
    {
        return effect_picture(&slot.config);
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
        // Nor the oversampling, which changes what folds and not the curve.
        oversampling: fontelle_dsp::Oversampling::Off,
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
    /// What each effect slot put out this block (§3.6), for the rack's
    /// meters; short or empty reads as silence.
    pub fx_levels: Vec<f32>,
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
/// nobody sees. `inspector` is the source open in the strip's inspector
/// (§3.4), whose card comes along marked for the drawer.
// Eight, one more than clippy's seven, for `tune::describe`'s reason: every
// one is a different source, and a struct whose only job is to be unpacked
// one line later would be a second thing to keep in step.
#[allow(clippy::too_many_arguments)]
pub fn describe(
    title: &str,
    patch: &Patch,
    gain_db: f32,
    pan: f32,
    page: FlopsynthPage,
    heard: Heard,
    bank: Vec<fontelle_ui::canvas::PresetChoice>,
    showing: fontelle_ui::canvas::FlopsynthShowing,
) -> FlopsynthView {
    let inspector = showing.inspector;
    let Heard {
        voices,
        lfo_phases: phases,
        fx_levels,
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
    // The captions: the words this window draws over its controls
    // (`crate::captions`), in capitals. The macros keep their names; an
    // effect card's are its effect's own, set in capitals like the rest.
    for group in &mut view.groups {
        if group.name == "Macros" {
            continue;
        }
        for param in &mut group.params {
            param.label = crate::captions::captioned(&param.label);
        }
    }
    // The strip's sources, on every page (§3.4), each with its picture.
    let source_list = fontelle_core::flopsynth::sources(patch);
    let sources: Vec<String> = source_list.iter().map(|(_, label)| label.clone()).collect();
    let source_shapes: Vec<Vec<f32>> = source_list
        .iter()
        .map(|(source, _)| source_shape(patch, *source))
        .collect();
    let source_families = source_list
        .iter()
        .map(|(source, _)| source_family(*source))
        .collect();
    let (routes, destinations, curves) = match page {
        FlopsynthPage::Modulation => (
            route_rows(patch),
            destination_labels(patch),
            CURVES.iter().map(|(_, label)| label.to_string()).collect(),
        ),
        _ => (Vec::new(), Vec::new(), Vec::new()),
    };
    // The inspected source's card (§3.4): the group that edits it, marked
    // for the drawer — an envelope's or an LFO's own card, the macros'
    // card for a macro. A source with nothing to edit (the velocity) opens
    // no drawer.
    let inspected_group: Option<String> = inspector
        .and_then(|index| source_list.get(index))
        .and_then(|(source, _)| match source {
            fontelle_core::ModSource::Envelope(i) => Some(format!("ENV {}", i + 1)),
            fontelle_core::ModSource::Lfo(i) => Some(format!("LFO {}", i + 1)),
            fontelle_core::ModSource::Macro(_) => Some("Macros".to_string()),
            _ => None,
        });
    let inspected_card = inspected_group.as_ref().and_then(|name| {
        view.groups
            .iter()
            .find(|group| group.name == *name || group.name.starts_with(&format!("{name} \u{b7}")))
            .map(|group| FlopsynthCard {
                row: INSPECTOR_ROW,
                aside: false,
                // The layout widens the drawer's card to the drawer; this
                // is only what it would be with nothing to widen it to.
                columns: 8,
                oscillator: None,
                removable: false,
                picture: picture_for(&group.name, patch, &phases),
                sizes: knob_sizes(&group.name, &group.params),
                group: group.clone(),
            })
    });
    let mut cards: Vec<FlopsynthCard> = view
        .groups
        .into_iter()
        // The matrix has a page of its own now, drawn as rows rather than
        // as a card of knobs: `describe_flopsynth`'s depth group is what
        // an automation lane addresses, and this window shows the routes.
        .filter(|group: &InstrumentGroup| !group.name.starts_with("Modulation"))
        .filter(|group| page_of(&group.name) == Some(page))
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
        .collect();
    let inspector = inspected_card.as_ref().and(inspector);
    cards.extend(inspected_card);

    // The Effects page (§3.6): the rack, and the selected slot's card
    // alone beside it — the first when none is chosen, the last for a
    // choice past the end.
    let (rack, fx_slot) = if page == FlopsynthPage::Effects {
        let rack: Vec<fontelle_ui::canvas::FxRackSlot> = patch
            .fx
            .iter()
            .enumerate()
            .map(|(index, slot)| fontelle_ui::canvas::FxRackSlot {
                name: format!("FX {} \u{b7} {}", index + 1, slot.config.kind().label()),
                enabled: slot.enabled,
                mix: slot.config.mix().clamp(0.0, 1.0),
                level: fx_levels.get(index).copied().unwrap_or(0.0),
            })
            .collect();
        let fx_slot = if rack.is_empty() {
            None
        } else {
            Some(showing.fx_slot.unwrap_or(0).min(rack.len() - 1))
        };
        if let Some(slot) = fx_slot {
            let wanted = format!("FX {} \u{b7}", slot + 1);
            cards.retain(|card| !card.removable || card.group.name.starts_with(&wanted));
        }
        (rack, fx_slot)
    } else {
        (Vec::new(), None)
    };
    FlopsynthView {
        title: view.title,
        cards,
        page,
        sources,
        source_shapes,
        source_families,
        destinations,
        curves,
        inspector,
        routes,
        voices,
        bank: match page {
            FlopsynthPage::Presets => bank,
            _ => Vec::new(),
        },
        browse: Default::default(),
        matrix_scroll: 0.0,
        scale: 1.0,
        thumbnails: chooser_thumbnails(patch, page),
        fx_room: page == FlopsynthPage::Effects && patch.fx.len() < fontelle_core::MAX_PATCH_FX,
        rack,
        fx_slot,
    }
}

/// How many points a chooser's thumbnail is drawn from: a 32-pixel picture.
const THUMB_POINTS: usize = 32;

/// A source's picture for its badge on the strip (§3.4): an envelope's
/// curve — up over the attack, down to the sustain over the decay, held,
/// then the release — an LFO's cycle, a macro's value as a level; nothing
/// for a source with no shape of its own.
fn source_shape(patch: &Patch, source: fontelle_core::ModSource) -> Vec<f32> {
    use fontelle_core::ModSource;
    use fontelle_core::patch_params::unlerp_stage;
    match source {
        ModSource::Envelope(i) => {
            let Some(env) = patch.envelopes.get(usize::from(i)) else {
                return Vec::new();
            };
            // The stage lengths as shares of the picture, the way the card's
            // own picture draws them: each a share of the dial rather than
            // of a second, so a short envelope is still readable.
            let attack = unlerp_stage(env.attack_s).max(0.04);
            let decay = unlerp_stage(env.decay_s).max(0.04);
            let release = unlerp_stage(env.release_s).max(0.04);
            let hold = 0.25;
            let total = attack + decay + hold + release;
            let sustain = env.sustain_level.clamp(0.0, 1.0);
            (0..THUMB_POINTS)
                .map(|i| {
                    let t = i as f32 / (THUMB_POINTS - 1) as f32 * total;
                    let level = if t < attack {
                        t / attack
                    } else if t < attack + decay {
                        1.0 - (1.0 - sustain) * ((t - attack) / decay)
                    } else if t < attack + decay + hold {
                        sustain
                    } else {
                        sustain * (1.0 - ((t - attack - decay - hold) / release).min(1.0))
                    };
                    level * 2.0 - 1.0
                })
                .collect()
        }
        // The drawn shape when there is one, else the wave: the badge
        // shows what plays.
        ModSource::Lfo(i) => match patch.lfos.get(usize::from(i)) {
            Some(lfo) => (0..THUMB_POINTS)
                .map(|n| {
                    let phase = n as f32 / THUMB_POINTS as f32;
                    lfo.shape
                        .as_ref()
                        .map_or_else(|| lfo.wave.value(phase), |shape| shape.value(phase))
                })
                .collect(),
            None => Vec::new(),
        },
        ModSource::Macro(i) => match patch.macros.get(usize::from(i)) {
            Some(m) => vec![m.value * 2.0 - 1.0; 2],
            None => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// The pictures for the choosers whose options are shapes (§3.3): every
/// table oscillator's table chooser gets the bank's forty first frames, and
/// every LFO's wave chooser its six cycles. The bank's pictures are built
/// **once** — the first asks `wavetables()` to build every table, a few
/// milliseconds each — and shared after, so a revision hands out the same
/// `Arc` rather than a copy.
fn chooser_thumbnails(
    patch: &Patch,
    page: FlopsynthPage,
) -> Vec<(fontelle_types::ParamAddress, std::sync::Arc<[Vec<f32>]>)> {
    use fontelle_core::Source;
    use fontelle_dsp::{SynthSource, WavetableId};
    use fontelle_types::LfoWave;
    static TABLES: std::sync::OnceLock<std::sync::Arc<[Vec<f32>]>> = std::sync::OnceLock::new();
    static WAVES: std::sync::OnceLock<std::sync::Arc<[Vec<f32>]>> = std::sync::OnceLock::new();
    let mut out = Vec::new();
    match page {
        FlopsynthPage::Synth => {
            for (index, layer) in patch.layers.iter().enumerate() {
                if let Source::Synth(osc) = &layer.source
                    && matches!(osc.source, SynthSource::Table(_))
                {
                    let tables = TABLES.get_or_init(|| {
                        WavetableId::ALL
                            .iter()
                            .map(|id| {
                                let table = fontelle_dsp::wavetables().get(*id);
                                // The frame with the most in it of the
                                // first, the middle and the last: a morphing
                                // table's first frame can be next to nothing
                                // (SubSaw's is), and a picture of nothing
                                // says nothing about the table.
                                [0.0f32, 0.5, 1.0]
                                    .into_iter()
                                    .map(|position| {
                                        (0..THUMB_POINTS)
                                            .map(|i| {
                                                table.read(
                                                    position,
                                                    i as f32 / THUMB_POINTS as f32,
                                                    0,
                                                )
                                            })
                                            .collect::<Vec<f32>>()
                                    })
                                    .max_by(|a, b| {
                                        let peak = |s: &Vec<f32>| {
                                            s.iter().fold(0.0f32, |m, v| m.max(v.abs()))
                                        };
                                        peak(a).total_cmp(&peak(b))
                                    })
                                    .map(|mut shape| {
                                        // A picture of the shape, not of the
                                        // level: a table trimmed quiet
                                        // (FormantSweep sits at a fifth of
                                        // full scale) is drawn full height.
                                        let peak = shape.iter().fold(0.0f32, |m, v| m.max(v.abs()));
                                        if peak > 1e-6 {
                                            for s in &mut shape {
                                                *s /= peak;
                                            }
                                        }
                                        shape
                                    })
                                    .unwrap_or_default()
                            })
                            .collect::<Vec<Vec<f32>>>()
                            .into()
                    });
                    out.push((
                        fontelle_types::ParamAddress::new(format!(
                            "patch/layer[{index}]/synth/table"
                        )),
                        tables.clone(),
                    ));
                }
            }
        }
        FlopsynthPage::Modulation => {
            let waves = WAVES.get_or_init(|| {
                LfoWave::ALL
                    .iter()
                    .map(|wave| {
                        (0..THUMB_POINTS)
                            .map(|i| wave.value(i as f32 / THUMB_POINTS as f32))
                            .collect()
                    })
                    .collect::<Vec<Vec<f32>>>()
                    .into()
            });
            for index in 0..patch.lfos.len() {
                out.push((
                    fontelle_types::ParamAddress::new(format!("patch/lfo[{index}]/wave")),
                    waves.clone(),
                ));
            }
        }
        _ => {}
    }
    out
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
            via: route.via.and_then(|via| {
                sources
                    .iter()
                    .find(|(source, _)| *source == via)
                    .map(|(_, label)| label.clone())
            }),
            curve: curve_label(route.curve).to_string(),
            invert: route.invert,
            bypass: route.bypass,
        })
        .collect()
}

/// The curves a route can have, in the table's chooser order, and what
/// each is called. One name for the stepped curve whatever its step count:
/// the count is not in the table, and twelve is the count that makes a
/// pitch route play semitones.
pub const CURVES: [(fontelle_core::Curve, &str); 5] = [
    (fontelle_core::Curve::Linear, "Linear"),
    (fontelle_core::Curve::Exponential, "Exponential"),
    (fontelle_core::Curve::Logarithmic, "Logarithmic"),
    (fontelle_core::Curve::SCurve, "S-curve"),
    (fontelle_core::Curve::Quantised { steps: 12 }, "Stepped"),
];

pub fn curve_label(curve: fontelle_core::Curve) -> &'static str {
    match curve {
        fontelle_core::Curve::Quantised { .. } => "Stepped",
        other => CURVES
            .iter()
            .find(|(c, _)| *c == other)
            .map_or("Linear", |(_, label)| label),
    }
}

/// Every destination this patch's routes could reach, by label — the
/// table's destination chooser (§3.4).
pub fn destination_labels(patch: &Patch) -> Vec<String> {
    fontelle_core::flopsynth::destinations(patch)
        .into_iter()
        .map(|(_, label)| label)
        .collect()
}

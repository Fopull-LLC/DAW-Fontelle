//! The audio clip editor (TDD §15.1).
//!
//! Reported from using the window:
//!
//! > *"double clicking on an audio clip should open a menu that lets me make
//! > changes to that audio like basic changes that would be widely general
//! > useful across all audio editing so for example changing the boost or the
//! > cutoff or resonance of the sound or even changing a fade in or fade out or
//! > stuff like that. just need all the features we would expect or need for a
//! > full audio suite."*
//!
//! # The shape, and why it is this one
//!
//! **Rows of a name and a control**, under headings that say what each group is
//! for. Which control a row gets is decided by what the row *is* (see
//! [`AudioControl`]) rather than by what was easiest to draw:
//!
//! - a **slider** for anything continuous — the boost, the pitch, a fade;
//! - a **switch** for anything that is on or off;
//! - a **drop-down** for anything that is one of a list.
//!
//! This is the second shape. The first was a click that stepped every row
//! forward and a Ctrl+click that stepped it back, which is the settings tab's
//! shape and was reported as the wrong one here:
//!
//! > *"right now a lot of options that could be knobs or sliders or dropdowns
//! > for some reason are instead shown as buttons you click to toggle through a
//! > list of options in order iteratively. this is really annoying please
//! > ensure we have cleaner and more polshed ux. for example, this is happening
//! > in the audio clip editing panel right now the pitch changing should be a
//! > knob but instead its a button i click to iteratively go through a list of
//! > pre made values."*
//!
//! Stepping survives as the **wheel**, which is what a wheel over a control
//! should do anyway and is how a value is nudged by exactly one of whatever it
//! is measured in — see [`nudge_audio_row`]. What went is stepping being the
//! *only* way in: a pitch you have to click twelve times to move an octave is
//! not a pitch control.
//!
//! A slider rather than a knob because the panel is a **list of rows**: a knob
//! in a 22-pixel row is a smudge with no readable travel, and a horizontal
//! track has the row's whole width to spend on precision. The number stays on
//! the row, over the track, because a control whose value you cannot read is a
//! control you cannot set.
//!
//! Across the top is the clip's **own waveform**, because a fade you cannot see
//! is a fade you are aiming blind, and a list of numbers with nothing to look at
//! is a list of numbers.
//!
//! # What is on it, and what is deliberately not
//!
//! Everything §15.1 names except the two it defers: `time_lock` needs the
//! stretch engine (§3.3, a v2 feature), and the inline three-band EQ is left out
//! because the clip already carries a whole multimode filter with drive — a
//! third tone control on one clip is a panel nobody can read.
//!
//! Everything is pure: the panel is geometry, the properties are
//! [`AudioClipData`], and *"what does a click on this row do"* is a function
//! from one of those to another. So the whole editor is checkable without a
//! window (§2.5 of `docs/first-usable-plan.md`).

use fontelle_types::{
    AudioClipData, ClipLoopMode, ClipStretch, FadeCurve, FilterShape, MAX_CLIP_GAIN_DB,
    MAX_CLIP_SPEED, MAX_FILTER_HZ, MIN_CLIP_GAIN_DB, MIN_CLIP_SPEED, MIN_FILTER_HZ, Sample,
};

use crate::layout::Rect;
use crate::theme::Metrics;

/// One row of the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioField {
    /// A section title. Nothing to set, and a click does nothing — it is what
    /// makes "Cutoff" read as the *filter's* rather than as something the fade
    /// does.
    Heading(&'static str),
    /// Which mixer track the clip plays through — `AudioClipData::mixer_track`.
    ///
    /// *"soundclips dont have options right now for selecting their mixer
    /// track."* The field has always been there; a take is given one so that
    /// recording through a strip means something. What was missing was a row.
    Route,
    /// *"the boost"*, in decibels.
    Gain,
    Pan,
    Normalize,
    /// Whether the clip follows the song's tempo — see
    /// [`fontelle_types::ClipStretch`]. First in the Time group, because it
    /// decides what the two below it are *relative to*.
    Stretch,
    Pitch,
    Speed,
    Reverse,
    Loop,
    FadeIn,
    FadeInCurve,
    FadeOut,
    FadeOutCurve,
    FilterShape,
    /// *"the cutoff"*.
    Cutoff,
    /// *"or resonance"*.
    Resonance,
    Drive,
}

/// Every row, in the order the editor draws them.
///
/// Grouped by what you are doing rather than by what kind of control it is:
/// how loud, how fast, how it starts and ends, and what colour it is.
pub const AUDIO_ROWS: [AudioField; 22] = [
    AudioField::Heading("Track"),
    AudioField::Route,
    AudioField::Heading("Level"),
    AudioField::Gain,
    AudioField::Pan,
    AudioField::Normalize,
    AudioField::Heading("Time"),
    AudioField::Stretch,
    AudioField::Pitch,
    AudioField::Speed,
    AudioField::Reverse,
    AudioField::Loop,
    AudioField::Heading("Fades"),
    AudioField::FadeIn,
    AudioField::FadeInCurve,
    AudioField::FadeOut,
    AudioField::FadeOutCurve,
    AudioField::Heading("Filter"),
    AudioField::FilterShape,
    AudioField::Cutoff,
    AudioField::Resonance,
    AudioField::Drive,
];

/// What kind of control a row gets, and so what a press on it means.
///
/// Asked of the field rather than listed at each call site, so the hit-test,
/// the drawing and the gesture cannot disagree about what a row is — the way
/// they did when everything was a step and the panel had one gesture for
/// eighteen different kinds of value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioControl {
    /// A heading. Nothing to set.
    None,
    /// A continuous value along a track, set by where you press and dragged
    /// from there — see [`audio_row_fraction`] and [`set_audio_row_fraction`].
    Slider,
    /// Off or on, clicked.
    Switch,
    /// One of a list, chosen from a drop-down — see [`audio_row_choices`].
    Choice,
}

/// Which control `field` gets.
pub fn audio_row_control(field: AudioField) -> AudioControl {
    match field {
        AudioField::Heading(_) => AudioControl::None,
        AudioField::Normalize | AudioField::Reverse => AudioControl::Switch,
        AudioField::Route
        | AudioField::Stretch
        | AudioField::Loop
        | AudioField::FadeInCurve
        | AudioField::FadeOutCurve
        | AudioField::FilterShape => AudioControl::Choice,
        AudioField::Gain
        | AudioField::Pan
        | AudioField::Pitch
        | AudioField::Speed
        | AudioField::FadeIn
        | AudioField::FadeOut
        | AudioField::Cutoff
        | AudioField::Resonance
        | AudioField::Drive => AudioControl::Slider,
    }
}

/// What a [`AudioControl::Choice`] row's drop-down lists, in order.
///
/// Empty for every other row, and empty for [`AudioField::Route`] — the one
/// choice whose entries are not the clip's to know. Which mixer tracks exist
/// is the document's, and this crate may not see one (INVARIANT 2), so the
/// window supplies that list and [`nudge_route`] and
/// [`choose_audio_route`] take it.
pub fn audio_row_choices(field: AudioField) -> Vec<&'static str> {
    match field {
        AudioField::Stretch => ClipStretch::ALL.iter().map(|c| c.label()).collect(),
        AudioField::Loop => ClipLoopMode::ALL.iter().map(|c| c.label()).collect(),
        AudioField::FadeInCurve | AudioField::FadeOutCurve => {
            FadeCurve::ALL.iter().map(|c| c.label()).collect()
        }
        AudioField::FilterShape => FilterShape::ALL.iter().map(|c| c.label()).collect(),
        _ => Vec::new(),
    }
}

/// Which entry of [`audio_row_choices`] the clip is on, for the tick beside
/// the open drop-down's rows.
pub fn audio_row_chosen(clip: &AudioClipData, field: AudioField) -> Option<usize> {
    let at = |all: &[&str], label: &str| all.iter().position(|c| *c == label);
    let choices = audio_row_choices(field);
    match field {
        AudioField::Stretch => at(&choices, clip.stretch.label()),
        AudioField::Loop => at(&choices, clip.loop_mode.label()),
        AudioField::FadeInCurve => at(&choices, clip.fade_in.curve.label()),
        AudioField::FadeOutCurve => at(&choices, clip.fade_out.curve.label()),
        AudioField::FilterShape => at(&choices, clip.filter.shape.label()),
        _ => None,
    }
}

/// Picks the `index`th entry of [`audio_row_choices`].
///
/// Out of range does nothing rather than wrapping: a drop-down hands back a
/// row it drew, so an index it did not draw is a bug somewhere else and not a
/// reason to change the clip.
pub fn choose_audio_row(clip: &mut AudioClipData, field: AudioField, index: usize) {
    match field {
        AudioField::Stretch => {
            if let Some(v) = ClipStretch::ALL.get(index) {
                clip.stretch = *v;
            }
        }
        AudioField::Loop => {
            if let Some(v) = ClipLoopMode::ALL.get(index) {
                clip.loop_mode = *v;
            }
        }
        AudioField::FadeInCurve => {
            if let Some(v) = FadeCurve::ALL.get(index) {
                clip.fade_in.curve = *v;
            }
        }
        AudioField::FadeOutCurve => {
            if let Some(v) = FadeCurve::ALL.get(index) {
                clip.fade_out.curve = *v;
            }
        }
        AudioField::FilterShape => {
            if let Some(v) = FilterShape::ALL.get(index) {
                clip.filter.shape = *v;
            }
        }
        _ => {}
    }
}

/// Points `clip` at the `index`th entry of `tracks` — [`nudge_route`]'s
/// drop-down half. Master is `tracks[0]`, as `None`.
pub fn choose_audio_route(
    clip: &mut AudioClipData,
    index: usize,
    tracks: &[Option<fontelle_types::MixerTrackId>],
) {
    if let Some(track) = tracks.get(index) {
        clip.mixer_track = *track;
    }
}

/// Flips a [`AudioControl::Switch`] row.
pub fn toggle_audio_row(clip: &mut AudioClipData, field: AudioField) {
    match field {
        AudioField::Normalize => clip.normalize = !clip.normalize,
        AudioField::Reverse => clip.reverse = !clip.reverse,
        _ => {}
    }
}

/// Whether a [`AudioControl::Switch`] row is on.
pub fn audio_row_is_on(clip: &AudioClipData, field: AudioField) -> bool {
    match field {
        AudioField::Normalize => clip.normalize,
        AudioField::Reverse => clip.reverse,
        _ => false,
    }
}

/// Where a [`AudioControl::Slider`] row's value sits along its track, 0..=1.
///
/// `None` for every row that is not a slider, which is what makes "is this a
/// slider" one question rather than two lists to keep in step.
///
/// **Not always linear.** A speed and a cutoff are *ratios* — ten per cent of
/// half speed and ten per cent of double speed are the same musical distance
/// and different numbers — so those two run logarithmically, exactly as their
/// stepping already did. A fade runs on a square, because the useful lengths
/// span three orders of magnitude and a linear track would spend nine tenths
/// of itself on lengths nobody asks for.
pub fn audio_row_fraction(clip: &AudioClipData, field: AudioField) -> Option<f32> {
    let linear = |value: f32, min: f32, max: f32| ((value - min) / (max - min)).clamp(0.0, 1.0);
    let log = |value: f64, min: f64, max: f64| {
        ((value.max(f64::MIN_POSITIVE) / min).ln() / (max / min).ln()).clamp(0.0, 1.0) as f32
    };
    Some(match field {
        AudioField::Gain => linear(clip.gain_db, MIN_CLIP_GAIN_DB, MAX_CLIP_GAIN_DB),
        AudioField::Pan => linear(clip.pan, -1.0, 1.0),
        AudioField::Pitch => linear(clip.pitch_semitones, MIN_CLIP_PITCH, MAX_CLIP_PITCH),
        AudioField::Speed => log(clip.speed, MIN_CLIP_SPEED, MAX_CLIP_SPEED),
        AudioField::FadeIn | AudioField::FadeOut => {
            let longest = clip.source_frames();
            if longest <= 0 {
                return Some(0.0);
            }
            let fade = if field == AudioField::FadeIn {
                &clip.fade_in
            } else {
                &clip.fade_out
            };
            (fade.frames as f32 / longest as f32).clamp(0.0, 1.0).sqrt()
        }
        AudioField::Cutoff => log(
            f64::from(clip.filter.cutoff_hz),
            f64::from(MIN_FILTER_HZ),
            f64::from(MAX_FILTER_HZ),
        ),
        AudioField::Resonance => clip.filter.resonance.clamp(0.0, 1.0),
        AudioField::Drive => clip.filter.drive.clamp(0.0, 1.0),
        _ => return None,
    })
}

/// The other direction: writes the value `t` along the track stands for.
///
/// **Detented**, which is the difference between a slider you can use and one
/// you can only get near: unity gain, dead centre and normal speed are the
/// values a mix is actually built out of, and a track a few hundred points
/// long cannot land on them by hand. Pitch goes further and quantises to whole
/// semitones — see below.
///
/// Does nothing to a row that is not a slider.
pub fn set_audio_row_fraction(clip: &mut AudioClipData, field: AudioField, t: f32) {
    let t = t.clamp(0.0, 1.0);
    let linear = |min: f32, max: f32| min + t * (max - min);
    let log = |min: f64, max: f64| min * (max / min).powf(f64::from(t));
    match field {
        AudioField::Gain => {
            clip.gain_db = detent(linear(MIN_CLIP_GAIN_DB, MAX_CLIP_GAIN_DB), 0.0, 0.75);
        }
        AudioField::Pan => clip.pan = detent(linear(-1.0, 1.0), 0.0, 0.04),
        // **Whole semitones.** Four octaves each way over a track a couple of
        // hundred points long is about two points a semitone, so the cents
        // between them are not something a hand can aim at — a track that
        // offered them would be a track that could not reliably land on a
        // note. Nothing is lost: stepping never offered cents either.
        AudioField::Pitch => {
            clip.pitch_semitones = linear(MIN_CLIP_PITCH, MAX_CLIP_PITCH).round();
        }
        AudioField::Speed => {
            let wanted = log(MIN_CLIP_SPEED, MAX_CLIP_SPEED);
            clip.speed = f64::from(detent(wanted as f32, 1.0, 0.03));
        }
        AudioField::FadeIn | AudioField::FadeOut => {
            // Squared back out of the square above, and never longer than the
            // clip — a fade over more than it fades is a clip that never
            // reaches full level.
            let longest = clip.source_frames();
            let frames = ((t * t) as f64 * longest as f64).round() as Sample;
            let fade = if field == AudioField::FadeIn {
                &mut clip.fade_in
            } else {
                &mut clip.fade_out
            };
            fade.frames = frames.clamp(0, longest);
        }
        AudioField::Cutoff => {
            clip.filter.cutoff_hz = log(f64::from(MIN_FILTER_HZ), f64::from(MAX_FILTER_HZ)) as f32;
        }
        AudioField::Resonance => clip.filter.resonance = detent(t, 0.0, 0.02),
        AudioField::Drive => clip.filter.drive = detent(t, 0.0, 0.02),
        _ => {}
    }
}

/// Where a slider's fill grows **from**: the value that means "nothing done
/// to this clip".
///
/// Unity gain, dead centre, unison, normal speed, a filter wide open. A bar
/// that always grew from the left would say a clip cut two decibels and one
/// boosted twenty look like the same kind of thing done by different amounts,
/// which is the one fact a level control has to get across.
pub fn audio_row_neutral(field: AudioField) -> Option<f32> {
    let mut clip = identity();
    Some(match field {
        AudioField::Gain | AudioField::Pan | AudioField::Pitch | AudioField::Speed => {
            audio_row_fraction(&clip, field)?
        }
        // Wide open does nothing, and that is the top of the track.
        AudioField::Cutoff => {
            clip.filter.cutoff_hz = MAX_FILTER_HZ;
            audio_row_fraction(&clip, field)?
        }
        AudioField::FadeIn | AudioField::FadeOut | AudioField::Resonance | AudioField::Drive => 0.0,
        _ => return None,
    })
}

/// A clip with every knob where it does nothing — `AudioClipData::whole` over
/// a file that is not there, which is all [`audio_row_neutral`] needs.
fn identity() -> AudioClipData {
    AudioClipData::whole(
        fontelle_types::AssetRef {
            id: fontelle_types::AssetId::default(),
            path: std::path::PathBuf::new(),
            content_hash: 0,
            size: 0,
            kind: fontelle_types::AssetKind::Sample,
        },
        0,
        0,
    )
}

/// `value`, snapped to `to` when it is within `window` of it.
fn detent(value: f32, to: f32, window: f32) -> f32 {
    if (value - to).abs() <= window {
        to
    } else {
        value
    }
}

/// The furthest a clip may be repitched, in semitones — the clamp
/// `AudioClipData::rate` already applies, named so the slider and the step
/// share it rather than each writing 48 down.
pub const MIN_CLIP_PITCH: f32 = -48.0;
pub const MAX_CLIP_PITCH: f32 = 48.0;

/// The name in the row's left-hand column.
pub fn audio_row_label(field: AudioField) -> &'static str {
    match field {
        AudioField::Heading(title) => title,
        // "Boost" rather than "Gain", because that is the word that was used
        // and because a clip's own level is not the mixer's.
        AudioField::Route => "Mixer track",
        AudioField::Gain => "Boost",
        AudioField::Pan => "Pan",
        AudioField::Normalize => "Normalize",
        AudioField::Stretch => "Stretch",
        AudioField::Pitch => "Pitch",
        AudioField::Speed => "Speed",
        AudioField::Reverse => "Reverse",
        AudioField::Loop => "When it ends",
        AudioField::FadeIn => "Fade in",
        AudioField::FadeInCurve => "In shape",
        AudioField::FadeOut => "Fade out",
        AudioField::FadeOutCurve => "Out shape",
        AudioField::FilterShape => "Shape",
        AudioField::Cutoff => "Cutoff",
        AudioField::Resonance => "Resonance",
        AudioField::Drive => "Drive",
    }
}

/// What a hover tip says about the row.
pub fn audio_row_tip(field: AudioField) -> Option<&'static str> {
    Some(match field {
        AudioField::Heading(_) => return None,
        AudioField::Route => "Which mixer track this clip plays through",
        AudioField::Stretch => {
            "Whether it follows the song's tempo. Resample moves its pitch with it"
        }
        AudioField::Gain => "How loud this clip is \u{2014} the file is not changed",
        AudioField::Pan => "Where it sits, left to right",
        AudioField::Normalize => "Bring its loudest moment up to full scale",
        AudioField::Pitch => "In semitones. Speed follows, until time-stretch lands",
        AudioField::Speed => "How fast it is read. Pitch follows",
        AudioField::Reverse => "Play it backwards",
        AudioField::Loop => "Whether it comes round again when the block outlasts it",
        AudioField::FadeIn => "How long it takes to arrive",
        AudioField::FadeInCurve => "The shape it arrives with",
        AudioField::FadeOut => "How long it takes to leave",
        AudioField::FadeOutCurve => "The shape it leaves with",
        AudioField::FilterShape => "Which way the filter cuts",
        AudioField::Cutoff => "Where the filter sits. Wide open does nothing",
        AudioField::Resonance => "How much it sings at the corner",
        AudioField::Drive => "Harmonics into the filter, before it cuts",
    })
}

/// What it is set to, in the row's right-hand column.
///
/// `sample_rate` is the file's, so a fade reads in **milliseconds** — the unit
/// a fade is thought about in — rather than in frames, which is the unit it is
/// stored in and means nothing to anybody.
pub fn audio_row_value_at(
    clip: &AudioClipData,
    field: AudioField,
    sample_rate: u32,
    route: &str,
) -> String {
    let ms = |frames: Sample| {
        if sample_rate == 0 {
            return "0 ms".to_string();
        }
        let ms = frames as f64 * 1000.0 / f64::from(sample_rate);
        if ms >= 1000.0 {
            format!("{:.2} s", ms / 1000.0)
        } else {
            format!("{ms:.0} ms")
        }
    };
    match field {
        AudioField::Heading(_) => String::new(),
        // With its sign, always: "+0" against "0" is the difference between a
        // number that can go either way and one that might not.
        AudioField::Gain => format!("{:+.1} dB", clip.gain_db),
        AudioField::Pan => match clip.pan {
            p if p.abs() < 0.005 => "centre".to_string(),
            p if p < 0.0 => format!("{:.0}% left", -p * 100.0),
            p => format!("{:.0}% right", p * 100.0),
        },
        AudioField::Normalize => on_off(clip.normalize),
        AudioField::Pitch => format!("{:+.2} st", clip.pitch_semitones),
        AudioField::Speed => format!("{:.0}%", clip.speed * 100.0),
        AudioField::Reverse => on_off(clip.reverse),
        AudioField::Loop => clip.loop_mode.label().to_string(),
        AudioField::Stretch => clip.stretch.label().to_string(),
        // The **name** is the caller's: this crate may not see a `Project`
        // (INVARIANT 2), so what a `MixerTrackId` is called is something only
        // the window knows. It is passed in rather than left blank, because a
        // row that says nothing about what it is at reads as a broken row —
        // the rule every other panel of rows in this window keeps.
        AudioField::Route => route.to_string(),
        AudioField::FadeIn => ms(clip.fade_in.frames),
        AudioField::FadeInCurve => clip.fade_in.curve.label().to_string(),
        AudioField::FadeOut => ms(clip.fade_out.frames),
        AudioField::FadeOutCurve => clip.fade_out.curve.label().to_string(),
        AudioField::FilterShape => clip.filter.shape.label().to_string(),
        AudioField::Cutoff => {
            if clip.filter.cutoff_hz >= 1000.0 {
                format!("{:.1} kHz", clip.filter.cutoff_hz / 1000.0)
            } else {
                format!("{:.0} Hz", clip.filter.cutoff_hz)
            }
        }
        AudioField::Resonance => format!("{:.0}%", clip.filter.resonance * 100.0),
        AudioField::Drive => format!("{:.0}%", clip.filter.drive * 100.0),
    }
}

/// [`audio_row_value_at`] at the rate a file is most likely to be.
///
/// The window knows the real one and passes it; this is for the tests and for
/// a clip whose asset has gone missing, where a fade reading in 48 kHz
/// milliseconds is better than one reading in frames.
pub fn audio_row_value(clip: &AudioClipData, field: AudioField) -> String {
    // A clip on the master, at the commonest rate: the convenience form, for
    // callers that are asking about a value rather than about a project.
    audio_row_value_at(clip, field, 48_000, MASTER_ROUTE)
}

/// What the route row says for a clip that goes straight out.
///
/// `AudioClipData::mixer_track` spells the master `None`, the same convention
/// `Channel::mixer_track` follows, so there is no name to look up for it — and
/// one place to write the word rather than three.
pub const MASTER_ROUTE: &str = "Master";

fn on_off(value: bool) -> String {
    if value { "on" } else { "off" }.to_string()
}

/// How far one click moves a fade, in milliseconds.
///
/// A ladder rather than a step, because the useful lengths span three orders of
/// magnitude: a click-remover is five milliseconds and a long swell is four
/// seconds, and stepping by one from one to the other is not a control.
const FADE_LADDER_MS: [f64; 14] = [
    0.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 1500.0, 2000.0, 3000.0, 4000.0, 8000.0,
];

/// Steps `field` one place in the direction `direction` says.
///
/// **A choice wraps and a number stops**, the same rule the settings tab and
/// the tool dialogs follow: six filter shapes are a set with no ends, so
/// stopping at one would mean knowing to go back through the others; ±24 dB are
/// the ends of a range, and running off one and arriving at the other is a
/// control that cannot be trusted to a held press.
///
/// `sample_rate` is the file's, for the two rows measured in time.
pub fn nudge_audio_row(
    clip: &mut AudioClipData,
    field: AudioField,
    direction: i32,
    sample_rate: u32,
) {
    if direction == 0 {
        return;
    }
    let step = direction.signum();
    let forward = step > 0;
    match field {
        AudioField::Heading(_) => {}
        AudioField::Gain => {
            clip.gain_db = (clip.gain_db + step as f32).clamp(MIN_CLIP_GAIN_DB, MAX_CLIP_GAIN_DB);
        }
        AudioField::Pan => {
            clip.pan = (clip.pan + step as f32 * 0.1).clamp(-1.0, 1.0);
        }
        AudioField::Normalize => clip.normalize = !clip.normalize,
        AudioField::Pitch => {
            clip.pitch_semitones =
                (clip.pitch_semitones + step as f32).clamp(MIN_CLIP_PITCH, MAX_CLIP_PITCH);
        }
        AudioField::Speed => {
            // By a ratio, like the cutoff and for the same reason: ten per cent
            // of half speed and ten per cent of double speed are different
            // amounts of the same musical distance.
            let ratio = if forward { 1.0594631 } else { 1.0 / 1.0594631 };
            clip.speed = (clip.speed * ratio).clamp(MIN_CLIP_SPEED, MAX_CLIP_SPEED);
        }
        AudioField::Reverse => clip.reverse = !clip.reverse,
        AudioField::Stretch => {
            clip.stretch = cycle(&ClipStretch::ALL, clip.stretch, forward);
        }
        // Stepped by `nudge_route`, which needs the list of tracks and
        // therefore cannot be answered from the clip alone.
        AudioField::Route => {}
        AudioField::Loop => {
            clip.loop_mode = match clip.loop_mode {
                ClipLoopMode::Once => ClipLoopMode::Loop,
                ClipLoopMode::Loop => ClipLoopMode::Once,
            };
        }
        AudioField::FadeIn | AudioField::FadeOut => {
            let longest = clip.source_frames();
            let rate = f64::from(sample_rate.max(1));
            let fade = if field == AudioField::FadeIn {
                &mut clip.fade_in
            } else {
                &mut clip.fade_out
            };
            let now_ms = fade.frames as f64 * 1000.0 / rate;
            let next = ladder_step(&FADE_LADDER_MS, now_ms, step);
            // Never longer than the clip: a fade over more than it fades is a
            // clip that never reaches full level, which is a thing to be able
            // to ask for by mistake and not a thing to be stuck with.
            fade.frames = ((next * rate / 1000.0) as Sample).clamp(0, longest);
        }
        AudioField::FadeInCurve => {
            clip.fade_in.curve = cycle(&FadeCurve::ALL, clip.fade_in.curve, forward);
        }
        AudioField::FadeOutCurve => {
            clip.fade_out.curve = cycle(&FadeCurve::ALL, clip.fade_out.curve, forward);
        }
        AudioField::FilterShape => {
            clip.filter.shape = cycle(&FilterShape::ALL, clip.filter.shape, forward);
        }
        AudioField::Cutoff => {
            // By a ratio — a **semitone**, so a filter can be tuned to the
            // music. Equal steps in hertz are useless: a hundred hertz is a
            // third of the way up the bass and inaudible at the top.
            let ratio = if forward { 1.0594631 } else { 1.0 / 1.0594631 };
            clip.filter.cutoff_hz =
                (clip.filter.cutoff_hz * ratio).clamp(MIN_FILTER_HZ, MAX_FILTER_HZ);
        }
        AudioField::Resonance => {
            clip.filter.resonance = (clip.filter.resonance + step as f32 * 0.05).clamp(0.0, 1.0);
        }
        AudioField::Drive => {
            clip.filter.drive = (clip.filter.drive + step as f32 * 0.05).clamp(0.0, 1.0);
        }
    }
}

/// `value` moved one rung along `ladder`, stopping at both ends.
fn ladder_step(ladder: &[f64], value: f64, step: i32) -> f64 {
    if step > 0 {
        ladder
            .iter()
            .copied()
            .find(|rung| *rung > value + 1e-6)
            .unwrap_or_else(|| ladder.last().copied().unwrap_or(value))
    } else {
        ladder
            .iter()
            .copied()
            .rev()
            .find(|rung| *rung < value - 1e-6)
            .unwrap_or_else(|| ladder.first().copied().unwrap_or(value))
    }
}

/// The next value round a set of choices.
fn cycle<T: Copy + PartialEq>(all: &[T], current: T, forward: bool) -> T {
    if all.is_empty() {
        return current;
    }
    let at = all.iter().position(|c| *c == current).unwrap_or(0) as i32;
    let next = (at + if forward { 1 } else { -1 }).rem_euclid(all.len() as i32) as usize;
    all[next]
}

// ------------------------------------------------------------- geometry ---

/// The editor window's insides, laid out.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioEditorLayout {
    /// The clip's own waveform, across the top. A fade you cannot see is a fade
    /// you are aiming blind.
    pub waveform: Rect,
    /// One rectangle per row, in [`AUDIO_ROWS`] order. A row with no room left
    /// is an **empty** rectangle, which draws as nothing and hit-tests as
    /// absent — the list stays the same length either way, so a caller can
    /// index it.
    pub rows: Vec<(AudioField, Rect)>,
}

/// How much of the panel the waveform takes, in rows' worth of height.
///
/// Three rows: enough to read a shape, and not so much that the list below it
/// needs scrolling on a window that opens at its default size.
const WAVEFORM_ROWS: f32 = 3.0;

/// Lays the editor out inside `body`.
pub fn audio_editor_layout(body: Rect, metrics: &Metrics, rows: usize) -> AudioEditorLayout {
    let pad = metrics.panel_padding;
    let row_height = metrics.row_height.max(1.0);
    let inner = Rect::new(
        body.x + pad,
        body.y + pad,
        (body.width - pad * 2.0).max(0.0),
        (body.height - pad * 2.0).max(0.0),
    )
    .clamped();

    let waveform = Rect::new(inner.x, inner.y, inner.width, row_height * WAVEFORM_ROWS)
        .intersection(&inner)
        .clamped();
    let top = waveform.bottom() + pad;
    let laid = AUDIO_ROWS
        .iter()
        .take(rows)
        .copied()
        .enumerate()
        .map(|(index, field)| {
            let rect = Rect::new(
                inner.x,
                top + row_height * index as f32,
                inner.width,
                row_height,
            );
            // Clipped to the panel rather than dropped, so a row that ran off
            // the end of a short window is an empty rectangle: it draws as
            // nothing and hit-tests as absent, and the list keeps its length.
            (field, rect.intersection(&body).clamped())
        })
        .collect();

    AudioEditorLayout {
        waveform,
        rows: laid,
    }
}

/// How much of a row its control takes, as a fraction of the row's width.
///
/// Half: the names are short and the numbers are short, and a track with less
/// than this has too little travel to set a cutoff on.
const CONTROL_FRACTION: f32 = 0.5;

/// The control's own rectangle inside `row` — the track a slider is dragged
/// along, the box a switch is drawn in, the field a drop-down hangs under.
///
/// Right-aligned, so a column of controls reads down the panel rather than
/// wandering with the names, and inset vertically so a row still reads as a
/// row rather than as a solid bar.
pub fn audio_row_control_rect(row: Rect, metrics: &Metrics) -> Rect {
    if row.is_empty() {
        return Rect::ZERO;
    }
    let pad = metrics.panel_padding.min(row.width / 4.0);
    let width = (row.width * CONTROL_FRACTION - pad).max(0.0);
    let inset = (row.height * 0.18).min(4.0);
    Rect::new(
        row.right() - pad - width,
        row.y + inset,
        width,
        (row.height - inset * 2.0).max(0.0),
    )
    .intersection(&row)
    .clamped()
}

/// The fraction a press at `x` along `row`'s track is asking for.
///
/// **Absolute**, the way the mixer's fader and pan are: a press jumps the
/// value to where it landed and the drag follows from there, because that is
/// how a track works everywhere and because the alternative makes "take it all
/// the way down" a long haul rather than one click.
pub fn audio_slider_at(row: Rect, metrics: &Metrics, x: f32) -> f32 {
    let track = audio_row_control_rect(row, metrics);
    if track.width <= 0.0 {
        return 0.0;
    }
    ((x - track.x) / track.width).clamp(0.0, 1.0)
}

/// The other direction: where along the track a fraction sits.
pub fn audio_slider_x_of(row: Rect, metrics: &Metrics, t: f32) -> f32 {
    let track = audio_row_control_rect(row, metrics);
    track.x + track.width * t.clamp(0.0, 1.0)
}

/// Which row `(x, y)` is on, if it is on one.
///
/// The waveform is not one: it says what the clip is and there is nothing to
/// press. A heading **is** one, unlike a greyed menu entry, so the caller knows
/// the press landed on the panel — [`nudge_audio_row`] already does nothing to
/// a heading.
pub fn audio_editor_hit(layout: &AudioEditorLayout, x: f32, y: f32) -> Option<AudioField> {
    layout
        .rows
        .iter()
        .find(|(_, rect)| !rect.is_empty() && rect.contains(x, y))
        .map(|(field, _)| *field)
}

/// Points `clip` at the next mixer track in `tracks`, or the previous one.
///
/// > *"soundclips dont have options right now for selecting their mixer
/// > track."*
///
/// Its own function rather than an arm of [`nudge_audio_row`] because it is
/// the one row whose answer is not in the clip: which tracks exist is the
/// document's, and this crate may not see one (INVARIANT 2). `tracks` is the
/// list the window already keeps for every other route control — **master
/// first, as `None`**, then each track somebody made, in the order the mixer
/// lays them out.
///
/// A choice, so it **wraps**: the same rule the loop mode and the filter shape
/// follow, and for the same reason — a list of destinations has no ends, and
/// stopping at one would mean going back through the others to reach it.
///
/// A clip pointed at a track that has since been deleted is not in the list at
/// all, so a step from there lands on the master rather than on nothing: a row
/// that cannot be stepped is a clip that can never be re-routed.
pub fn nudge_route(
    clip: &mut AudioClipData,
    direction: i32,
    tracks: &[Option<fontelle_types::MixerTrackId>],
) {
    if direction == 0 || tracks.is_empty() {
        return;
    }
    let at = tracks.iter().position(|t| *t == clip.mixer_track);
    let next = match at {
        Some(at) => {
            let len = tracks.len() as i32;
            let step = direction.signum();
            (((at as i32 + step) % len) + len) % len
        }
        // Somewhere that no longer exists: the master, which is where a clip
        // whose track was deleted is already being heard.
        None => 0,
    };
    clip.mixer_track = tracks[next as usize];
}

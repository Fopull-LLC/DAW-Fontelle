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
//! **Rows of a name and a value, where a click steps the value forward and a
//! Ctrl+click steps it back**, under headings that say what each group is for.
//! That is the settings tab's shape and the tool dialogs' shape, already in
//! this window and already understood; no text field is involved, because there
//! is not one in this window and a value you can reach in a handful of clicks
//! is quicker than one you have to type.
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
    AudioClipData, ClipLoopMode, FadeCurve, FilterShape, MAX_CLIP_GAIN_DB, MAX_CLIP_SPEED,
    MAX_FILTER_HZ, MIN_CLIP_GAIN_DB, MIN_CLIP_SPEED, MIN_FILTER_HZ, Sample,
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
    /// *"the boost"*, in decibels.
    Gain,
    Pan,
    Normalize,
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
pub const AUDIO_ROWS: [AudioField; 19] = [
    AudioField::Heading("Level"),
    AudioField::Gain,
    AudioField::Pan,
    AudioField::Normalize,
    AudioField::Heading("Time"),
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

/// The name in the row's left-hand column.
pub fn audio_row_label(field: AudioField) -> &'static str {
    match field {
        AudioField::Heading(title) => title,
        // "Boost" rather than "Gain", because that is the word that was used
        // and because a clip's own level is not the mixer's.
        AudioField::Gain => "Boost",
        AudioField::Pan => "Pan",
        AudioField::Normalize => "Normalize",
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
pub fn audio_row_value_at(clip: &AudioClipData, field: AudioField, sample_rate: u32) -> String {
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
    audio_row_value_at(clip, field, 48_000)
}

fn on_off(value: bool) -> String {
    if value { "on" } else { "off" }.to_string()
}

/// How far one click moves a fade, in milliseconds.
///
/// A ladder rather than a step, because the useful lengths span three orders of
/// magnitude: a click-remover is five milliseconds and a long swell is four
/// seconds, and stepping by one from one to the other is not a control.
const FADE_LADDER_MS: [f64; 14] = [
    0.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 1500.0, 2000.0, 3000.0, 4000.0,
    8000.0,
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
            clip.gain_db =
                (clip.gain_db + step as f32).clamp(MIN_CLIP_GAIN_DB, MAX_CLIP_GAIN_DB);
        }
        AudioField::Pan => {
            clip.pan = (clip.pan + step as f32 * 0.1).clamp(-1.0, 1.0);
        }
        AudioField::Normalize => clip.normalize = !clip.normalize,
        AudioField::Pitch => {
            clip.pitch_semitones = (clip.pitch_semitones + step as f32).clamp(-48.0, 48.0);
        }
        AudioField::Speed => {
            // By a ratio, like the cutoff and for the same reason: ten per cent
            // of half speed and ten per cent of double speed are different
            // amounts of the same musical distance.
            let ratio = if forward { 1.0594631 } else { 1.0 / 1.0594631 };
            clip.speed = (clip.speed * ratio).clamp(MIN_CLIP_SPEED, MAX_CLIP_SPEED);
        }
        AudioField::Reverse => clip.reverse = !clip.reverse,
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
            let rect = Rect::new(inner.x, top + row_height * index as f32, inner.width, row_height);
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

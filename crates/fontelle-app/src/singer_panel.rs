//! The vocal instrument panel's parameter map: what each control on a vocal
//! track's instrument panel means to the channel's
//! [`VocalStyle`](fontelle_model::VocalStyle) (`docs/vocal-plan.md` §2.5).
//!
//! The Fontelle Singer's panel and an engine's panel without a window of
//! its own are the same table, read against what the voice declares
//! ([`Capabilities`]): its lanes become the Voice section's knobs, its
//! vibrato and glides the Expression section's, its declared settings and
//! per-note options the Performance section's. A control the voice ignores
//! is still there — a track keeps what it set for when another singer sings
//! it — and is greyed, saying why.
//!
//! Every address is written down once, in [`controls`]: the read
//! ([`describe`]) and the write ([`set`]) both go through it, as the patch
//! panel's table does (`crate::instrument`).
//!
//! **Addresses are INVARIANT 7's**: `vocal/lane/<lane>`, `vocal/vibrato/*`,
//! `vocal/glide/*`, `vocal/consonant_speed`, `vocal/setting/<id>`,
//! `vocal/extra/<id>`, `vocal/option/<id>` (the last three the singing
//! backend's own ids). They never change once assigned; an address this
//! build does not know changes nothing.

use std::collections::BTreeMap;

use fontelle_model::{AutoGlide, AutoVibrato, VocalStyle, lane_spec};
use fontelle_sing::{Capabilities, GlideShape, Lane};
use fontelle_types::ParamAddress;
use fontelle_ui::canvas::{InstrumentGroup, InstrumentParam, InstrumentView, ParamKind};

/// The sections, in the order the panel shows them.
pub const VOICE: &str = "Voice";
pub const EXPRESSION: &str = "Expression";
pub const PERFORMANCE: &str = "Performance";
pub const OUTPUT: &str = "Output";

/// The standard lanes the Voice section offsets, with their addresses.
const VOICE_LANES: [(Lane, &str, &str); 4] = [
    (Lane::Gender, "vocal/lane/gender", "Gender"),
    (Lane::Breathiness, "vocal/lane/breathiness", "Breath"),
    (Lane::Tension, "vocal/lane/tension", "Tension"),
    (Lane::Voicing, "vocal/lane/voicing", "Voicing"),
];

/// The auto vibrato's ranges: what OpenUtau's and Synthesizer V's vibrato
/// panels span.
const DEPTH_CENTS: (f32, f32) = (0.0, 200.0);
const RATE_HZ: (f32, f32) = (3.0, 9.0);
const MIN_LENGTH_S: (f32, f32) = (0.0, 2.0);
const GLIDE_S: (f32, f32) = (0.01, 0.5);
const SHAPES: [(GlideShape, &str); 4] = [
    (GlideShape::S, "S-curve"),
    (GlideShape::Linear, "Linear"),
    (GlideShape::Out, "Fast in"),
    (GlideShape::In, "Slow in"),
];

/// What a control is, and so how a 0..1 maps onto the style and back.
#[derive(Debug, Clone, PartialEq)]
enum Control {
    /// An amount added to a standard lane over the whole track.
    Lane(Lane),
    VibratoOn,
    VibratoDepth,
    VibratoRate,
    VibratoDelay,
    VibratoFadeIn,
    VibratoFadeOut,
    VibratoMinLength,
    GlideOn,
    GlideLength,
    GlideShape,
    ConsonantSpeed,
    /// The backend's declared setting `index` (into `Capabilities::settings`).
    Setting(usize),
    /// The backend's own lane `index`'s value where nothing is drawn.
    Extra(usize),
    /// The backend's per-note option `index`'s default.
    Option(usize),
}

/// One control: where it is, what it is called, and what it is.
struct Spec {
    section: &'static str,
    address: String,
    label: String,
    control: Control,
}

fn spec(
    section: &'static str,
    address: impl Into<String>,
    label: impl Into<String>,
    control: Control,
) -> Spec {
    Spec {
        section,
        address: address.into(),
        label: label.into(),
        control,
    }
}

/// Every control a voice with `caps` gets, in order.
fn controls(caps: &Capabilities) -> Vec<Spec> {
    let mut out: Vec<Spec> = VOICE_LANES
        .iter()
        .map(|(lane, address, label)| spec(VOICE, *address, *label, Control::Lane(*lane)))
        .collect();
    for (n, lane) in caps.extra_lanes.iter().enumerate() {
        out.push(spec(
            VOICE,
            format!("vocal/extra/{}", lane.id),
            lane.name.clone(),
            Control::Extra(n),
        ));
    }
    out.extend([
        spec(
            EXPRESSION,
            "vocal/vibrato/on",
            "Vibrato",
            Control::VibratoOn,
        ),
        spec(
            EXPRESSION,
            "vocal/vibrato/depth",
            "Depth",
            Control::VibratoDepth,
        ),
        spec(
            EXPRESSION,
            "vocal/vibrato/rate",
            "Rate",
            Control::VibratoRate,
        ),
        spec(
            EXPRESSION,
            "vocal/vibrato/delay",
            "Delay",
            Control::VibratoDelay,
        ),
        spec(
            EXPRESSION,
            "vocal/vibrato/fade_in",
            "Fade in",
            Control::VibratoFadeIn,
        ),
        spec(
            EXPRESSION,
            "vocal/vibrato/fade_out",
            "Fade out",
            Control::VibratoFadeOut,
        ),
        spec(
            EXPRESSION,
            "vocal/vibrato/min_length",
            "Notes over",
            Control::VibratoMinLength,
        ),
        spec(EXPRESSION, "vocal/glide/on", "Glide", Control::GlideOn),
        spec(
            EXPRESSION,
            "vocal/glide/length",
            "Glide time",
            Control::GlideLength,
        ),
        spec(
            EXPRESSION,
            "vocal/glide/shape",
            "Glide shape",
            Control::GlideShape,
        ),
        spec(
            EXPRESSION,
            "vocal/consonant_speed",
            "Consonants",
            Control::ConsonantSpeed,
        ),
    ]);
    for (n, setting) in caps.settings.iter().enumerate() {
        out.push(spec(
            PERFORMANCE,
            format!("vocal/setting/{}", setting.id),
            setting.name.clone(),
            Control::Setting(n),
        ));
    }
    for (n, option) in caps.note_options.iter().enumerate() {
        out.push(spec(
            PERFORMANCE,
            format!("vocal/option/{}", option.id),
            option.name.clone(),
            Control::Option(n),
        ));
    }
    out
}

fn lerp(range: (f32, f32), t: f32) -> f32 {
    range.0 + (range.1 - range.0) * t.clamp(0.0, 1.0)
}

fn unlerp(range: (f32, f32), v: f32) -> f32 {
    if range.1 == range.0 {
        return 0.0;
    }
    ((v - range.0) / (range.1 - range.0)).clamp(0.0, 1.0)
}

/// A list's 0..1 for option `index` of `count`, and back.
fn list_value(index: usize, count: usize) -> f32 {
    if count < 2 {
        0.0
    } else {
        index as f32 / (count - 1) as f32
    }
}

fn list_index(value: f32, count: usize) -> usize {
    if count < 2 {
        return 0;
    }
    ((value.clamp(0.0, 1.0) * (count - 1) as f32).round() as usize).min(count - 1)
}

fn lane_range(lane: Lane) -> (f32, f32) {
    let s = lane_spec(lane);
    (s.min, s.max)
}

/// Consonant speed: half to twice, the middle the notes' own.
fn speed_of(t: f32) -> f32 {
    2f32.powf(t.clamp(0.0, 1.0) * 2.0 - 1.0)
}

fn speed_norm(speed: f32) -> f32 {
    ((speed.max(1e-3).log2() + 1.0) / 2.0).clamp(0.0, 1.0)
}

fn percent(v: f32) -> String {
    format!("{:+.0}%", v * 100.0)
}

fn number(v: f32, unit: &str, range: (f32, f32)) -> String {
    let decimals = if (range.1 - range.0).abs() <= 2.0 {
        2
    } else {
        0
    };
    let unit = if unit.is_empty() {
        String::new()
    } else {
        format!(" {unit}")
    };
    format!("{v:.decimals$}{unit}")
}

/// The auto vibrato and glide as they read, untouched ones as their
/// defaults (off).
fn vibrato_of(style: &VocalStyle) -> AutoVibrato {
    style.vibrato.unwrap_or_default()
}

fn glide_of(style: &VocalStyle) -> AutoGlide {
    style.glide.unwrap_or_default()
}

/// One control's 0..1 and read-out, as `style` has it.
fn read(
    control: &Control,
    caps: &Capabilities,
    backend: &str,
    style: &VocalStyle,
) -> (f32, String, ParamKind) {
    let vib = vibrato_of(style);
    let glide = glide_of(style);
    let switch = |on: bool| {
        (
            if on { 1.0 } else { 0.0 },
            if on { "on" } else { "off" }.to_string(),
            ParamKind::Switch,
        )
    };
    match control {
        Control::Lane(lane) => {
            let v = style.lanes.get(lane).copied().unwrap_or(0.0);
            let range = lane_range(*lane);
            let display = match lane {
                Lane::Gender => format!("{v:+.0} c"),
                _ => percent(v),
            };
            (unlerp(range, v), display, ParamKind::Knob)
        }
        Control::VibratoOn => switch(vib.enabled),
        Control::VibratoDepth => (
            unlerp(DEPTH_CENTS, vib.vibrato.depth),
            format!("{:.0} c", vib.vibrato.depth),
            ParamKind::Knob,
        ),
        Control::VibratoRate => (
            unlerp(RATE_HZ, vib.vibrato.rate),
            format!("{:.1} Hz", vib.vibrato.rate),
            ParamKind::Knob,
        ),
        Control::VibratoDelay => (
            vib.vibrato.delay.clamp(0.0, 1.0),
            format!("{:.0}%", vib.vibrato.delay * 100.0),
            ParamKind::Knob,
        ),
        Control::VibratoFadeIn => (
            vib.vibrato.fade_in.clamp(0.0, 1.0),
            format!("{:.0}%", vib.vibrato.fade_in * 100.0),
            ParamKind::Knob,
        ),
        Control::VibratoFadeOut => (
            vib.vibrato.fade_out.clamp(0.0, 1.0),
            format!("{:.0}%", vib.vibrato.fade_out * 100.0),
            ParamKind::Knob,
        ),
        Control::VibratoMinLength => (
            unlerp(MIN_LENGTH_S, vib.min_length),
            format!("{:.2} s", vib.min_length),
            ParamKind::Knob,
        ),
        Control::GlideOn => switch(glide.enabled),
        Control::GlideLength => (
            unlerp(GLIDE_S, glide.portamento.length),
            format!("{:.0} ms", glide.portamento.length * 1000.0),
            ParamKind::Knob,
        ),
        Control::GlideShape => {
            let index = SHAPES
                .iter()
                .position(|(s, _)| *s == glide.portamento.shape)
                .unwrap_or(0);
            (
                list_value(index, SHAPES.len()),
                SHAPES[index].1.to_string(),
                ParamKind::Choice(SHAPES.iter().map(|(_, n)| n.to_string()).collect()),
            )
        }
        Control::ConsonantSpeed => (
            speed_norm(style.consonant_speed),
            format!("\u{d7}{:.2}", style.consonant_speed),
            ParamKind::Knob,
        ),
        Control::Setting(n) => {
            let Some(info) = caps.settings.get(*n) else {
                return (0.0, String::new(), ParamKind::Knob);
            };
            let v = style
                .settings
                .get(backend)
                .and_then(|s| s.get(&info.id))
                .copied()
                .unwrap_or(info.default);
            if info.choices.is_empty() {
                (
                    unlerp((info.min, info.max), v),
                    number(v, &info.unit, (info.min, info.max)),
                    ParamKind::Knob,
                )
            } else {
                let count = info.choices.len();
                let index = ((v - info.min).round().max(0.0) as usize).min(count - 1);
                (
                    list_value(index, count),
                    info.choices[index].clone(),
                    ParamKind::Choice(info.choices.clone()),
                )
            }
        }
        Control::Extra(n) => {
            let Some(info) = caps.extra_lanes.get(*n) else {
                return (0.0, String::new(), ParamKind::Knob);
            };
            let v = style
                .extra
                .get(backend)
                .and_then(|s| s.get(&info.id))
                .copied()
                .unwrap_or(info.default);
            (
                unlerp((info.min, info.max), v),
                number(v, &info.unit, (info.min, info.max)),
                ParamKind::Knob,
            )
        }
        Control::Option(n) => {
            let Some(info) = caps.note_options.get(*n) else {
                return (0.0, String::new(), ParamKind::Knob);
            };
            let chosen = style.options.get(backend).and_then(|o| o.get(&info.id));
            let index = chosen
                .and_then(|c| info.choices.iter().position(|(v, _)| v == c))
                .unwrap_or(0);
            let count = info.choices.len().max(1);
            (
                list_value(index, count),
                info.choices
                    .get(index)
                    .map(|(_, label)| label.clone())
                    .unwrap_or_default(),
                ParamKind::Choice(info.choices.iter().map(|(_, l)| l.clone()).collect()),
            )
        }
    }
}

/// Why a voice ignores a control, or `None` when it uses it.
fn ignored(control: &Control, caps: &Capabilities, who: &str) -> Option<String> {
    let uses = match control {
        Control::Lane(lane) => caps.lanes.contains(lane),
        Control::VibratoOn
        | Control::VibratoDepth
        | Control::VibratoRate
        | Control::VibratoDelay
        | Control::VibratoFadeIn
        | Control::VibratoFadeOut
        | Control::VibratoMinLength => caps.vibrato,
        Control::GlideOn | Control::GlideLength | Control::GlideShape => caps.portamento,
        Control::ConsonantSpeed => caps.velocity,
        Control::Setting(_) | Control::Extra(_) | Control::Option(_) => true,
    };
    if uses {
        return None;
    }
    let what = match control {
        Control::Lane(Lane::Gender) => "gender (formant)",
        Control::Lane(Lane::Breathiness) => "breathiness",
        Control::Lane(Lane::Tension) => "tension",
        Control::Lane(Lane::Voicing) => "voicing",
        Control::Lane(_) => "that lane",
        Control::GlideOn | Control::GlideLength | Control::GlideShape => "glides",
        Control::ConsonantSpeed => "consonant speed",
        _ => "vibrato",
    };
    Some(format!("{who} doesn't use {what}"))
}

/// Everything the panel shows for a vocal track: its sections of controls
/// (Voice, Expression, Performance — the empty ones left out — and
/// Output), and which of them `who` (the singer, or an engine by name)
/// ignores, by address, with why.
pub fn describe(
    title: &str,
    caps: &Capabilities,
    backend: &str,
    who: &str,
    style: &VocalStyle,
    gain_db: f32,
    pan: f32,
) -> (InstrumentView, Vec<(String, String)>) {
    let mut groups: Vec<InstrumentGroup> = [VOICE, EXPRESSION, PERFORMANCE]
        .iter()
        .map(|name| InstrumentGroup {
            name: name.to_string(),
            params: Vec::new(),
        })
        .collect();
    let mut greyed = Vec::new();
    for s in controls(caps) {
        let (value, display, kind) = read(&s.control, caps, backend, style);
        if let Some(why) = ignored(&s.control, caps, who) {
            greyed.push((s.address.clone(), why));
        }
        let Some(group) = groups.iter_mut().find(|g| g.name == s.section) else {
            continue;
        };
        group.params.push(InstrumentParam {
            address: ParamAddress::new(s.address),
            label: s.label,
            value,
            display,
            kind,
            automated: false,
        });
    }
    groups.retain(|g| !g.params.is_empty());
    let mut output = crate::instrument::channel_group(gain_db, pan);
    output.name = OUTPUT.to_string();
    for (param, label) in output.params.iter_mut().zip(["Volume", "Pan"]) {
        param.label = label.to_string();
    }
    groups.push(output);
    (
        InstrumentView {
            title: title.to_string(),
            keys: Vec::new(),
            key: None,
            groups,
        },
        greyed,
    )
}

fn find(caps: &Capabilities, address: &str) -> Option<Control> {
    controls(caps)
        .into_iter()
        .find(|s| s.address == address)
        .map(|s| s.control)
}

/// Removes `id` from `map[backend]`, and the backend when it is left empty.
fn clear<V>(map: &mut BTreeMap<String, BTreeMap<String, V>>, backend: &str, id: &str) {
    if let Some(inner) = map.get_mut(backend) {
        inner.remove(id);
        if inner.is_empty() {
            map.remove(backend);
        }
    }
}

/// Writes control `address` at `value` (0..1) into `style`. `false` when
/// the address is not one of this voice's controls (and nothing changed).
/// A value back at its default is taken out rather than stored, so a song
/// whose knobs were all turned back writes nothing.
pub fn set(
    style: &mut VocalStyle,
    caps: &Capabilities,
    backend: &str,
    address: &str,
    value: f32,
) -> bool {
    let Some(control) = find(caps, address) else {
        return false;
    };
    let value = value.clamp(0.0, 1.0);
    let mut vib = vibrato_of(style);
    let mut glide = glide_of(style);
    match &control {
        Control::Lane(lane) => {
            let v = lerp(lane_range(*lane), value);
            // The middle of a knob is a detent: no offset at all.
            if v.abs() < (lane_range(*lane).1 - lane_range(*lane).0) * 0.005 {
                style.lanes.remove(lane);
            } else {
                style.lanes.insert(*lane, v);
            }
        }
        Control::VibratoOn => vib.enabled = value >= 0.5,
        Control::VibratoDepth => vib.vibrato.depth = lerp(DEPTH_CENTS, value),
        Control::VibratoRate => vib.vibrato.rate = lerp(RATE_HZ, value),
        Control::VibratoDelay => vib.vibrato.delay = value,
        Control::VibratoFadeIn => vib.vibrato.fade_in = value,
        Control::VibratoFadeOut => vib.vibrato.fade_out = value,
        Control::VibratoMinLength => vib.min_length = lerp(MIN_LENGTH_S, value),
        Control::GlideOn => glide.enabled = value >= 0.5,
        Control::GlideLength => glide.portamento.length = lerp(GLIDE_S, value),
        Control::GlideShape => {
            glide.portamento.shape = SHAPES[list_index(value, SHAPES.len())].0;
        }
        Control::ConsonantSpeed => {
            let speed = speed_of(value);
            style.consonant_speed = if (speed - 1.0).abs() < 0.01 {
                1.0
            } else {
                speed
            };
        }
        Control::Setting(n) => {
            let Some(info) = caps.settings.get(*n) else {
                return false;
            };
            let v = if info.choices.is_empty() {
                lerp((info.min, info.max), value)
            } else {
                info.min + list_index(value, info.choices.len()) as f32
            };
            if (v - info.default).abs() < 1e-6 {
                clear(&mut style.settings, backend, &info.id);
            } else {
                style
                    .settings
                    .entry(backend.to_string())
                    .or_default()
                    .insert(info.id.clone(), v);
            }
        }
        Control::Extra(n) => {
            let Some(info) = caps.extra_lanes.get(*n) else {
                return false;
            };
            let v = lerp((info.min, info.max), value);
            if (v - info.default).abs() < 1e-6 {
                clear(&mut style.extra, backend, &info.id);
            } else {
                style
                    .extra
                    .entry(backend.to_string())
                    .or_default()
                    .insert(info.id.clone(), v);
            }
        }
        Control::Option(n) => {
            let Some(info) = caps.note_options.get(*n) else {
                return false;
            };
            let index = list_index(value, info.choices.len());
            match info.choices.get(index) {
                // The first choice is the default: a note with none sings it.
                Some((v, _)) if index > 0 => {
                    style
                        .options
                        .entry(backend.to_string())
                        .or_default()
                        .insert(info.id.clone(), v.clone());
                }
                _ => clear(&mut style.options, backend, &info.id),
            }
        }
    }
    // An auto vibrato or glide left at its untouched default is none.
    style.vibrato = (vib != AutoVibrato::default()).then_some(vib);
    style.glide = (glide != AutoGlide::default()).then_some(glide);
    true
}

/// The 0..1 a control rests at — what Alt-click puts back.
pub fn default_value(caps: &Capabilities, backend: &str, address: &str) -> Option<f32> {
    let control = find(caps, address)?;
    Some(read(&control, caps, backend, &VocalStyle::default()).0)
}

/// The 0..1 a typed entry means: a number in the control's own units
/// ("300" cents, "6.5" Hz, "40%"), or a list's option by name.
pub fn from_text(caps: &Capabilities, backend: &str, address: &str, text: &str) -> Option<f32> {
    let control = find(caps, address)?;
    let (_, _, kind) = read(&control, caps, backend, &VocalStyle::default());
    let text = text.trim();
    if let ParamKind::Choice(options) = &kind {
        let index = options.iter().position(|o| o.eq_ignore_ascii_case(text))?;
        return Some(list_value(index, options.len()));
    }
    let lowered = text.to_ascii_lowercase();
    if matches!(control, Control::VibratoOn | Control::GlideOn) {
        return match lowered.as_str() {
            "on" | "1" | "yes" => Some(1.0),
            "off" | "0" | "no" => Some(0.0),
            _ => None,
        };
    }
    let is_percent = lowered.ends_with('%');
    let digits: String = lowered
        .chars()
        .filter(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+'))
        .collect();
    let mut v: f32 = digits.parse().ok()?;
    let fraction = |v: f32| {
        if is_percent || v.abs() > 1.0 {
            v / 100.0
        } else {
            v
        }
    };
    Some(match &control {
        Control::Lane(Lane::Gender) => unlerp(lane_range(Lane::Gender), v),
        Control::Lane(lane) => unlerp(lane_range(*lane), fraction(v)),
        Control::VibratoDepth => unlerp(DEPTH_CENTS, v),
        Control::VibratoRate => unlerp(RATE_HZ, v),
        Control::VibratoDelay | Control::VibratoFadeIn | Control::VibratoFadeOut => {
            fraction(v).clamp(0.0, 1.0)
        }
        Control::VibratoMinLength => unlerp(MIN_LENGTH_S, v),
        Control::GlideLength => {
            if lowered.ends_with("ms") || v > 1.0 {
                v /= 1000.0;
            }
            unlerp(GLIDE_S, v)
        }
        Control::ConsonantSpeed => speed_norm(v),
        Control::Setting(n) => {
            let info = caps.settings.get(*n)?;
            unlerp((info.min, info.max), v)
        }
        Control::Extra(n) => {
            let info = caps.extra_lanes.get(*n)?;
            unlerp((info.min, info.max), v)
        }
        _ => return None,
    })
}

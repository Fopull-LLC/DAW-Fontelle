//! The EQ editor: a curve you drag (TDD §13.4).
//!
//! Geometry and arithmetic, pure, per §2.5 of `docs/first-usable-plan.md`.
//! Nothing here knows what a `Project` is — an editor is handed an
//! [`EqConfig`], which is the document's own type and not a view of it, for the
//! reason `fontelle_types::effect` gives.
//!
//! **Why a curve and not eight rows of numbers.** Both are the same amount of
//! code, and one of them is the control. An EQ is a shape, the shape is what a
//! person is deciding about, and the sum of eight bands is not something
//! anybody reads off a table. The curve drawn here is computed from the same
//! parameters the filter is built from — `EqBand::response_db` — and
//! `fontelle-fx`'s tests hold the two against each other, because a curve that
//! lies about the sound is worse than no curve: it is believed.

use crate::layout::Rect;
use crate::theme::Metrics;
use fontelle_types::{BandChannel, EqConfig};

/// The bottom of the frequency axis. Below this is not a note, it is a rumble
/// somebody wants gone, and a high-pass corner is set by ear rather than by
/// reading the number.
pub const EQ_MIN_HZ: f32 = 20.0;

/// The top. Nothing above it survives a 44.1 kHz render.
pub const EQ_MAX_HZ: f32 = 20_000.0;

/// How far up and down the display goes.
///
/// Wider than anyone should need on a mix bus, and deliberately: a cut that
/// runs off the bottom of its own display is one you cannot see the shape of.
pub const EQ_MAX_DB: f32 = 24.0;

/// The grab area of a band's handle.
const HANDLE_SIZE: f32 = 11.0;

/// How many points the curve is drawn with. One per two pixels at any width
/// worth looking at, which is smooth enough that the eye reads a line.
const CURVE_STEPS: usize = 160;

/// Where a frequency sits across `area`, on a log scale.
///
/// Log because that is how hearing works: the octave from 100 to 200 Hz has to
/// take as much width as the one from 5 to 10 kHz, or the half of the spectrum
/// where music lives is three pixels wide.
pub fn eq_x_of_freq(area: Rect, hz: f32) -> f32 {
    if area.width <= 0.0 {
        return area.x;
    }
    let span = (EQ_MAX_HZ / EQ_MIN_HZ).ln();
    let t = ((hz.clamp(EQ_MIN_HZ, EQ_MAX_HZ) / EQ_MIN_HZ).ln() / span).clamp(0.0, 1.0);
    area.x + t * area.width
}

/// The other direction, clamped into the axis: a drag off the end of the scale
/// is asking for the end.
pub fn eq_freq_at(area: Rect, x: f32) -> f32 {
    if area.width <= 0.0 {
        return EQ_MIN_HZ;
    }
    let t = ((x - area.x) / area.width).clamp(0.0, 1.0);
    let span = (EQ_MAX_HZ / EQ_MIN_HZ).ln();
    (EQ_MIN_HZ * (t * span).exp()).clamp(EQ_MIN_HZ, EQ_MAX_HZ)
}

/// Where a gain sits down `area`. Zero is the middle line and up is louder,
/// which is the one convention nobody has ever argued about.
pub fn eq_y_of_gain(area: Rect, db: f32) -> f32 {
    if area.height <= 0.0 {
        return area.y;
    }
    let t = (db.clamp(-EQ_MAX_DB, EQ_MAX_DB) + EQ_MAX_DB) / (EQ_MAX_DB * 2.0);
    area.bottom() - t * area.height
}

pub fn eq_gain_at(area: Rect, y: f32) -> f32 {
    if area.height <= 0.0 {
        return 0.0;
    }
    let t = ((area.bottom() - y) / area.height).clamp(0.0, 1.0);
    t * EQ_MAX_DB * 2.0 - EQ_MAX_DB
}

/// One band's handle on the curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EqHandle {
    /// Which of the eight bands, so an edit names the band rather than the
    /// handle's position in a list that changes as bands are switched on.
    pub band: usize,
    pub rect: Rect,
}

/// Where everything in the editor is.
#[derive(Debug, Clone, PartialEq)]
pub struct EqLayout {
    pub body: Rect,
    /// The curve's own area — what [`eq_x_of_freq`] and friends measure
    /// against.
    pub curve: Rect,
    /// A row under the curve for the selected band's type and channel.
    pub controls: Rect,
    /// A handle per **enabled** band. A switched-off band has no shape to
    /// grab, and eight handles on a flat line is eight things to knock.
    pub handles: Vec<EqHandle>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EqHit {
    Handle(usize),
    /// The curve itself — where a click adds a band, or clears the selection.
    Curve,
    Nothing,
}

pub fn eq_layout(body: Rect, metrics: &Metrics, config: &EqConfig) -> EqLayout {
    let controls_height = metrics.row_height.min(body.height.max(0.0));
    let curve = Rect::new(
        body.x,
        body.y,
        body.width,
        (body.height - controls_height).max(0.0),
    )
    .clamped();
    let controls = Rect::new(
        body.x,
        curve.bottom(),
        body.width,
        (body.bottom() - curve.bottom()).max(0.0),
    )
    .clamped();

    let handles = config
        .bands
        .iter()
        .enumerate()
        .filter(|(_, band)| band.enabled)
        .map(|(index, band)| {
            // A band with no gain — a pass filter, a notch — has a corner and
            // not a height, so its handle rides the zero line and moves only
            // sideways.
            let gain = if band.band_type.uses_gain() {
                band.gain_db
            } else {
                0.0
            };
            let cx = eq_x_of_freq(curve, band.freq_hz);
            let cy = eq_y_of_gain(curve, gain);
            EqHandle {
                band: index,
                rect: Rect::new(
                    cx - HANDLE_SIZE / 2.0,
                    cy - HANDLE_SIZE / 2.0,
                    HANDLE_SIZE,
                    HANDLE_SIZE,
                ),
            }
        })
        .collect();

    EqLayout {
        body,
        curve,
        controls,
        handles,
    }
}

pub fn eq_hit(layout: &EqLayout, x: f32, y: f32) -> EqHit {
    // Handles first, and the *last* one that contains the point: bands drawn
    // later sit on top, so the one on top is the one grabbed.
    if let Some(handle) = layout
        .handles
        .iter()
        .rev()
        .find(|handle| handle.rect.contains(x, y))
    {
        return EqHit::Handle(handle.band);
    }
    if layout.curve.contains(x, y) {
        return EqHit::Curve;
    }
    EqHit::Nothing
}

/// The curve, as points to draw a line through.
///
/// Evenly spaced in **pixels** rather than in frequency, so the line is smooth
/// where it is looked at rather than dense at one end.
pub fn eq_curve_points(layout: &EqLayout, config: &EqConfig) -> Vec<(f32, f32)> {
    let area = layout.curve;
    if area.width <= 0.0 || area.height <= 0.0 {
        return Vec::new();
    }
    (0..=CURVE_STEPS)
        .map(|step| {
            let x = area.x + area.width * step as f32 / CURVE_STEPS as f32;
            let db = config.response_db(eq_freq_at(area, x), BandChannel::Stereo);
            (x, eq_y_of_gain(area, db))
        })
        .collect()
}

/// What a scroll over a handle does to its Q, and the bounds it stops at.
///
/// Multiplicative rather than additive: Q is a ratio, and a step that takes
/// 0.5 to 1.5 would take 8 to 9. Bounded at each end because a Q of zero is
/// not a filter and a Q of a thousand is a sine wave.
pub fn eq_nudge_q(q: f32, steps: f32) -> f32 {
    (q * 1.2f32.powf(steps)).clamp(0.1, 24.0)
}

/// One insert, as a mixer strip's rack draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsertInfo {
    /// What the row says — [`EffectKind::label`](fontelle_types::EffectKind::label).
    pub label: String,
    pub bypassed: bool,
}

/// The menu the rack's `+ fx` row drops.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectMenu {
    pub frame: Rect,
    pub items: Vec<(fontelle_types::EffectKind, Rect)>,
}

/// A row per kind, from `EffectKind::ALL`.
///
/// One list rather than two: a new effect appears in this menu by *existing*,
/// not by somebody remembering to add it in a second place. That is the same
/// rule the roll's lane properties follow and the reason they have a `const`
/// list too.
pub fn effect_menu_layout(anchor: Rect, bounds: Rect, metrics: &Metrics) -> EffectMenu {
    const PAD: f32 = 3.0;
    const WIDTH: f32 = 108.0;

    let kinds = fontelle_types::EffectKind::ALL;
    let row = metrics.row_height.max(1.0);
    let height = row * kinds.len() as f32 + PAD * 2.0;
    let width = WIDTH.max(anchor.width);

    // Dropped downward from the row that opened it, and flipped upward when
    // there is no room — a menu that opens off the bottom of the window is a
    // menu you cannot use on the strip you most want it on.
    let x = anchor.x.min((bounds.right() - width).max(bounds.x));
    let below = anchor.bottom();
    let y = if below + height <= bounds.bottom() {
        below
    } else {
        (anchor.y - height).max(bounds.y)
    };
    let frame = Rect::new(x, y, width, height).intersection(&bounds);

    let items = kinds
        .iter()
        .enumerate()
        .map(|(index, kind)| {
            (
                *kind,
                Rect::new(
                    frame.x + PAD,
                    frame.y + PAD + index as f32 * row,
                    (frame.width - PAD * 2.0).max(0.0),
                    row,
                )
                .intersection(&frame),
            )
        })
        .collect();

    EffectMenu { frame, items }
}

pub fn effect_menu_hit(menu: &EffectMenu, x: f32, y: f32) -> Option<fontelle_types::EffectKind> {
    menu.items
        .iter()
        .find(|(_, rect)| rect.contains(x, y))
        .map(|(kind, _)| *kind)
}

/// One insert's controls, as the generic parameter panel draws them.
///
/// **Built from the effect's own parameter list**, which is the payoff of
/// §8.2's one addressing scheme: an effect that declares its parameters gets
/// an editor without anybody writing one, every control on it hands back the
/// address automation would use, and a new effect is editable the day its
/// `specs` table exists.
///
/// It is an [`InstrumentView`] because that is the panel this crate already
/// has for "a list of addressed parameters" — the instrument editor and the
/// effect editor are the same problem, and were always going to be.
pub fn effect_view(
    track_name: &str,
    slot: usize,
    track: fontelle_types::MixerTrackId,
    config: &fontelle_types::EffectConfig,
) -> super::InstrumentView {
    use fontelle_types::{ParamTarget, Taper, Unit};

    let params = config
        .specs()
        .iter()
        .map(|spec| {
            let value = config.get(spec.id).unwrap_or(spec.default);
            let kind = match (spec.unit, spec.taper) {
                (Unit::Switch, _) => super::ParamKind::Switch,
                (_, Taper::Stepped(steps)) => super::ParamKind::Choice(
                    (0..steps)
                        .map(|step| {
                            let at = spec.denormalise(step as f32 / (steps.max(2) - 1) as f32);
                            format!("{at:.0}")
                        })
                        .collect(),
                ),
                _ => super::ParamKind::Knob,
            };
            super::InstrumentParam {
                address: ParamTarget::Insert {
                    track,
                    slot,
                    param: spec.id.to_string(),
                }
                .address(),
                label: spec.name.to_string(),
                value: spec.normalise(value),
                display: format_value(value, spec.unit),
                kind,
            }
        })
        .collect();

    super::InstrumentView {
        title: format!("{track_name} \u{2014} {}", config.kind().label()),
        groups: vec![super::InstrumentGroup {
            name: config.kind().label().to_string(),
            params,
        }],
    }
}

/// A parameter's value with its unit on it. A knob whose number you cannot
/// read is a knob you cannot set.
fn format_value(value: f32, unit: fontelle_types::Unit) -> String {
    use fontelle_types::Unit;
    match unit {
        Unit::Switch => if value >= 0.5 { "on" } else { "off" }.to_string(),
        Unit::Hertz if value >= 1_000.0 => format!("{:.2} kHz", value / 1_000.0),
        Unit::Seconds if value >= 1_000.0 => format!("{:.2} s", value / 1_000.0),
        // The times are stored in milliseconds and read better that way until
        // they get long.
        Unit::Seconds => format!("{value:.1} ms"),
        Unit::None => format!("{value:.2}"),
        _ => format!("{value:.1}{}", unit.suffix()),
    }
}

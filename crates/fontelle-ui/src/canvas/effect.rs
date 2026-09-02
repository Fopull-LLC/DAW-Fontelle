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
use fontelle_types::{BANDS, BandChannel, BandType, EqConfig};

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

/// How many bars the analyser is drawn with.
///
/// One per two or three pixels at the width an EQ window opens at, which is
/// smooth enough to read as a curve and coarse enough that the bottom octave —
/// where the transform has only a handful of bins — is not a staircase of
/// duplicates.
pub const SPECTRUM_BANDS: usize = 96;

/// The loudest and quietest the analyser draws, in dBFS.
///
/// Zero at the top of the plot and -90 at the bottom, which is the range a mix
/// actually occupies. It is **not** the ±24 dB the curve is drawn against:
/// those are two different quantities on one picture — a *gain* and a *level* —
/// and pretending they share an axis would make a 6 dB boost look like a 6 dB
/// signal.
pub const SPECTRUM_TOP_DB: f32 = 0.0;
pub const SPECTRUM_BOTTOM_DB: f32 = -90.0;

/// The analyser's bars as a filled outline across `area`, left to right.
///
/// The same shape [`eq_curve_points`] returns and for the same reason: the
/// renderer takes points and knows nothing about decibels.
///
/// An empty or all-silent spectrum returns **nothing**, so a stopped transport
/// draws no analyser at all rather than a flat line along the floor that reads
/// as a signal.
pub fn spectrum_points(area: Rect, bands: &[f32]) -> Vec<(f32, f32)> {
    if area.is_empty() || bands.len() < 2 {
        return Vec::new();
    }
    if bands.iter().all(|db| *db <= SPECTRUM_BOTTOM_DB + 0.01) {
        return Vec::new();
    }
    let span = SPECTRUM_TOP_DB - SPECTRUM_BOTTOM_DB;
    let y_of = |db: f32| {
        let level = ((db - SPECTRUM_BOTTOM_DB) / span).clamp(0.0, 1.0);
        area.bottom() - level * area.height
    };
    // Each band is drawn at its **low edge**, which is where
    // [`spectrum_band_hz`] says the band begins — so a peak sits under the
    // part of the curve that shapes it rather than half a band to one side.
    let mut points: Vec<(f32, f32)> = bands
        .iter()
        .enumerate()
        .map(|(index, db)| {
            let (low, _) = spectrum_band_hz(index);
            (eq_x_of_freq(area, low), y_of(*db))
        })
        .collect();
    // And the last band is carried to the right-hand edge: a polyline that
    // stopped one band short would leave a notch at 20 kHz that is a drawing
    // artefact rather than a hole in the signal.
    if let Some(last) = bands.last() {
        points.push((area.right(), y_of(*last)));
    }
    points
}

/// The frequency band `index` of [`SPECTRUM_BANDS`] covers, as `(low, high)` in
/// hertz.
///
/// Logarithmic, across the same axis the curve is drawn on — so a bar sits
/// under the part of the curve that shapes it, which is the whole point of
/// drawing the two together.
pub fn spectrum_band_hz(index: usize) -> (f32, f32) {
    let span = (EQ_MAX_HZ / EQ_MIN_HZ).ln();
    let at = |i: usize| EQ_MIN_HZ * ((i as f32 / SPECTRUM_BANDS as f32) * span).exp();
    (at(index), at(index + 1))
}

/// The grab area of a band's handle, and how big it is drawn.
///
/// Eighteen, up from eleven, and that was *"kinda easy to miss them"* from
/// somebody using the window. Eleven is under the 16 logical pixels every
/// desktop's guidelines put on a pointer target, and this particular target is
/// dragged in two axes at once and carries its band's number, which has to be
/// legible on it.
const HANDLE_SIZE: f32 = 18.0;

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

/// Where a band lands when it is switched on from its chip.
///
/// Spread across the spectrum rather than all at 1 kHz: eight bands stacked on
/// one frequency are eight handles on top of each other, and the first thing
/// anybody does after switching one on is drag it somewhere anyway. These are
/// roughly log-even from the bottom of the range to the top, which is where a
/// mixing engineer would put eight bands before hearing the track.
const BAND_HOMES: [f32; BANDS] = [
    60.0, 150.0, 350.0, 800.0, 1_800.0, 4_000.0, 9_000.0, 15_000.0,
];

/// Where band `index` sits when nothing has moved it.
pub fn band_home_hz(index: usize) -> f32 {
    BAND_HOMES[index.min(BANDS - 1)]
}

/// One control in the row under the curve.
///
/// An enum rather than eight named rectangles, because every one of them is
/// pressed, dragged, scrolled and right-clicked the same way — and because
/// §12.4's "right-click any control to automate it" needs a control to be a
/// *value*, not a place on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EqField {
    /// Bell, shelf, pass, notch — press to step through them.
    Type,
    /// The band's corner or centre. Dragged.
    Freq,
    /// How much it lifts or cuts. Dragged. Blank on a band that has no gain.
    Gain,
    /// How wide it is. Dragged.
    Q,
    /// Stereo, mid or side — press to step through them.
    Channel,
    /// Listen to this band's region on its own.
    Solo,
    /// Switch this band off, which is what "remove" means on a fixed eight.
    Delete,
    /// The whole effect's wet/dry, which is not a band's but belongs on the
    /// same row: it is the other thing you reach for while listening.
    Mix,
}

impl EqField {
    /// The parameter this control moves, as §8.2 addresses one — which is what
    /// makes a right-click on it an automation lane.
    ///
    /// `None` for the two that are not parameters: soloing is a listening aid
    /// and deleting is not a value.
    pub fn param(self, band: usize) -> Option<String> {
        let field = match self {
            Self::Type => "type",
            Self::Freq => "freq",
            Self::Gain => "gain",
            Self::Q => "q",
            Self::Channel => "channel",
            Self::Mix => return Some(fontelle_types::MIX.to_string()),
            Self::Solo | Self::Delete => return None,
        };
        Some(format!("band{}.{field}", band + 1))
    }

    /// Whether a drag up and down moves it. The rest are pressed.
    pub fn is_dragged(self) -> bool {
        matches!(self, Self::Freq | Self::Gain | Self::Q | Self::Mix)
    }

    /// What a hover tip says.
    pub fn tip(self) -> &'static str {
        match self {
            Self::Type => "What shape this band is \u{2014} click to step, right-click to automate",
            Self::Freq => "Where the band sits. Drag, or drag its handle on the curve",
            Self::Gain => "How much it lifts or cuts",
            Self::Q => "How wide it is \u{2014} drag, or scroll over the handle",
            Self::Channel => "Which part of the stereo image this band works on",
            Self::Solo => "Listen to this band's region on its own",
            Self::Delete => "Switch this band off, keeping its settings",
            Self::Mix => "Wet/dry: blend the whole EQ with the sound that went into it",
        }
    }

    /// Every control in the row, in the order it is laid out.
    pub const ALL: [Self; 8] = [
        Self::Type,
        Self::Freq,
        Self::Gain,
        Self::Q,
        Self::Channel,
        Self::Solo,
        Self::Delete,
        Self::Mix,
    ];
}

/// Where everything in the editor is.
#[derive(Debug, Clone, PartialEq)]
pub struct EqLayout {
    pub body: Rect,
    /// The curve's own area — what [`eq_x_of_freq`] and friends measure
    /// against.
    pub curve: Rect,
    /// The two rows under the curve: the band chips and the controls.
    pub controls: Rect,
    /// A handle per **enabled** band. A switched-off band has no shape to
    /// grab, and eight handles on a flat line is eight things to knock.
    pub handles: Vec<EqHandle>,
    /// A chip per band, switched on or not, in band order.
    ///
    /// This is what makes a fresh EQ usable: eight bands that are all off draw
    /// no handles at all, and *"i just see a flat line"* is what that looks
    /// like from the other side of the screen.
    pub bands: Vec<(usize, Rect)>,
    /// The selected band's controls, and the effect's own wet/dry.
    pub fields: Vec<(EqField, Rect)>,
    /// Which band the controls describe.
    pub selected: usize,
}

impl EqLayout {
    /// Where one control is, if the editor had room for it.
    pub fn field(&self, field: EqField) -> Option<Rect> {
        self.fields
            .iter()
            .find(|(which, _)| *which == field)
            .map(|(_, rect)| *rect)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EqHit {
    Handle(usize),
    /// One of the eight band chips: select it, and switch it on if it is off.
    Band(usize),
    /// One of the selected band's controls.
    Field(EqField),
    /// The curve itself — where a click adds a band, or clears the selection.
    Curve,
    Nothing,
}

/// How tall the chip row and the control row are, as multiples of a text row.
const CHIP_ROW: f32 = 1.0;
const FIELD_ROW: f32 = 1.0;

/// How the control row's width is shared out. Relative rather than absolute so
/// the row fills whatever the window is dragged to.
const FIELD_WEIGHTS: [(EqField, f32); 8] = [
    (EqField::Type, 1.5),
    (EqField::Freq, 1.2),
    (EqField::Gain, 1.1),
    (EqField::Q, 1.0),
    (EqField::Channel, 1.0),
    (EqField::Solo, 0.6),
    (EqField::Delete, 0.6),
    (EqField::Mix, 1.0),
];

/// The editor with band 0's controls showing. See [`eq_layout_for`].
pub fn eq_layout(body: Rect, metrics: &Metrics, config: &EqConfig) -> EqLayout {
    eq_layout_for(body, metrics, config, 0)
}

/// The editor, with `selected`'s controls in the row under the curve.
pub fn eq_layout_for(
    body: Rect,
    metrics: &Metrics,
    config: &EqConfig,
    selected: usize,
) -> EqLayout {
    let row = metrics.row_height;
    // The rows come off the bottom before the curve is measured, so the curve
    // never has to be shrunk after the fact — the same order the roll's
    // property lane is taken in.
    let wanted = row * (CHIP_ROW + FIELD_ROW);
    let controls_height = wanted.min((body.height * 0.5).max(0.0));
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

    let chip_height = (controls.height * CHIP_ROW / (CHIP_ROW + FIELD_ROW)).max(0.0);
    let (chips_row, fields_row) = controls.split_top(chip_height);

    // A chip per band, evenly across the row: eight numbered switches, which
    // is how every parametric EQ says how many bands it has.
    let chip_pad = 2.0;
    // Capped: a numbered switch does not get wider by being given more room,
    // and eight buttons stretched across a 700-pixel window read as a segmented
    // control rather than as the eight bands they are.
    const CHIP_MAX: f32 = 40.0;
    let chip_width = ((chips_row.width - chip_pad) / BANDS as f32 - chip_pad)
        .clamp(0.0, CHIP_MAX);
    let bands = (0..BANDS)
        .map(|index| {
            let x = chips_row.x + chip_pad + index as f32 * (chip_width + chip_pad);
            (
                index,
                Rect::new(x, chips_row.y + 1.0, chip_width, (chips_row.height - 2.0).max(0.0))
                    .intersection(&chips_row),
            )
        })
        .collect();

    let total: f32 = FIELD_WEIGHTS.iter().map(|(_, weight)| weight).sum();
    let mut x = fields_row.x;
    let mut fields = Vec::with_capacity(FIELD_WEIGHTS.len());
    for (field, weight) in FIELD_WEIGHTS {
        let width = (fields_row.width * weight / total).max(0.0);
        fields.push((
            field,
            Rect::new(x + 1.0, fields_row.y + 1.0, (width - 2.0).max(0.0), (fields_row.height - 2.0).max(0.0))
                .intersection(&fields_row),
        ));
        x += width;
    }

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
        bands,
        fields,
        selected: selected.min(BANDS - 1),
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
    if let Some((index, _)) = layout
        .bands
        .iter()
        .find(|(_, chip)| chip.contains(x, y))
    {
        return EqHit::Band(*index);
    }
    if let Some((field, _)) = layout
        .fields
        .iter()
        .find(|(_, rect)| rect.contains(x, y))
    {
        return EqHit::Field(*field);
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

/// One band's own response, as points to draw a line through.
///
/// What every parametric EQ shows behind the sum: the band you are holding,
/// picked out of the shape everything adds up to, so a cut you are making is
/// visible against the curve it is being made in.
pub fn eq_band_curve_points(
    layout: &EqLayout,
    config: &EqConfig,
    band: usize,
) -> Vec<(f32, f32)> {
    let area = layout.curve;
    let Some(band) = config.bands.get(band).copied() else {
        return Vec::new();
    };
    if area.width <= 0.0 || area.height <= 0.0 {
        return Vec::new();
    }
    (0..=CURVE_STEPS)
        .map(|step| {
            let x = area.x + area.width * step as f32 / CURVE_STEPS as f32;
            (x, eq_y_of_gain(area, band.response_db(eq_freq_at(area, x))))
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

/// What a drag of `steps` does to a band's frequency.
///
/// A ratio, like the Q and for the same reason: one notch at 100 Hz has to be
/// the same musical move as one notch at 10 kHz. A control that added hertz
/// would be unusable at the bottom of the range and useless at the top.
pub fn eq_nudge_freq(hz: f32, steps: f32) -> f32 {
    (hz * 1.03f32.powf(steps)).clamp(EQ_MIN_HZ, EQ_MAX_HZ)
}

/// And to its gain. A difference, because decibels are one.
pub fn eq_nudge_gain(db: f32, steps: f32) -> f32 {
    (db + steps * 0.5).clamp(-EQ_MAX_DB, EQ_MAX_DB)
}

/// And to the effect's wet/dry, in hundredths.
pub fn eq_nudge_mix(mix: f32, steps: f32) -> f32 {
    (mix + steps * 0.01).clamp(0.0, 1.0)
}

/// The next band type, forwards or back — what a press on the type control
/// does. Wraps, because a chooser that stops at the end is one you have to
/// know the length of.
pub fn next_band_type(current: BandType, forward: bool) -> BandType {
    step_through(&BandType::ALL, current, forward)
}

/// The same, for which part of the stereo image a band works on.
pub fn next_band_channel(current: BandChannel, forward: bool) -> BandChannel {
    step_through(&BandChannel::ALL, current, forward)
}

fn step_through<T: Copy + PartialEq>(all: &[T], current: T, forward: bool) -> T {
    let len = all.len();
    let index = all.iter().position(|item| *item == current).unwrap_or(0);
    let next = if forward {
        (index + 1) % len
    } else {
        (index + len - 1) % len
    };
    all[next]
}

/// What one control says it is set to.
///
/// The em dash rather than a number where a parameter does not apply: a
/// low-pass has a corner and not a gain, and a read-out saying `0.0 dB` on one
/// is a control that looks live and moves nothing.
pub fn eq_field_caption(field: EqField, config: &EqConfig, band: usize) -> String {
    let Some(band) = config.bands.get(band) else {
        return String::new();
    };
    match field {
        EqField::Type => band.band_type.label().to_string(),
        EqField::Freq => format_hz(band.freq_hz),
        EqField::Gain if !band.band_type.uses_gain() => "\u{2014}".to_string(),
        EqField::Gain => format!("{:.1} dB", band.gain_db),
        EqField::Q => format!("Q {:.2}", band.q),
        EqField::Channel => band.channel.label().to_string(),
        EqField::Solo => "solo".to_string(),
        EqField::Delete => "off".to_string(),
        EqField::Mix => format!("{}%", (config.mix.clamp(0.0, 1.0) * 100.0).round() as i32),
    }
}

/// A frequency as an EQ writes one: hertz down low, kilohertz up high, and
/// never more digits than the ear can tell apart.
pub fn format_hz(hz: f32) -> String {
    if hz >= 1_000.0 {
        format!("{:.2} kHz", hz / 1_000.0)
    } else {
        format!("{hz:.0} Hz")
    }
}

/// One insert, as a mixer strip's rack draws it.
#[derive(Debug, Clone, PartialEq)]
pub struct InsertInfo {
    /// What the row says — [`EffectKind::label`](fontelle_types::EffectKind::label).
    pub label: String,
    pub bypassed: bool,
    /// Where its wet/dry knob is, 0 (the signal that went in) to 1 (the
    /// effect). Every effect has one — see
    /// [`EffectConfig::mix`](fontelle_types::EffectConfig::mix).
    pub mix: f32,
    /// Whether an automation lane owns that knob, so the rack's dial can wear
    /// the same ring the effect's own panel gives it (TDD §12.2).
    ///
    /// The wet/dry is automatable like every other parameter, and a ring that
    /// appeared on one of the two places it is drawn and not the other would
    /// be a worse answer than neither.
    pub mix_automated: bool,
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
#[allow(clippy::too_many_arguments)]
pub fn effect_view(
    track_name: &str,
    slot: usize,
    track: fontelle_types::MixerTrackId,
    config: &fontelle_types::EffectConfig,
    strips: &[String],
    key: Option<usize>,
) -> super::InstrumentView {
    use fontelle_types::{ParamTarget, Taper, Unit};

    let params: Vec<super::InstrumentParam> = config
        .specs()
        .iter()
        .map(|spec| {
            let value = config.get(spec.id).unwrap_or(spec.default);
            let kind = match (spec.unit, spec.taper) {
                (Unit::Switch, _) => super::ParamKind::Switch,
                // A chooser says what its positions are **called** when the
                // parameter names them, and counts otherwise: "Peak"/"RMS"
                // rather than "0"/"1", which is a control nobody can set on
                // purpose. See `ParamSpec::positions`.
                (_, Taper::Stepped(_)) if !spec.positions.is_empty() => {
                    super::ParamKind::Choice(
                        spec.positions.iter().map(|name| name.to_string()).collect(),
                    )
                }
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
                display: display_of(spec, value),
                kind,
                // Whoever built this knows which lanes exist; see
                // `InstrumentView::mark_automated`.
                automated: false,
            }
        })
        .collect();

    // One heading per section the config declares, each taking the next run
    // of the table. The split is the document's, not the window's: fourteen
    // knobs in one grid is a panel nobody can read, and the place that
    // knows which five are the drive is the place that lists them.
    let mut params = params.into_iter();
    let groups = config
        .sections()
        .iter()
        .map(|section| super::InstrumentGroup {
            name: section.name.to_string(),
            params: params.by_ref().take(section.count).collect(),
        })
        .collect();

    super::InstrumentView {
        title: format!("{track_name} \u{2014} {}", config.kind().label()),
        // The named starting points, straight from the config. Nothing here
        // knows what any of them mean — clicking one hands the index back and
        // `EffectConfig::apply_preset` is what writes the knobs.
        presets: config.presets().iter().map(|name| (*name).to_string()).collect(),
        // What the detector can be pointed at, when there is one: "no key"
        // and then every strip. The list is the window's rather than the
        // config's, because which tracks exist is not something an effect
        // knows — see `InstrumentView::keys`.
        keys: if config.kind().takes_key() {
            std::iter::once(NO_KEY.to_string())
                .chain(strips.iter().cloned())
                .collect()
        } else {
            Vec::new()
        },
        key: config
            .kind()
            .takes_key()
            .then(|| key.map_or(0, |strip| strip + 1)),
        groups,
    }
}

/// The first chip in the key row: listening to the signal passing through the
/// insert, which is what every compressor did before keys existed.
pub const NO_KEY: &str = "no key";

/// A parameter's value with its unit on it. A knob whose number you cannot
/// read is a knob you cannot set.
/// The read-out under one of an effect's controls.
///
/// A named position reads as its name — the same word the chooser above it
/// shows, because a control whose read-out disagrees with its own chooser is a
/// control you cannot trust.
fn display_of(spec: &fontelle_types::ParamSpec, value: f32) -> String {
    if !spec.positions.is_empty() {
        let at = (spec.normalise(value) * (spec.positions.len() - 1) as f32).round() as usize;
        if let Some(name) = spec.positions.get(at.min(spec.positions.len() - 1)) {
            return (*name).to_string();
        }
    }
    format_value(value, spec.unit)
}

fn format_value(value: f32, unit: fontelle_types::Unit) -> String {
    use fontelle_types::Unit;
    match unit {
        Unit::Switch => if value >= 0.5 { "on" } else { "off" }.to_string(),
        Unit::Hertz if value >= 1_000.0 => format!("{:.2} kHz", value / 1_000.0),
        // A millisecond value long enough to be seconds reads as seconds: a
        // five-second release is not "5000.0 ms" to anybody.
        Unit::Milliseconds if value >= 1_000.0 => format!("{:.2} s", value / 1_000.0),
        Unit::Milliseconds => format!("{value:.1} ms"),
        Unit::Seconds => format!("{value:.2} s"),
        Unit::None => format!("{value:.2}"),
        _ => format!("{value:.1}{}", unit.suffix()),
    }
}

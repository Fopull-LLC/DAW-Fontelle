//! Lapse's window: the memory across the top, the lanes under it, the console
//! under those (`docs/lapse-plan.md` §7).
//!
//! # The one idea
//!
//! **The slope is the sound.** A segment of the time lane falling at one
//! lane-length per lane holds the sound still; half that plays it an octave
//! down; steeper plays it backwards. That is not a setting to be found in a
//! menu, it is the *shape of the line* — so the grid draws the freeze slope
//! as a guide at every beat, the tool bar has a pencil that lays one down,
//! and the read-out under the cursor says what the slope there is worth in
//! semitones. Somebody who has never seen this kind of plugin can draw a tape
//! stop in ten seconds because the picture tells them how.
//!
//! # What is here and what is not
//!
//! Pure geometry and pure text. No `Scene`, no shaping (INVARIANT 2), and
//! every rule below is checked without a window (`fontelle-ui/tests/lapse.rs`).
//! The drawing is `render/`'s and the document half is `fontelle-app`'s.

use fontelle_types::{CurveShape, LapseConfig, LapseLaneKind, LapseLength, LapsePoint};

use crate::layout::Rect;

/// How tall the memory canopy is at 100 %.
pub const LAPSE_CANOPY: f32 = 96.0;

/// The window's design size. Nothing on it shrinks, so this is also its
/// minimum — Flopsynth's rule (`docs/flopsynth-next.md` §3.1).
pub const LAPSE_SIZE: (u32, u32) = (1180, 760);

/// What the window shows of one Lapse insert.
#[derive(Debug, Clone, PartialEq)]
pub struct LapseView {
    /// The strip this is an insert on — what the window calls itself, so two
    /// open on two tracks are tellable apart on the desktop.
    pub track: String,
    pub config: LapseConfig,
    /// The scene being drawn, which is the one playing.
    pub scene: usize,
    /// What each scene is called, or empty for the ones nobody named.
    pub scene_names: Vec<String>,
    /// Which scenes have anything drawn on them, so the chips can say so
    /// without the window reading the bank.
    pub scene_used: Vec<bool>,
    /// The four lanes of the showing scene.
    pub lanes: Vec<LaneView>,
    /// Where the pattern is, 0..1 round the time lane — the playhead.
    pub phase: f32,
    /// Where the read head is, in lane-lengths, so it lands on the grid.
    pub offset: f32,
    /// How fast the memory is being read. 1 is live, 0 is frozen.
    pub rate: f32,
    /// Whether the read is against the end of what has been written.
    pub clamped: bool,
    /// Seconds of memory actually written.
    pub filled_seconds: f32,
    /// The memory's peak envelope, oldest first.
    pub memory: Vec<(f32, f32)>,
    /// The beats in a bar, for the grid's own divisions.
    pub beats_per_bar: u32,
    /// The tempo, which is what turns a lane-length into seconds — the
    /// canopy draws the memory in time and the lanes draw it in bars.
    pub bpm: f32,
    /// Which tool the pointer is holding. Window state, not the document's —
    /// a tool is how the picture is *drawn on*, like `WaveTool`.
    pub tool: LapseTool,
    /// What the drawing snaps to.
    pub snap: LapseSnap,
    /// How far the lane's value axis reaches, in lane-lengths.
    ///
    /// One is the whole of it — the lane's value *is* one lane-length either
    /// way — and it is the default because at that reach the freeze is a true
    /// 45° line and the grid teaches it. Smaller magnifies: a quarter shows
    /// the region a groove template lives in, where the offsets are
    /// milliseconds. It never goes above one, because there is nothing above
    /// one to show and a grid with dead space at the bottom is a grid where
    /// half the clicks do nothing.
    pub zoom: f32,
}

/// One lane, as the window draws it.
#[derive(Debug, Clone, PartialEq)]
pub struct LaneView {
    pub kind: LapseLaneKind,
    pub length: LapseLength,
    pub on: bool,
    pub points: Vec<LapsePoint>,
    /// Collapsed to a single row until somebody switches it on: an off lane
    /// costs one row of chrome and no attention.
    pub open: bool,
}

/// What a drag on the grid does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LapseTool {
    /// Points: click to add, drag to move, drag a segment to bend it.
    #[default]
    Points,
    /// Free-hand.
    Pencil,
    /// A straight segment from press to release.
    Line,
    /// **The freeze**, laid down at exactly the slope that holds the sound
    /// still — the one-gesture tape stop, and the thing that makes this
    /// window teach itself.
    Hold,
    /// Steps at the snap division: trance-gate drawing.
    Step,
}

impl LapseTool {
    pub const ALL: [Self; 5] = [
        Self::Points,
        Self::Pencil,
        Self::Line,
        Self::Hold,
        Self::Step,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Points => "points",
            Self::Pencil => "pencil",
            Self::Line => "line",
            Self::Hold => "hold",
            Self::Step => "step",
        }
    }

    /// One sentence, for the tooltip. Every hit kind has one
    /// (`docs/flopsynth-next.md` §3.3).
    pub fn tip(self) -> &'static str {
        match self {
            Self::Points => "Click to add a point, drag one to move it, drag a segment to bend it",
            Self::Pencil => "Draw freehand",
            Self::Line => "Drag a straight segment",
            Self::Hold => "Drag the freeze: the sound stops where you start and waits",
            Self::Step => "Paint steps on the snap division",
        }
    }
}

/// What a drawn point snaps to along the lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LapseSnap {
    Quarter,
    Eighth,
    #[default]
    Sixteenth,
    ThirtySecond,
    Triplet,
    Off,
}

impl LapseSnap {
    pub const ALL: [Self; 6] = [
        Self::Quarter,
        Self::Eighth,
        Self::Sixteenth,
        Self::ThirtySecond,
        Self::Triplet,
        Self::Off,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Quarter => "1/4",
            Self::Eighth => "1/8",
            Self::Sixteenth => "1/16",
            Self::ThirtySecond => "1/32",
            Self::Triplet => "1/12",
            Self::Off => "off",
        }
    }

    /// How many divisions there are in one **beat**, or `None` for off.
    pub fn per_beat(self) -> Option<f64> {
        Some(match self {
            Self::Quarter => 1.0,
            Self::Eighth => 2.0,
            Self::Sixteenth => 4.0,
            Self::ThirtySecond => 8.0,
            Self::Triplet => 3.0,
            Self::Off => return None,
        })
    }

    /// Snaps a phase to this division of a lane `beats` long.
    pub fn snap(self, phase: f64, beats: f64) -> f64 {
        let Some(per_beat) = self.per_beat() else {
            return phase.clamp(0.0, 1.0);
        };
        let divisions = (per_beat * beats).max(1.0);
        ((phase * divisions).round() / divisions).clamp(0.0, 1.0)
    }
}

impl LapseView {
    /// How long the showing scene's time lane is, in beats — what the grid's
    /// divisions and the snap are measured in.
    pub fn time_beats(&self) -> f64 {
        self.lanes
            .first()
            .map(|lane| lane.length.beats(self.beats_per_bar) * self.config.rate.stretch())
            .unwrap_or(4.0)
    }

    /// What the rate read-out says: the playback speed as a pitch.
    ///
    /// `rate = 1 + dv/dp`, and a rate is a pitch — twelve times its log. A
    /// musician can aim at "−7.0 st"; nobody can aim at "slope −0.33".
    pub fn rate_caption(&self) -> String {
        rate_caption(self.rate)
    }

    /// Which lanes are drawn open, in order.
    pub fn open_lanes(&self) -> impl Iterator<Item = (usize, &LaneView)> {
        self.lanes.iter().enumerate().filter(|(_, lane)| lane.open)
    }

    /// What a scene chip says: its name, or its number.
    pub fn scene_label(&self, scene: usize) -> String {
        match self.scene_names.get(scene) {
            Some(name) if !name.is_empty() => name.clone(),
            _ => format!("{}", scene + 1),
        }
    }
}

/// The rate read-out, as a caption.
///
/// Zero rate is a freeze rather than an infinitely deep pitch, and a negative
/// one is *backwards* rather than a pitch at all: taking the log of either
/// would put "-inf st" and "NaN" on the face of a musical instrument.
pub fn rate_caption(rate: f32) -> String {
    if rate.abs() < 0.01 {
        return "frozen".to_string();
    }
    let semitones = 12.0 * rate.abs().log2();
    if rate < 0.0 {
        format!("reverse {semitones:+.1} st")
    } else {
        format!("{semitones:+.1} st")
    }
}

/// Where everything is, once the window has a size.
#[derive(Debug, Clone, PartialEq)]
pub struct LapseLayout {
    pub body: Rect,
    pub canopy: Rect,
    /// One per lane, in lane order; a collapsed lane gets a one-row strip.
    pub lanes: Vec<Rect>,
    /// The scene chips, twelve of them.
    pub scenes: Vec<Rect>,
    /// The tool chips.
    pub tools: Vec<Rect>,
    /// The snap chips.
    pub snaps: Vec<Rect>,
    pub console: Rect,
    /// The generic knob panel's cells, laid out by the caller.
    pub controls: Vec<Rect>,
}

impl Default for LapseLayout {
    fn default() -> Self {
        Self {
            body: Rect::ZERO,
            canopy: Rect::ZERO,
            lanes: Vec::new(),
            scenes: Vec::new(),
            tools: Vec::new(),
            snaps: Vec::new(),
            console: Rect::ZERO,
            controls: Vec::new(),
        }
    }
}

/// How tall an open lane is, and a collapsed one.
const LANE_HEIGHT: f32 = 132.0;
const LANE_STRIP: f32 = 26.0;
const LANE_GAP: f32 = 8.0;
/// The column down the right of the lanes, holding the twelve scene chips.
///
/// The tools are **not** in it: five chips in this width is 29 px each, which
/// is narrower than the word "pencil" and draws as a row of empty boxes. They
/// have a bar of their own under the canopy, where there is room to say what
/// they are — found by looking at the window rather than at the layout.
const ASIDE: f32 = 196.0;
const PADDING: f32 = 12.0;
const TOOLBAR_HEIGHT: f32 = 26.0;
const CONSOLE_HEIGHT: f32 = 196.0;
/// One console cell: a caption (14), a control (32) and a read-out (14),
/// with room between them. Measured against the window rather than guessed —
/// the first draft was 58 and the captions of the second row landed on the
/// read-outs of the first.
const CELL: (f32, f32) = (108.0, 76.0);

/// Lays the window out at `body`.
pub fn lapse_layout(view: &LapseView, body: Rect) -> LapseLayout {
    let mut layout = LapseLayout {
        body,
        ..Default::default()
    };
    layout.canopy = Rect {
        x: body.x + PADDING,
        y: body.y + PADDING,
        width: (body.width - PADDING * 2.0).max(0.0),
        height: LAPSE_CANOPY,
    };

    // The tool bar, across the top of the lanes.
    let toolbar_y = layout.canopy.y + layout.canopy.height + LANE_GAP;
    let mut x = body.x + PADDING;
    for tool in LapseTool::ALL {
        let width = chip_width(tool.label());
        layout.tools.push(Rect {
            x,
            y: toolbar_y,
            width,
            height: TOOLBAR_HEIGHT,
        });
        x += width + 4.0;
    }
    x += 16.0;
    for snap in LapseSnap::ALL {
        let width = chip_width(snap.label());
        layout.snaps.push(Rect {
            x,
            y: toolbar_y,
            width,
            height: TOOLBAR_HEIGHT,
        });
        x += width + 4.0;
    }

    let lanes_top = toolbar_y + TOOLBAR_HEIGHT + LANE_GAP;
    let console_top = body.y + body.height - CONSOLE_HEIGHT;
    layout.console = Rect {
        x: body.x + PADDING,
        y: console_top,
        width: (body.width - PADDING * 2.0).max(0.0),
        height: (CONSOLE_HEIGHT - PADDING).max(0.0),
    };

    let lane_width = (body.width - PADDING * 2.0 - ASIDE - LANE_GAP).max(0.0);
    let mut y = lanes_top;
    for lane in &view.lanes {
        let height = if lane.open { LANE_HEIGHT } else { LANE_STRIP };
        layout.lanes.push(Rect {
            x: body.x + PADDING,
            y,
            width: lane_width,
            height,
        });
        y += height + LANE_GAP;
    }

    // The aside: twelve scene chips in a grid of three, tall enough to hold
    // a name. Flopsynth's aside column and its rule — the column is a fixed
    // width and the lanes take what is left.
    let aside_x = body.x + PADDING + lane_width + LANE_GAP;
    let chip_w = (ASIDE - LANE_GAP * 2.0) / 3.0;
    let chip_h = 38.0;
    for scene in 0..view.scene_names.len().max(1) {
        let (row, column) = (scene / 3, scene % 3);
        layout.scenes.push(Rect {
            x: aside_x + column as f32 * (chip_w + LANE_GAP),
            y: lanes_top + row as f32 * (chip_h + LANE_GAP),
            width: chip_w,
            height: chip_h,
        });
    }

    // The console: one cell per parameter, wrapped into rows.
    let specs = fontelle_types::EffectConfig::Lapse(view.config);
    let per_row = ((layout.console.width / CELL.0).floor() as usize).max(1);
    for (index, _) in specs.specs().iter().enumerate() {
        let (row, column) = (index / per_row, index % per_row);
        layout.controls.push(Rect {
            x: layout.console.x + column as f32 * CELL.0,
            y: layout.console.y + row as f32 * CELL.1,
            width: CELL.0 - 6.0,
            height: CELL.1 - 6.0,
        });
    }
    layout
}

/// How wide a chip has to be for its word.
///
/// Measured rather than shaped: this crate may not shape text (INVARIANT 2),
/// so a chip's width is its characters times a conservative advance, which is
/// what every other chip row here does.
fn chip_width(label: &str) -> f32 {
    (label.chars().count() as f32 * 7.5 + 20.0).max(40.0)
}

/// What the pointer is over.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LapseHit {
    /// Inside a lane's grid, at this phase and value.
    Grid {
        lane: usize,
        phase: f64,
        value: f64,
    },
    /// On one of a lane's points.
    Point {
        lane: usize,
        index: usize,
    },
    /// On the strip that opens or closes a lane.
    LaneStrip {
        lane: usize,
    },
    Scene(usize),
    Tool(LapseTool),
    Snap(LapseSnap),
    /// One of the console's parameters, by its place in
    /// `EffectConfig::specs`.
    Control(usize),
    Canopy,
}

/// The corner of an open lane that carries its name, and switches it off.
pub const LANE_NAME: (f32, f32) = (72.0, 18.0);

/// How close to a point the pointer has to be to grab it, in pixels.
pub const LAPSE_GRAB: f32 = 9.0;

/// What is under `(x, y)`.
pub fn lapse_hit(layout: &LapseLayout, view: &LapseView, x: f32, y: f32) -> Option<LapseHit> {
    if layout.canopy.contains(x, y) {
        return Some(LapseHit::Canopy);
    }
    for (index, rect) in layout.scenes.iter().enumerate() {
        if rect.contains(x, y) {
            return Some(LapseHit::Scene(index));
        }
    }
    for (index, rect) in layout.tools.iter().enumerate() {
        if rect.contains(x, y) {
            return Some(LapseHit::Tool(LapseTool::ALL[index]));
        }
    }
    for (index, rect) in layout.snaps.iter().enumerate() {
        if rect.contains(x, y) {
            return Some(LapseHit::Snap(LapseSnap::ALL[index]));
        }
    }
    for (index, rect) in layout.controls.iter().enumerate() {
        if rect.contains(x, y) {
            return Some(LapseHit::Control(index));
        }
    }
    for (index, rect) in layout.lanes.iter().enumerate() {
        if !rect.contains(x, y) {
            continue;
        }
        let lane = view.lanes.get(index)?;
        if !lane.open {
            return Some(LapseHit::LaneStrip { lane: index });
        }
        // An open lane's **name** switches it off again, so the gesture works
        // both ways with one hit kind rather than one to open and none to
        // close.
        let name = Rect::new(rect.x, rect.y, LANE_NAME.0, LANE_NAME.1);
        if name.contains(x, y) {
            return Some(LapseHit::LaneStrip { lane: index });
        }
        // A point first: the grab radius beats the grid everywhere they
        // overlap, or a point could never be picked up off its own line.
        for (point_index, point) in lane.points.iter().enumerate() {
            let (px, py) = point_position(view, lane, *rect, point.at, point.value);
            if (px - x).hypot(py - y) <= LAPSE_GRAB {
                return Some(LapseHit::Point {
                    lane: index,
                    index: point_index,
                });
            }
        }
        let (phase, value) = grid_position(view, lane, *rect, x, y);
        return Some(LapseHit::Grid {
            lane: index,
            phase,
            value,
        });
    }
    None
}

/// Where a point sits in the lane's rectangle.
pub fn point_position(
    view: &LapseView,
    lane: &LaneView,
    rect: Rect,
    at: f64,
    value: f64,
) -> (f32, f32) {
    let x = rect.x + (at.clamp(0.0, 1.0) as f32) * rect.width;
    let y = rect.y + value_to_fraction(view, lane, value) * rect.height;
    (x, y)
}

/// And the other way: where the pointer is, in the lane's own units.
pub fn grid_position(view: &LapseView, lane: &LaneView, rect: Rect, x: f32, y: f32) -> (f64, f64) {
    let phase = if rect.width > 0.0 {
        ((x - rect.x) / rect.width).clamp(0.0, 1.0) as f64
    } else {
        0.0
    };
    let fraction = if rect.height > 0.0 {
        ((y - rect.y) / rect.height).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (phase, fraction_to_value(view, lane, fraction))
}

/// Where a value sits down the lane, 0 at the top.
///
/// The **time** lane puts zero at the top and the past below it, which is
/// what makes the freeze a line running down to the right — and it is why
/// the default zoom is one lane-length: at that reach the freeze is exactly
/// 45° and the grid teaches it.
fn value_to_fraction(view: &LapseView, lane: &LaneView, value: f64) -> f32 {
    match lane.kind {
        LapseLaneKind::Time => {
            let reach = view.zoom.max(0.01) as f64;
            // Zero at the top when nothing looks ahead, and the zero line
            // slides down to make room for the future when something does.
            let above = if view.config.look == fontelle_types::LapseLook::Off {
                0.0
            } else {
                reach * 0.25
            };
            let span = reach + above;
            (((above - value) / span).clamp(0.0, 1.0)) as f32
        }
        LapseLaneKind::Volume => (1.0 - value.clamp(0.0, 1.0)) as f32,
        _ => ((1.0 - value.clamp(-1.0, 1.0)) * 0.5) as f32,
    }
}

fn fraction_to_value(view: &LapseView, lane: &LaneView, fraction: f32) -> f64 {
    let fraction = fraction.clamp(0.0, 1.0) as f64;
    match lane.kind {
        LapseLaneKind::Time => {
            let reach = view.zoom.max(0.01) as f64;
            let above = if view.config.look == fontelle_types::LapseLook::Off {
                0.0
            } else {
                reach * 0.25
            };
            let span = reach + above;
            (above - fraction * span).clamp(-1.0, 1.0)
        }
        LapseLaneKind::Volume => 1.0 - fraction,
        _ => 1.0 - fraction * 2.0,
    }
}

/// The freeze slope, in the lane rectangle's own pixels: how far down the
/// grid a hold runs per pixel across it.
///
/// One at the default zoom, which is the whole point of that default — the
/// guide the grid draws is a true 45° line and a hold is something you trace.
pub fn freeze_slope(view: &LapseView, rect: Rect) -> f32 {
    if rect.width <= 0.0 {
        return 1.0;
    }
    let reach = view.zoom.max(0.01);
    let above = if view.config.look == fontelle_types::LapseLook::Off {
        0.0
    } else {
        reach * 0.25
    };
    let span = reach + above;
    (rect.height / span) / (rect.width / 1.0)
}

/// Which segment of a lane `phase` falls in, and how far the drawn curve is
/// from `value` there — the two numbers a *bend* gesture needs.
///
/// The segment is named by the point that **carries its shape**, which is the
/// point before it; the one that wraps round is the last. `None` for a lane
/// with nothing on it.
pub fn segment_at(lane: &LaneView, phase: f64, value: f64) -> Option<(usize, f64)> {
    if lane.points.is_empty() {
        return None;
    }
    let index = lane.points.partition_point(|p| p.at <= phase);
    let carrier = if index == 0 {
        lane.points.len() - 1
    } else {
        index - 1
    };
    let drawn = fontelle_types::curve_at(&lane.points, phase, lane.kind.neutral());
    Some((carrier, (drawn - value).abs()))
}

/// How close to the drawn curve a press has to be to bend it rather than to
/// add a point, in pixels.
pub const LAPSE_BEND_GRAB: f32 = 7.0;

/// Which way a bend runs on this segment.
///
/// `value = from + (to − from) · eased(t, tension)`, so on a **descending**
/// segment a positive tension pulls the curve *down*. Dragging up has to
/// raise the line whichever way it runs, or half the segments in a lane
/// would bend backwards under the hand.
pub fn bend_sign(lane: &LaneView, carrier: usize) -> f64 {
    let Some(from) = lane.points.get(carrier) else {
        return 1.0;
    };
    let to = lane.points.get(carrier + 1).or_else(|| lane.points.first());
    match to {
        Some(to) if to.value < from.value => -1.0,
        _ => 1.0,
    }
}

/// The points a **hold** drag lays down: the freeze from `from` to `to`.
///
/// Two points and the segment between them, at exactly the slope that holds
/// the sound still — `value` falls by the same fraction of a lane-length that
/// `phase` advances. The gesture is the whole feature.
pub fn hold_points(from: (f64, f64), to: (f64, f64)) -> [LapsePoint; 2] {
    let (start, end) = if from.0 <= to.0 {
        (from, to)
    } else {
        (to, from)
    };
    let fallen = (start.1 - (end.0 - start.0)).clamp(-1.0, 1.0);
    [
        LapsePoint::new(start.0, start.1, CurveShape::Linear),
        LapsePoint::new(end.0, fallen, CurveShape::Linear),
    ]
}

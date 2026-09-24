//! DisgustingBeat's window: the memory across the top, the lanes under it, the
//! console under those (`docs/disgusting-beat-plan.md` §7).
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
//! every rule below is checked without a window (`fontelle-ui/tests/disgusting_beat.rs`).
//! The drawing is `render/`'s and the document half is `fontelle-app`'s.

use fontelle_types::{
    CurveShape, DisgustingBeatConfig, DisgustingBeatLaneKind, DisgustingBeatLength,
    DisgustingBeatPoint,
};

use crate::layout::Rect;

/// How tall the memory canopy is at 100 %.
pub const DISGUSTING_BEAT_CANOPY: f32 = 96.0;

/// The window's design size. Nothing on it shrinks, so this is also its
/// minimum — Flopsynth's rule (`docs/flopsynth-next.md` §3.1).
pub const DISGUSTING_BEAT_SIZE: (u32, u32) = (1180, 760);

/// What the window shows of one DisgustingBeat insert.
#[derive(Debug, Clone, PartialEq)]
pub struct DisgustingBeatView {
    /// The strip this is an insert on — what the window calls itself, so two
    /// open on two tracks are tellable apart on the desktop.
    pub track: String,
    pub config: DisgustingBeatConfig,
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
    /// Where the read head has **been**, one entry per memory bucket and
    /// oldest first: how many buckets behind the write head the read was when
    /// that bucket was written.
    ///
    /// The bare head says where the sound is coming from *now*; the trail
    /// says what the curve has been doing to it, which is the thing this kind
    /// of plugin has never shown anybody. A freeze draws a line sloping away
    /// from the present, a stutter draws a saw, a reverse draws a line
    /// climbing back into the past.
    pub trail: Vec<f32>,
    /// The beats in a bar, for the grid's own divisions.
    pub beats_per_bar: u32,
    /// The tempo, which is what turns a lane-length into seconds — the
    /// canopy draws the memory in time and the lanes draw it in bars.
    pub bpm: f32,
    /// Which tool the pointer is holding. Window state, not the document's —
    /// a tool is how the picture is *drawn on*, like `WaveTool`.
    pub tool: DisgustingBeatTool,
    /// What the drawing snaps to.
    pub snap: DisgustingBeatSnap,
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
    /// The shape menu, when a point has one open. Window state, like the
    /// tool: what a point's curve *is* belongs to the document, and which
    /// menu happens to be on the screen does not.
    pub menu: Option<DisgustingBeatMenu>,
}

/// A menu of shapes, open on one point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DisgustingBeatMenu {
    pub lane: usize,
    pub index: usize,
    /// Where it was asked for, in the window's own coordinates. The layout
    /// moves it to keep it on the window.
    pub at: (f32, f32),
}

/// How far the lane's value axis is magnified.
///
/// Magnification rather than reach, because "4\u{d7}" is a word everybody
/// already has and "reach 0.25" is not. The lane then reaches a quarter of a
/// lane-length either way, which is where a groove template lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DisgustingBeatZoom {
    #[default]
    One,
    Two,
    Four,
    Eight,
}

impl DisgustingBeatZoom {
    pub const ALL: [Self; 4] = [Self::One, Self::Two, Self::Four, Self::Eight];

    pub fn label(self) -> &'static str {
        match self {
            Self::One => "1\u{d7}",
            Self::Two => "2\u{d7}",
            Self::Four => "4\u{d7}",
            Self::Eight => "8\u{d7}",
        }
    }

    /// How far the axis reaches, in lane-lengths, either way from neutral.
    pub fn reach(self) -> f32 {
        match self {
            Self::One => 1.0,
            Self::Two => 0.5,
            Self::Four => 0.25,
            Self::Eight => 0.125,
        }
    }

    pub fn tip(self) -> &'static str {
        match self {
            Self::One => "The whole lane-length: a freeze is a true 45\u{b0} line",
            Self::Two => "Half a lane-length either way",
            Self::Four => "A quarter: where a groove's push and drag live",
            Self::Eight => "An eighth, for the smallest offsets there are",
        }
    }

    /// Which chip is lit for a reach the document is already holding.
    pub fn nearest(reach: f32) -> Self {
        let mut best = Self::One;
        for zoom in Self::ALL {
            if (zoom.reach() - reach).abs() < (best.reach() - reach).abs() {
                best = zoom;
            }
        }
        best
    }
}

/// One lane, as the window draws it.
#[derive(Debug, Clone, PartialEq)]
pub struct LaneView {
    pub kind: DisgustingBeatLaneKind,
    pub length: DisgustingBeatLength,
    pub on: bool,
    pub points: Vec<DisgustingBeatPoint>,
    /// Collapsed to a single row until somebody switches it on: an off lane
    /// costs one row of chrome and no attention.
    pub open: bool,
}

/// What a drag on the grid does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DisgustingBeatTool {
    /// Points: click to add, drag to move, drag a segment's grip to bend it.
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

impl DisgustingBeatTool {
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
            Self::Points => {
                "Click to add a point, drag one to move it, drag a segment's grip to bend it. \
                 Two points at one place is a jump"
            }
            Self::Pencil => "Draw freehand",
            Self::Line => "Drag a straight segment",
            Self::Hold => "Drag the freeze: the sound stops where you start and waits",
            Self::Step => "Paint steps on the snap division",
        }
    }
}

/// What a drawn point snaps to along the lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DisgustingBeatSnap {
    Quarter,
    Eighth,
    #[default]
    Sixteenth,
    ThirtySecond,
    Triplet,
    Off,
}

impl DisgustingBeatSnap {
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

impl DisgustingBeatView {
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

/// Where the read head sits along the canopy, 0 at the oldest column and 1
/// at the newest.
///
/// **Against the ring, not against what is in it.** The canopy draws all
/// twelve seconds of columns and leaves the unwritten ones empty on the left,
/// so a read a quarter of a second back is a quarter of a second from the
/// right edge whatever the memory happens to hold. Measuring it against
/// `filled_seconds` instead put the head halfway across a picture of nothing
/// for the first seconds after a start — found by drawing the trail beside
/// it and watching the two disagree.
pub fn canopy_head(view: &DisgustingBeatView) -> f32 {
    let back_seconds =
        (-view.offset).max(0.0) * view.time_beats() as f32 * 60.0 / view.bpm.max(1.0);
    (1.0 - back_seconds / fontelle_types::DISGUSTING_BEAT_MEMORY_SECONDS.max(0.001)).clamp(0.0, 1.0)
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
pub struct DisgustingBeatLayout {
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
    /// The magnification chips.
    pub zooms: Vec<Rect>,
    /// The open shape menu's rows, or empty.
    pub menu: Vec<Rect>,
    pub console: Rect,
    /// The generic knob panel's cells, laid out by the caller.
    pub controls: Vec<Rect>,
}

impl Default for DisgustingBeatLayout {
    fn default() -> Self {
        Self {
            body: Rect::ZERO,
            canopy: Rect::ZERO,
            lanes: Vec::new(),
            scenes: Vec::new(),
            tools: Vec::new(),
            snaps: Vec::new(),
            zooms: Vec::new(),
            menu: Vec::new(),
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
pub fn disgusting_beat_layout(view: &DisgustingBeatView, body: Rect) -> DisgustingBeatLayout {
    let mut layout = DisgustingBeatLayout {
        body,
        ..Default::default()
    };
    layout.canopy = Rect {
        x: body.x + PADDING,
        y: body.y + PADDING,
        width: (body.width - PADDING * 2.0).max(0.0),
        height: DISGUSTING_BEAT_CANOPY,
    };

    // The tool bar, across the top of the lanes.
    let toolbar_y = layout.canopy.y + layout.canopy.height + LANE_GAP;
    let mut x = body.x + PADDING;
    for tool in DisgustingBeatTool::ALL {
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
    for snap in DisgustingBeatSnap::ALL {
        let width = chip_width(snap.label());
        layout.snaps.push(Rect {
            x,
            y: toolbar_y,
            width,
            height: TOOLBAR_HEIGHT,
        });
        x += width + 4.0;
    }
    x += 16.0;
    for zoom in DisgustingBeatZoom::ALL {
        let width = chip_width(zoom.label());
        layout.zooms.push(Rect {
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
    let specs = fontelle_types::EffectConfig::DisgustingBeat(view.config);
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

    // The shape menu, wherever it was asked for and never off the window: a
    // right-click on the last point of the bottom lane asks for one four
    // pixels from the corner.
    if view.menu.is_some() {
        let (ask_x, ask_y) = view.menu.map(|menu| menu.at).unwrap_or((0.0, 0.0));
        let height = DISGUSTING_BEAT_MENU_ROWS as f32 * DISGUSTING_BEAT_MENU_ROW;
        let x = ask_x
            .min(body.right() - DISGUSTING_BEAT_MENU_WIDTH)
            .max(body.x);
        let y = ask_y.min(body.bottom() - height).max(body.y);
        for row in 0..DISGUSTING_BEAT_MENU_ROWS {
            layout.menu.push(Rect {
                x,
                y: y + row as f32 * DISGUSTING_BEAT_MENU_ROW,
                width: DISGUSTING_BEAT_MENU_WIDTH,
                height: DISGUSTING_BEAT_MENU_ROW,
            });
        }
    }
    layout
}

/// One row of the shape menu, and how wide the menu is.
pub const DISGUSTING_BEAT_MENU_ROW: f32 = 22.0;
pub const DISGUSTING_BEAT_MENU_WIDTH: f32 = 132.0;

/// How many rows the shape menu has: every shape, *split*, and *remove*.
pub const DISGUSTING_BEAT_MENU_ROWS: usize = CurveShape::ALL.len() + 2;

/// What a row of the shape menu says.
///
/// **Remove is the last row** rather than a gesture of its own, because
/// right-click used to remove a point outright and somebody who learned that
/// must not lose it to a menu that took its place. **Split** is above it: a
/// second point at this one's phase, which is how a lane spells an instant —
/// silence to full between two samples — and which is otherwise drawn by
/// clicking the grid above a point that is already there and guessing.
pub fn disgusting_beat_menu_label(row: usize) -> &'static str {
    match CurveShape::ALL.get(row) {
        Some(shape) => shape.label(),
        None if row == CurveShape::ALL.len() => "split",
        None => "remove",
    }
}

/// What a row of the menu does to the point it is open on.
pub fn disgusting_beat_menu_shape(row: usize) -> Option<CurveShape> {
    CurveShape::ALL.get(row).copied()
}

/// Whether this row is the one that makes a vertical.
pub fn disgusting_beat_menu_splits(row: usize) -> bool {
    row == CurveShape::ALL.len()
}

/// Whether a row would do anything to the point the menu is open on.
///
/// Only **split** ever says no, and only at the two ends of a lane: a
/// vertical needs a before and an after, and at phase 0 nothing arrives while
/// at phase 1 nothing leaves, so the lane would keep one half and drop the
/// other. The jump at the edge is already drawn — it is the seam. A row that
/// quietly does nothing is worse than one that is plainly off.
pub fn disgusting_beat_menu_enabled(
    view: &DisgustingBeatView,
    menu: DisgustingBeatMenu,
    row: usize,
) -> bool {
    if !disgusting_beat_menu_splits(row) {
        return true;
    }
    view.lanes
        .get(menu.lane)
        .and_then(|lane| lane.points.get(menu.index))
        .is_some_and(|point| point.at > 1e-9 && point.at < 1.0 - 1e-9)
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
pub enum DisgustingBeatHit {
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
    /// On the grip halfway along a segment, which bends it. The segment is
    /// named by the point that **carries its shape**, which is the one
    /// before it.
    Bend {
        lane: usize,
        carrier: usize,
    },
    /// On the strip that opens or closes a lane.
    LaneStrip {
        lane: usize,
    },
    /// On the chip in a lane's header that says how long it is.
    LaneLength {
        lane: usize,
    },
    /// On the chip that puts a lane back to neutral.
    LaneClear {
        lane: usize,
    },
    Scene(usize),
    Tool(DisgustingBeatTool),
    Snap(DisgustingBeatSnap),
    Zoom(DisgustingBeatZoom),
    /// A row of the open shape menu.
    MenuRow(usize),
    /// One of the console's parameters, by its place in
    /// `EffectConfig::specs`.
    Control(usize),
    Canopy,
}

/// The corner of an open lane that carries its name, and switches it off.
pub const LANE_NAME: (f32, f32) = (72.0, 18.0);

/// How far inside a lane's frame the value axis sits at the bottom, and the
/// header band it leaves at the top.
///
/// **Not decoration.** A curve at an end of its range lands exactly on the
/// frame, where half of a two-pixel stroke is clipped away and the other half
/// is the border — so a *flat* lane, which is what every fresh
/// DisgustingBeat has,
/// drew as nothing at all. And a flat lane at the *top* of its range then ran
/// straight through the lane's own name. Both were found by opening one on
/// `:99`; the headless case could not see either, because the scene it drew
/// had no value at an extreme.
pub const LANE_INSET: f32 = 7.0;

/// The band at the top of a lane that carries its name — and switches it off
/// (see [`LANE_NAME`]).
pub const LANE_HEADER: f32 = 18.0;

/// The gap between the chips in a lane's header, and how wide each is.
const LANE_CHIP_GAP: f32 = 4.0;
const LANE_LENGTH_CHIP: f32 = 58.0;
const LANE_CLEAR_CHIP: f32 = 44.0;

/// The chip in a lane's header that says how long the lane is.
///
/// A **per-lane** length is the polyrhythm: a volume lane three beats long
/// over a time lane of four walks round the bar. It was in the document from
/// the first day and had no way to be reached until this chip.
pub fn lane_length_rect(rect: Rect) -> Rect {
    Rect::new(
        rect.x + LANE_NAME.0 + LANE_CHIP_GAP,
        rect.y,
        LANE_LENGTH_CHIP,
        LANE_HEADER - 1.0,
    )
}

/// And the one that puts the lane back to neutral.
pub fn lane_clear_rect(rect: Rect) -> Rect {
    let length = lane_length_rect(rect);
    Rect::new(
        length.right() + LANE_CHIP_GAP,
        rect.y,
        LANE_CLEAR_CHIP,
        LANE_HEADER - 1.0,
    )
}

/// The part of a lane's frame the values are plotted in.
pub fn plot_area(rect: Rect) -> Rect {
    Rect::new(
        rect.x,
        rect.y + LANE_HEADER,
        rect.width,
        (rect.height - LANE_HEADER - LANE_INSET).max(1.0),
    )
}

/// How close to a point the pointer has to be to grab it, in pixels.
pub const DISGUSTING_BEAT_GRAB: f32 = 9.0;

/// What is under `(x, y)`.
pub fn disgusting_beat_hit(
    layout: &DisgustingBeatLayout,
    view: &DisgustingBeatView,
    x: f32,
    y: f32,
) -> Option<DisgustingBeatHit> {
    // The menu first and above everything: a click that chooses a shape must
    // not also draw a point on the lane it is covering.
    for (row, rect) in layout.menu.iter().enumerate() {
        if rect.contains(x, y) {
            return Some(DisgustingBeatHit::MenuRow(row));
        }
    }
    if layout.canopy.contains(x, y) {
        return Some(DisgustingBeatHit::Canopy);
    }
    for (index, rect) in layout.scenes.iter().enumerate() {
        if rect.contains(x, y) {
            return Some(DisgustingBeatHit::Scene(index));
        }
    }
    for (index, rect) in layout.tools.iter().enumerate() {
        if rect.contains(x, y) {
            return Some(DisgustingBeatHit::Tool(DisgustingBeatTool::ALL[index]));
        }
    }
    for (index, rect) in layout.snaps.iter().enumerate() {
        if rect.contains(x, y) {
            return Some(DisgustingBeatHit::Snap(DisgustingBeatSnap::ALL[index]));
        }
    }
    for (index, rect) in layout.zooms.iter().enumerate() {
        if rect.contains(x, y) {
            return Some(DisgustingBeatHit::Zoom(DisgustingBeatZoom::ALL[index]));
        }
    }
    for (index, rect) in layout.controls.iter().enumerate() {
        if rect.contains(x, y) {
            return Some(DisgustingBeatHit::Control(index));
        }
    }
    for (index, rect) in layout.lanes.iter().enumerate() {
        if !rect.contains(x, y) {
            continue;
        }
        let lane = view.lanes.get(index)?;
        if !lane.open {
            return Some(DisgustingBeatHit::LaneStrip { lane: index });
        }
        // An open lane's **name** switches it off again, so the gesture works
        // both ways with one hit kind rather than one to open and none to
        // close.
        let name = Rect::new(rect.x, rect.y, LANE_NAME.0, LANE_NAME.1);
        if name.contains(x, y) {
            return Some(DisgustingBeatHit::LaneStrip { lane: index });
        }
        if lane_length_rect(*rect).contains(x, y) {
            return Some(DisgustingBeatHit::LaneLength { lane: index });
        }
        if lane_clear_rect(*rect).contains(x, y) {
            return Some(DisgustingBeatHit::LaneClear { lane: index });
        }
        // **Only the points tool has handles.** With a tool that draws, the
        // lane is a canvas and nothing on it is a grip, which is what every
        // drawing program does with a pencil — and what the *hold* tool
        // needed: a tape stop starts at the top left of the lane, which is
        // exactly where a flat lane's one point sits, so its own gesture
        // grabbed that point and dragged it instead of laying the freeze
        // down. Found on `:99`.
        if view.tool != DisgustingBeatTool::Points {
            let (phase, value) = grid_position(view, lane, *rect, x, y);
            return Some(DisgustingBeatHit::Grid {
                lane: index,
                phase,
                value,
            });
        }
        // A point first: the grab radius beats the grid everywhere they
        // overlap, or a point could never be picked up off its own line.
        for (point_index, point) in lane.points.iter().enumerate() {
            let (px, py) = point_position(view, lane, *rect, point.at, point.value);
            if (px - x).hypot(py - y) <= DISGUSTING_BEAT_GRAB {
                return Some(DisgustingBeatHit::Point {
                    lane: index,
                    index: point_index,
                });
            }
        }
        // Then the grips on the segments. **After** the points, because a
        // grip on a short segment can sit under one and a point has to stay
        // pickable; before the grid, for the same reason a point is.
        for carrier in 0..lane.points.len() {
            let Some((bx, by)) = bend_handle(view, lane, *rect, carrier) else {
                continue;
            };
            if (bx - x).hypot(by - y) <= DISGUSTING_BEAT_BEND_GRAB {
                return Some(DisgustingBeatHit::Bend {
                    lane: index,
                    carrier,
                });
            }
        }
        let (phase, value) = grid_position(view, lane, *rect, x, y);
        return Some(DisgustingBeatHit::Grid {
            lane: index,
            phase,
            value,
        });
    }
    None
}

/// One sentence for whatever is under the pointer, or `None` for the parts
/// of the window that *are* the picture.
///
/// The canopy carries it while the pointer is on a chip, which is the only
/// written rule this window has — beside the thing it explains rather than in
/// a manual. Static strings, so the window can shape the one it needs without
/// keeping a table of every sentence there is.
pub fn disgusting_beat_tip(
    view: &DisgustingBeatView,
    hit: DisgustingBeatHit,
) -> Option<&'static str> {
    Some(match hit {
        DisgustingBeatHit::Tool(tool) => tool.tip(),
        DisgustingBeatHit::Snap(_) => "Where a drawn point lands along the lane",
        DisgustingBeatHit::Zoom(zoom) => zoom.tip(),
        DisgustingBeatHit::LaneStrip { lane } => match view.lanes.get(lane).map(|lane| lane.open) {
            Some(true) => "Switch this lane off \u{2014} it keeps what is drawn on it",
            _ => "Switch this lane on",
        },
        DisgustingBeatHit::LaneLength { .. } => {
            "How long this lane is: click to step it, right-click to step back. \
             A lane shorter than the others walks round the bar"
        }
        DisgustingBeatHit::LaneClear { .. } => "Put this lane back to neutral",
        DisgustingBeatHit::Scene(_) => {
            "Click to play this scene, right-click to name it, drag it onto another to copy it"
        }
        DisgustingBeatHit::MenuRow(row) => {
            if disgusting_beat_menu_splits(row) {
                "A second point here, so the lane can jump rather than slide"
            } else {
                "The shape of the segment that leaves this point"
            }
        }
        DisgustingBeatHit::Canopy => {
            "The memory the curves read from: oldest on the left, now on the right, \
             lit where the read head has been"
        }
        DisgustingBeatHit::Grid { .. }
        | DisgustingBeatHit::Point { .. }
        | DisgustingBeatHit::Bend { .. }
        | DisgustingBeatHit::Control(_) => return None,
    })
}

/// What the thing under the pointer is **worth**, in the lane's own words.
///
/// The console says what the machine is doing now; this says what the hand is
/// on, which is the number somebody drawing needs and the one the window
/// could not say until 2026-09-23. On the time lane a segment is a **speed**,
/// because the slope is the sound and "-0.5" means nothing to anybody:
/// `rate = 1 + dv/dp`, and a rate is a pitch.
///
/// Shaped by the caller, so this crate stays free of text measuring
/// (INVARIANT 2) — it returns the words, not a picture of them.
pub fn disgusting_beat_readout(
    view: &DisgustingBeatView,
    hit: DisgustingBeatHit,
) -> Option<String> {
    let (lane, at, value) = match hit {
        DisgustingBeatHit::Point { lane, index } => {
            let lane = view.lanes.get(lane)?;
            let point = lane.points.get(index)?;
            (lane, point.at, point.value)
        }
        DisgustingBeatHit::Grid { lane, phase, value } => (view.lanes.get(lane)?, phase, value),
        DisgustingBeatHit::Bend { lane, carrier } => {
            let lane = view.lanes.get(lane)?;
            let from = lane.points.get(carrier)?;
            let to = lane.points.get(carrier + 1)?;
            // A segment says what it **plays at**, which is the only thing
            // about a segment anybody is choosing.
            let slope = (to.value - from.value) / (to.at - from.at).max(1e-9);
            return Some(match lane.kind {
                DisgustingBeatLaneKind::Time => {
                    format!("segment: {}", rate_caption(1.0 + slope as f32))
                }
                _ => format!(
                    "segment: {:+.2} over {:.2} beats",
                    to.value - from.value,
                    (to.at - from.at) * lane_beats(view, lane)
                ),
            });
        }
        _ => return None,
    };
    let beat = at * lane_beats(view, lane) + 1.0;
    Some(match lane.kind {
        // Where it is, and how far back it reads — in **beats**, because a
        // lane-length is a unit nobody has a feel for and a beat is the one
        // everybody does.
        DisgustingBeatLaneKind::Time => format!(
            "beat {beat:.2}   {:+.2} beats back",
            value * lane_beats(view, lane)
        ),
        DisgustingBeatLaneKind::Volume => format!("beat {beat:.2}   {:.0} %", value * 100.0),
        _ => format!("beat {beat:.2}   {:+.0} %", value * 100.0),
    })
}

/// How many beats one lane runs for, with the rate knob's stretch in it.
fn lane_beats(view: &DisgustingBeatView, lane: &LaneView) -> f64 {
    lane.length.beats(view.beats_per_bar) * view.config.rate.stretch()
}

/// Where a point sits in the lane's rectangle.
pub fn point_position(
    view: &DisgustingBeatView,
    lane: &LaneView,
    rect: Rect,
    at: f64,
    value: f64,
) -> (f32, f32) {
    let plot = plot_area(rect);
    let x = plot.x + (at.clamp(0.0, 1.0) as f32) * plot.width;
    let y = plot.y + value_to_fraction(view, lane, value) * plot.height;
    (x, y)
}

/// And the other way: where the pointer is, in the lane's own units.
pub fn grid_position(
    view: &DisgustingBeatView,
    lane: &LaneView,
    rect: Rect,
    x: f32,
    y: f32,
) -> (f64, f64) {
    let plot = plot_area(rect);
    let phase = if plot.width > 0.0 {
        ((x - plot.x) / plot.width).clamp(0.0, 1.0) as f64
    } else {
        0.0
    };
    let fraction = if plot.height > 0.0 {
        ((y - plot.y) / plot.height).clamp(0.0, 1.0)
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
fn value_to_fraction(view: &DisgustingBeatView, lane: &LaneView, value: f64) -> f32 {
    match lane.kind {
        DisgustingBeatLaneKind::Time => {
            let reach = view.zoom.max(0.01) as f64;
            // Zero at the top when nothing looks ahead, and the zero line
            // slides down to make room for the future when something does.
            let above = if view.config.look == fontelle_types::DisgustingBeatLook::Off {
                0.0
            } else {
                reach * 0.25
            };
            let span = reach + above;
            (((above - value) / span).clamp(0.0, 1.0)) as f32
        }
        DisgustingBeatLaneKind::Volume => (1.0 - value.clamp(0.0, 1.0)) as f32,
        _ => ((1.0 - value.clamp(-1.0, 1.0)) * 0.5) as f32,
    }
}

fn fraction_to_value(view: &DisgustingBeatView, lane: &LaneView, fraction: f32) -> f64 {
    let fraction = fraction.clamp(0.0, 1.0) as f64;
    match lane.kind {
        DisgustingBeatLaneKind::Time => {
            let reach = view.zoom.max(0.01) as f64;
            let above = if view.config.look == fontelle_types::DisgustingBeatLook::Off {
                0.0
            } else {
                reach * 0.25
            };
            let span = reach + above;
            (above - fraction * span).clamp(-1.0, 1.0)
        }
        DisgustingBeatLaneKind::Volume => 1.0 - fraction,
        _ => 1.0 - fraction * 2.0,
    }
}

/// The freeze slope, in the lane rectangle's own pixels: how far down the
/// grid a hold runs per pixel across it.
///
/// One at the default zoom, which is the whole point of that default — the
/// guide the grid draws is a true 45° line and a hold is something you trace.
pub fn freeze_slope(view: &DisgustingBeatView, rect: Rect) -> f32 {
    let rect = plot_area(rect);
    if rect.width <= 0.0 {
        return 1.0;
    }
    let reach = view.zoom.max(0.01);
    let above = if view.config.look == fontelle_types::DisgustingBeatLook::Off {
        0.0
    } else {
        reach * 0.25
    };
    let span = reach + above;
    (rect.height / span) / (rect.width / 1.0)
}

/// How wide the grip on a segment is drawn, and how far from its centre a
/// press still lands on it.
pub const DISGUSTING_BEAT_BEND_GRAB: f32 = 7.0;

/// Which way a bend runs on this segment.
///
/// `value = from + (to − from) · eased(t, tension)`, so on a **descending**
/// segment a positive tension pulls the curve *down*. Dragging up has to
/// raise the line whichever way it runs, or half the segments in a lane
/// would bend backwards under the hand.
pub fn bend_sign(lane: &LaneView, carrier: usize) -> f64 {
    bend_reach(lane, carrier).1
}

/// Whether a segment has anywhere to bend *to*, and which way a drag runs.
///
/// Three kinds of segment have nowhere to go. A segment whose two ends are at
/// the same value is a flat line, and `from + (to − from) · eased(t)` is that
/// same value whatever the tension is (found on `:99`, where every fresh lane
/// is flat). A **stepped** one ignores where it is going at all. And a
/// **vertical** — a pair of points at one phase — has no width to bend
/// across.
///
/// The stretch past the last point is not a segment either, and that falls
/// out of this: there is no `carrier + 1` to bend towards. A lane holds its
/// last point until it comes round again, so there is nothing drawn there.
pub fn bend_reach(lane: &LaneView, carrier: usize) -> (bool, f64) {
    let Some(from) = lane.points.get(carrier) else {
        return (false, 1.0);
    };
    let Some(to) = lane.points.get(carrier + 1) else {
        return (false, 1.0);
    };
    if from.curve.holds() || (to.value - from.value).abs() < 1e-9 || (to.at - from.at).abs() < 1e-9
    {
        return (false, 1.0);
    }
    (true, if to.value < from.value { -1.0 } else { 1.0 })
}

/// Where the grip on a segment sits: halfway along it, **on the drawn line**.
///
/// A handle floating beside a line belongs to nothing, so this reads the
/// curve rather than averaging the two ends — a segment already bent has its
/// grip where the bend put it, which is also what tells you at a glance that
/// it is bent.
///
/// `None` when the segment has nowhere to bend to ([`bend_reach`]).
pub fn bend_handle(
    view: &DisgustingBeatView,
    lane: &LaneView,
    rect: Rect,
    carrier: usize,
) -> Option<(f32, f32)> {
    if !bend_reach(lane, carrier).0 {
        return None;
    }
    let from = lane.points.get(carrier)?;
    let to = lane.points.get(carrier + 1)?;
    let phase = (from.at + to.at) * 0.5;
    let value = fontelle_types::curve_at(&lane.points, phase, lane.kind.neutral());
    Some(point_position(view, lane, rect, phase, value))
}

/// Where the **split** row puts its twin: the same phase, far enough away in
/// value that the vertical it makes is something a hand can see and grab.
///
/// A quarter of what the lane is *showing*, not of what it holds. The time
/// lane's range is a lane-length either way and the reach chooses how much of
/// that is on the screen, so a quarter of the range is off the top at 8× —
/// and anything above the zero line is a read of the future, which the
/// machine clamps away when nothing is looking ahead. It goes towards
/// whichever end has more room.
///
/// `None` for a point at the very start or end of the lane, where a vertical
/// has no before or no after ([`disgusting_beat_menu_enabled`]).
pub fn split_twin(view: &DisgustingBeatView, lane: &LaneView, index: usize) -> Option<f64> {
    let point = lane.points.get(index)?;
    if point.at <= 1e-9 || point.at >= 1.0 - 1e-9 {
        return None;
    }
    // The two values the top and the bottom of this lane's grid stand for.
    let (high, low) = (
        fraction_to_value(view, lane, 0.0),
        fraction_to_value(view, lane, 1.0),
    );
    let reach = (high - low) * 0.25;
    let twin = if point.value - low > high - point.value {
        point.value - reach
    } else {
        point.value + reach
    };
    Some(twin.clamp(low, high))
}

/// Which point a drag is holding, now that the lane has been tidied under it.
///
/// A lane re-sorts after every edit, so the index a drag started with names a
/// **different point** the moment the one it is holding crosses another — and
/// the drag then walks off with the neighbour. `target` is where the drag
/// last put its point; the answer is the point that is there, or the index it
/// was given when nothing has moved.
pub fn grabbed_point(lane: &LaneView, target: (f64, f64), index: usize) -> Option<usize> {
    let near = |point: &DisgustingBeatPoint| {
        (point.at - target.0).abs() < 1e-6 && (point.value - target.1).abs() < 1e-6
    };
    match lane.points.get(index) {
        Some(point) if near(point) => Some(index),
        // `None` rather than a guess: the point the drag was holding is not
        // in the lane any more — the edit that would have put it there was
        // refused, which is what a full lane does — and moving whichever
        // point happens to be at that index instead is a curve changing under
        // a hand that did not ask.
        _ => lane.points.iter().position(near),
    }
}

/// The points a **hold** drag lays down: the freeze from `from` to `to`.
///
/// Two points and the segment between them, at exactly the slope that holds
/// the sound still — `value` falls by the same fraction of a lane-length that
/// `phase` advances. The gesture is the whole feature.
pub fn hold_points(from: (f64, f64), to: (f64, f64)) -> [DisgustingBeatPoint; 2] {
    let (start, end) = if from.0 <= to.0 {
        (from, to)
    } else {
        (to, from)
    };
    let fallen = (start.1 - (end.0 - start.0)).clamp(-1.0, 1.0);
    [
        DisgustingBeatPoint::new(start.0, start.1, CurveShape::Linear),
        DisgustingBeatPoint::new(end.0, fallen, CurveShape::Linear),
    ]
}

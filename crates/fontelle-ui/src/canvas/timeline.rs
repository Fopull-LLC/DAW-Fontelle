//! The arrangement canvas (TDD §16.4; the half of item 9 of
//! `docs/first-usable-plan.md` that was outstanding).
//!
//! Clips as blocks on lanes: move them, size them, duplicate them, mute them,
//! delete them, and click one to open it in the piano roll. It is the piano
//! roll's shape at a coarser grain and it obeys the same two rules —
//!
//! - **Virtualisation is mandatory** (§16.4). [`visible_lanes`] and
//!   [`visible_ticks`](crate::canvas::visible_ticks)'s counterpart here bound
//!   every loop, so a project with two hundred lanes costs a screenful of
//!   rectangles.
//! - **The canvas is a view, never a mutator** (INVARIANT 2). It reads
//!   [`ClipInfo`] and returns [`ArrangeEdit`] values; turning those into
//!   `Command`s is `fontelle-app`'s job.
//!
//! Direct-draw, not a widget tree, for the reason §16.4 gives: grid, clips and
//! playhead are separate layers with independent invalidation, so a moving
//! playhead never redirties clip geometry.

use std::ops::Range;

use fontelle_types::{ClipId, ClipStretch, PPQN, PointId, Tick};

use crate::canvas::automation::{automation_block, block_tick_at, block_value_at};
use crate::canvas::piano_roll::{SnapDivision, snap_tick, snap_unit};
use crate::canvas::{Modifiers, MouseButton, clamp_to_grid};
use crate::document::{ClipInfo, ClipKind};
use crate::layout::Rect;
use crate::theme::Metrics;

/// How wide the grab handle on a clip's right-hand edge is.
const HANDLE_PX: f32 = 7.0;

/// How wide `block`'s right-hand resize grip actually is.
///
/// A block narrower than three grips has none: moving a clip is more common
/// than sizing one, and a clip you cannot grab is worse than one you cannot
/// size without zooming in.
///
/// Public because an automation block's curve has to keep clear of it — see
/// [`automation_block`](crate::canvas::automation_block). Two answers to
/// "where does the grip start" would be a point drawn under a grip that then
/// swallows every press aimed at it.
pub fn clip_grip(block: Rect) -> f32 {
    HANDLE_PX.min(block.width / 3.0)
}

/// How tall the caption band across the top of a clip block is, at most.
///
/// A third of the block when the block is short, so a shallow lane still has
/// more content than caption: what is *in* a clip is the thing you are
/// reading, and the name only says which clip it is.
pub const CLIP_HEADER_PX: f32 = 12.0;

/// A clip block's two bands: the caption across the top, and the room its
/// content gets under it.
///
/// One function for both kinds of clip. An automation block's curve and a
/// note block's notes sit in the same place for the same reason — a caption
/// drawn *over* the content of one and *beside* the content of the other is
/// two pictures where there should be one.
pub fn clip_bands(block: Rect) -> (Rect, Rect) {
    let header = CLIP_HEADER_PX.min(block.height / 3.0).max(0.0);
    block.split_top(header)
}

/// How few semitones the note preview's pitch axis may span.
///
/// Scaled to the notes present a single note fills the block's whole height
/// and reads as a solid bar rather than as a note, and a two-note clip draws
/// two slabs. An octave is the floor, which is also about the range a bar of
/// music usually moves in, so the common case is not stretched either.
pub const NOTE_PREVIEW_MIN_KEYS: u8 = 12;

/// How short a block may be before its notes stop being drawn.
///
/// Under this a row of one-pixel smudges is less readable than the plain
/// block the arrangement used to draw.
const MIN_NOTE_BLOCK_PX: f32 = 10.0;

/// Which end of a clip a fade is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FadeEnd {
    In,
    Out,
}

/// How wide a fade handle is, at most.
const FADE_HANDLE_PX: f32 = 10.0;

/// How big the node on a fade's curve is.
const FADE_NODE_PX: f32 = 8.0;

/// An audio block's fade handles (TDD §15.2) — see [`fade_anatomy`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FadeAnatomy {
    /// The handle at the top-left corner, or — with a fade in place — at
    /// the top of the block where the fade ends.
    pub handle_in: Rect,
    /// The same at the top-right, where the fade out begins.
    pub handle_out: Rect,
    /// The node at the midpoint of the fade-in curve, when there is one.
    pub node_in: Option<Rect>,
    pub node_out: Option<Rect>,
}

/// The fade handles of an audio block, and `None` for any other kind.
///
/// > *"you can manually (like fl studios clip fades) drag in from the start
/// > or end of an audio clip to create a clip fade ... and you can bend the
/// > control node to bend the curve like fl studios too."*
///
/// FL's anatomy: a handle at each **top corner**, in the caption band.
/// Dragged along the block it sets the fade's length; with a fade in place
/// it sits where the fade ends, so what you grab is what you set. A fade
/// also has a **node** at the midpoint of its curve, in the content band,
/// which is what bends it. Both are measured against the block's whole
/// rectangle, the same one the waveform and the pointer are.
pub fn fade_anatomy(block: Rect, clip: &ClipInfo) -> Option<FadeAnatomy> {
    if clip.kind != ClipKind::Audio || block.width <= 0.0 {
        return None;
    }
    let (header, content) = clip_bands(block);
    let size = FADE_HANDLE_PX.min(block.width / 2.0).max(1.0);
    let fade_in = clip.audio.fade_in.clamp(0.0, 1.0);
    let fade_out = clip.audio.fade_out.clamp(0.0, 1.0);
    // Against the **file**, not the block: a fade is frames of the file and
    // the player measures it from the file's own start and end, so on an
    // unstretched block longer than its file the out handle sits where the
    // sound stops rather than at a corner nothing is playing under.
    let (file_px, file_end) = content_span(block, clip);
    let end_in = block.x + file_px * fade_in;
    let start_out = file_end - file_px * fade_out;
    // Never off the block: a handle drawn past the corner is one nobody can
    // find, and the hit-test is measured inside the block.
    let clamp_x = |x: f32| x.clamp(block.x, (block.right() - size).max(block.x));
    let handle_in = if fade_in > 0.0 {
        Rect::new(clamp_x(end_in - size / 2.0), header.y, size, header.height)
    } else {
        Rect::new(block.x, header.y, size, header.height)
    };
    let handle_out = if fade_out > 0.0 {
        Rect::new(
            clamp_x(start_out - size / 2.0),
            header.y,
            size,
            header.height,
        )
    } else {
        Rect::new(clamp_x(file_end - size), header.y, size, header.height)
    };
    let node = |x: f32, tension: f32| {
        if content.height <= 0.0 {
            return None;
        }
        let gain = fontelle_types::bend(0.5, tension) as f32;
        let y = content.bottom() - content.height * gain;
        Some(Rect::new(
            x - FADE_NODE_PX / 2.0,
            y - FADE_NODE_PX / 2.0,
            FADE_NODE_PX,
            FADE_NODE_PX,
        ))
    };
    let node_in = (fade_in > 0.0)
        .then(|| {
            node(
                block.x + file_px * fade_in * 0.5,
                clip.audio.fade_in_tension,
            )
        })
        .flatten();
    let node_out = (fade_out > 0.0)
        .then(|| {
            node(
                file_end - file_px * fade_out * 0.5,
                clip.audio.fade_out_tension,
            )
        })
        .flatten();
    Some(FadeAnatomy {
        handle_in,
        handle_out,
        node_in,
        node_out,
    })
}

/// How many points a fade curve is drawn with.
const FADE_CURVE_POINTS: usize = 24;

/// One pass of an audio clip, in ticks: its period when it loops, and the
/// whole block when it does not.
fn pass_ticks(clip: &ClipInfo) -> Tick {
    clip.loop_length
        .filter(|p| *p > 0)
        .unwrap_or(clip.length)
        .max(1)
}

/// How many ticks of each pass the clip's **file** covers.
///
/// This is the whole of the difference between the two things an edge drag
/// can mean (the Stretch switch, [`TimelineControl::Stretch`]):
///
/// - **Stretched**, the file fills its pass, whatever the pass is — drag the
///   block longer and the file plays slower to fill it.
/// - **Not stretched**, the file plays at its own rate and takes the ticks
///   it takes (`AudioPreview::natural_length`); the block is a window onto
///   it, and dragging the block longer shows silence past the file's end.
///
/// A clip whose natural length is not known yet is drawn filling its pass,
/// which is §15.3's "draw what exists" — never nothing.
pub fn content_ticks(clip: &ClipInfo) -> Tick {
    let pass = pass_ticks(clip);
    if clip.audio.stretched || clip.audio.natural_length <= 0 {
        pass
    } else {
        clip.audio.natural_length
    }
}

/// Where along the clip's file the point `along` ticks into the block falls,
/// 0..1 — or `None` where the block is past the file's end, which is silence.
///
/// One answer for the waveform, the fade handles, the fade drag and the
/// crossfade curves, so the four cannot disagree about where the sound is.
/// Ticks rather than pixels, and fractional, because a column of the
/// waveform is narrower than a tick at some zooms and wider at others.
pub fn content_fraction(clip: &ClipInfo, along: f32) -> Option<f32> {
    let pass = pass_ticks(clip) as f32;
    let within = if clip.loop_length.is_some_and(|p| p > 0) {
        along.rem_euclid(pass)
    } else {
        along
    };
    let content = content_ticks(clip) as f32;
    if within < 0.0 || within >= content {
        return None;
    }
    Some(within / content)
}

/// The file's span on `block`, in screen points: how wide one pass of it is
/// drawn, and where the **first** pass ends — clamped to the block, since
/// that is all there is to draw on.
fn content_span(block: Rect, clip: &ClipInfo) -> (f32, f32) {
    let per_tick = if clip.length > 0 {
        block.width / clip.length as f32
    } else {
        0.0
    };
    let file_px = (content_ticks(clip) as f32 * per_tick).max(1.0);
    let file_end = (block.x + file_px).min(block.right());
    (file_px, file_end)
}

/// One fade's curve across the content band, in screen points — the shape
/// the player uses, drawn: silence at the block's corner, full where the
/// fade ends, bent by the fade's tension. Empty for no fade, and for any
/// block that is not audio.
pub fn fade_curve(block: Rect, clip: &ClipInfo, end: FadeEnd) -> Vec<(f32, f32)> {
    if clip.kind != ClipKind::Audio {
        return Vec::new();
    }
    let (_, content) = clip_bands(block);
    if content.width <= 0.0 || content.height <= 0.0 {
        return Vec::new();
    }
    let (fraction, tension) = match end {
        FadeEnd::In => (clip.audio.fade_in, clip.audio.fade_in_tension),
        FadeEnd::Out => (clip.audio.fade_out, clip.audio.fade_out_tension),
    };
    let fraction = fraction.clamp(0.0, 1.0);
    if fraction <= 0.0 {
        return Vec::new();
    }
    // Against the **file**, not the block — the same span `fade_anatomy`
    // places the handles against, and for the same reason: a fade is frames of
    // the file, so a block dragged out past its file must not draw a longer
    // one. *"extending a loop on a clip is affecting the fade lengths on the
    // clip"* was this measured against `content.width` instead, which made the
    // curve and its own handle disagree by however far the block had been
    // dragged.
    let (file_px, file_end) = content_span(block, clip);
    let span = file_px * fraction;
    (0..=FADE_CURVE_POINTS)
        .map(|i| {
            let t = i as f32 / FADE_CURVE_POINTS as f32;
            let gain = fontelle_types::bend(f64::from(t), tension) as f32;
            match end {
                FadeEnd::In => (
                    content.x + span * t,
                    content.bottom() - content.height * gain,
                ),
                // Mirrored: full where the fade starts, silence at the end —
                // and "the end" is where the file stops, not where the block
                // does.
                FadeEnd::Out => (
                    file_end - span * (1.0 - t),
                    content.bottom()
                        - content.height * fontelle_types::bend(f64::from(1.0 - t), tension) as f32,
                ),
            }
        })
        .collect()
}

/// The waveform inside an audio clip's block (TDD §15.3).
///
/// Reported from using the window: *"i should be able to see the waveform of
/// the audio inside the clip."*
///
/// One rectangle per **pixel column**, from the summary
/// `AudioPreview::peaks` holds — resampled onto the columns rather than drawn
/// one bucket per bucket, so a clip zoomed in past its own summary is stretched
/// smoothly instead of drawn as a comb.
///
/// The rules the picture keeps, each of which is a test:
///
/// - It lives in the **content band** and never the caption, like the
///   automation curve and the note preview — one picture, not three.
/// - It is **centred**: zero is the middle of the band, so a quiet take and a
///   loud one are different shapes rather than the same shape hung differently.
/// - **Silence is a line, not nothing.** A gap in a take is part of the take;
///   nothing at all reads as the clip ending early.
/// - The **fades shape it**, because what you see has to be what you hear.
/// - Only the part on screen is built, like the note preview: a twenty-minute
///   take scrolled mostly off must not cost twenty minutes of columns a frame.
pub fn clip_waveform(block: Rect, visible: Rect, clip: &ClipInfo) -> Vec<Rect> {
    if clip.kind != ClipKind::Audio || clip.audio.peaks.is_empty() {
        return Vec::new();
    }
    let (_, content) = clip_bands(block);
    if content.is_empty() || content.width <= 0.0 || content.height < MIN_WAVEFORM_BLOCK_PX {
        return Vec::new();
    }
    // The columns actually on screen. The block may run for a screen either
    // way, and a column outside the grid is one nobody sees.
    let from = content.x.max(visible.x).floor();
    let to = content.right().min(visible.right()).ceil();
    if to <= from {
        return Vec::new();
    }

    let middle = content.y + content.height / 2.0;
    let half = content.height / 2.0;
    let peaks = &clip.audio.peaks;
    let ticks_per_px = clip.length.max(1) as f32 / content.width;
    let mut columns = Vec::with_capacity((to - from) as usize + 1);
    let mut x = from;
    while x < to {
        // Where this column sits along the **file**, 0..1 — through the
        // clip's passes and its mode (`content_fraction`), and measured
        // against the whole content band rather than the visible part, so
        // the picture does not slide as the arrangement scrolls. Past the
        // file's end there is no column: the sound is over there, and a line
        // would say it was quiet.
        let along = (x + 0.5 - content.x) * ticks_per_px;
        let Some(t) = content_fraction(clip, along) else {
            x += 1.0;
            continue;
        };
        let bucket = ((t * peaks.len() as f32) as usize).min(peaks.len() - 1);
        let (low, high) = peaks[bucket];
        let envelope = preview_fade(&clip.audio, t);
        let top = middle - (high.clamp(-1.0, 1.0) * half * envelope).max(0.0);
        let bottom = middle - (low.clamp(-1.0, 1.0) * half * envelope).min(0.0);
        // At least a pixel: silence is a line through the middle, which is
        // what says "this part of the take is quiet" rather than "the take
        // stops here".
        let height = (bottom - top).max(1.0);
        columns.push(Rect::new(x, top.min(middle), 1.0, height));
        x += 1.0;
    }
    columns
}

/// **Where the take runs out inside its block**, in screen points — or `None`
/// when the file fills the block, which is the ordinary case.
///
/// A block can be longer than the sound in it, and when it is, the rest is
/// drawn as nothing. That is honest and it is not *legible*: blank reads as
/// "this picture is broken" rather than as "the take ends here", and the
/// difference matters because somebody is trimming against it.
///
/// > *"is making the audio show completely blank after that even though it
/// > actually does have content"*
///
/// Measured: a clip shortened with Stretch on and then switched off carries
/// the compression as `speed`, so growing the block back leaves the file
/// covering a fifth of it and four fifths empty. The arithmetic is right and
/// the picture said nothing about why. This is what lets the renderer put a
/// mark at the end of the take and dim what is past it — a *stated* end
/// rather than an absence.
pub fn content_end(block: Rect, clip: &ClipInfo) -> Option<f32> {
    if clip.kind != ClipKind::Audio || clip.audio.peaks.is_empty() {
        return None;
    }
    // A looping clip has no single end: it restarts every pass, and the gap at
    // the end of one pass is a rhythm rather than a mistake.
    if clip.loop_length.is_some_and(|p| p > 0) {
        return None;
    }
    let content = content_ticks(clip);
    if content <= 0 || content >= clip.length {
        return None;
    }
    let per_tick = if clip.length > 0 {
        block.width / clip.length as f32
    } else {
        return None;
    };
    let at = block.x + content as f32 * per_tick;
    (at < block.right() - 1.0).then_some(at)
}

/// The fade envelope at `t` along a clip, 0..1 — the drawn form of
/// `AudioClipData::fade_gain`.
///
/// The two multiply, as they do in the player, so a short clip with a long fade
/// at each end is a shape rather than a step where one of them wins.
fn preview_fade(audio: &crate::document::AudioPreview, t: f32) -> f32 {
    let mut gain = 1.0;
    if audio.fade_in > 0.0 {
        let along = (t / audio.fade_in).clamp(0.0, 1.0);
        gain *= fontelle_types::bend(f64::from(along), audio.fade_in_tension) as f32;
    }
    if audio.fade_out > 0.0 {
        let along = ((1.0 - t) / audio.fade_out).clamp(0.0, 1.0);
        gain *= fontelle_types::bend(f64::from(along), audio.fade_out_tension) as f32;
    }
    gain
}

/// A block shorter than this has no room for a waveform, only for its caption.
const MIN_WAVEFORM_BLOCK_PX: f32 = 6.0;

/// The notes inside a clip's block, in screen points (TDD §16.4).
///
/// *"show a preview of the notes drawn out inside of it like how other daws
/// do... ensure it actually displays cleanly so the sections actually line up
/// with what youre editing."* Lining up is the whole of it: a note is placed
/// against the block's own time axis, which is the axis the ruler above it is
/// drawn against, so bar 3 of the clip is bar 3 of the song.
///
/// `block` is the clip's **whole** rectangle, unclipped — the same one
/// [`clip_rect`] returns — because that is what the time axis is measured
/// against; a block measured from the part of it on screen would slide as the
/// arrangement scrolled. `visible` bounds what is *built*: §16.4 makes
/// virtualisation mandatory, and a two-hundred-bar loop is otherwise two
/// hundred passes of geometry for a screenful of pixels.
///
/// Empty for a clip that is not [`ClipKind::Notes`], one with no notes, and
/// one drawn too small to read.
pub fn clip_notes(block: Rect, visible: Rect, clip: &ClipInfo) -> Vec<Rect> {
    if clip.kind != ClipKind::Notes || clip.notes.is_empty() || clip.length <= 0 {
        return Vec::new();
    }
    let (_, content) = clip_bands(block);
    if content.is_empty() || content.height < MIN_NOTE_BLOCK_PX || block.width <= 0.0 {
        return Vec::new();
    }

    // The pitch axis: the notes that are there, with a floor so a clip of one
    // note is not one slab. Centred on what is present, so a bass part sits
    // low in its block and a lead sits high — which is a thing you can read at
    // a glance without any numbers.
    let (low, high) = clip.notes.iter().fold((u8::MAX, u8::MIN), |(lo, hi), n| {
        (lo.min(n.key), hi.max(n.key))
    });
    let span = u32::from(high - low) + 1;
    let span = span.max(u32::from(NOTE_PREVIEW_MIN_KEYS));
    // Centred: half the slack below the lowest note and half above the
    // highest, so the picture does not sit against one edge.
    let slack = span - (u32::from(high - low) + 1);
    let bottom_key = i32::from(low) - (slack / 2) as i32;
    let row = content.height / span as f32;

    // The passes to build. A clip that does not loop is one pass at zero; a
    // looped one repeats every `period` until the clip runs out — the same
    // arithmetic `loop_marks` uses, so the notes and the seams agree.
    let period = clip.loop_length.filter(|p| *p > 0);
    let per_tick = block.width / clip.length as f32;

    let mut rects = Vec::new();
    let mut base: Tick = 0;
    loop {
        let pass_left = content.x + base as f32 * per_tick;
        // Off the right of what can be seen: every pass after this one is
        // further right, so there is nothing left to build.
        if pass_left > visible.right() {
            break;
        }
        let pass_right = match period {
            Some(p) => content.x + ((base + p) as f32).min(clip.length as f32) * per_tick,
            None => content.right(),
        };
        if pass_right >= visible.x {
            for note in &clip.notes {
                // A note at or past the period is content the loop does not
                // contain — the compiler drops it, so drawing it would be a
                // picture of something the song does not play.
                if period.is_some_and(|p| note.start >= p) {
                    continue;
                }
                let start = base + note.start;
                if start >= clip.length {
                    continue;
                }
                // And a pass that rings past the clip's end is cut there, the
                // same rule again.
                let end = (start + note.length.max(1)).min(clip.length);
                if end <= start {
                    continue;
                }
                let x = content.x + start as f32 * per_tick;
                let width = ((end - start) as f32 * per_tick).max(1.0);
                let top = content.bottom() - (i32::from(note.key) - bottom_key + 1) as f32 * row;
                let rect = Rect::new(x, top, width, row.max(1.0))
                    .clamped()
                    .intersection(&content);
                if !rect.is_empty() {
                    rects.push(rect);
                }
            }
        }
        let Some(p) = period else { break };
        base += p;
        if base >= clip.length {
            break;
        }
    }
    rects
}

/// Zoom limits. A pixels-per-tick of zero is a division by zero in every
/// conversion here; a lane taller than the panel is not a zoom.
pub const MIN_TIMELINE_PPT: f32 = 0.0005;
pub const MAX_TIMELINE_PPT: f32 = 0.5;
pub const MIN_LANE_ROW: f32 = 14.0;
pub const MAX_LANE_ROW: f32 = 96.0;

/// Where the arrangement is looking, and how closely.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimelineView {
    /// The tick at the left edge of the grid.
    pub scroll_tick: Tick,
    /// The lane in the top row. Lanes read downwards, unlike the roll's keys —
    /// a track list is a list, not a keyboard.
    pub top_lane: usize,
    pub pixels_per_tick: f32,
    pub lane_height: f32,
    pub snap: SnapDivision,
}

impl Default for TimelineView {
    fn default() -> Self {
        Self {
            scroll_tick: 0,
            top_lane: 0,
            // About a hundred pixels to the bar: enough of a piece on screen to
            // be an arrangement rather than a magnified clip.
            pixels_per_tick: 0.025,
            lane_height: 34.0,
            // Bars, because that is what an arrangement is built out of.
            snap: SnapDivision::Bar,
        }
    }
}

/// The arrangement's parts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimelineLayout {
    pub frame: Rect,
    /// The controls across the top: the snap chip, repeat, the clipboard,
    /// mute. Added because the arrangement had none at all — see
    /// [`TimelineControl`].
    pub toolbar: Rect,
    /// The bar ruler, under the toolbar. Clicking it moves the time marker.
    pub ruler: Rect,
    /// The lane names down the left, beside the grid.
    pub headers: Rect,
    /// Where the clips are.
    pub grid: Rect,
}

/// Wide enough for a lane name and narrow enough not to eat the arrangement.
const HEADER_WIDTH: f32 = 120.0;

pub fn timeline_layout(frame: Rect, metrics: &Metrics) -> TimelineLayout {
    let row = metrics.row_height.min(frame.height.max(0.0));
    let (toolbar, under_toolbar) = frame.split_top(row);
    let (ruler, below) =
        under_toolbar.split_top(metrics.row_height.min(under_toolbar.height.max(0.0)));
    let header_width = HEADER_WIDTH.min(below.width.max(0.0));
    let headers = Rect::new(below.x, below.y, header_width, below.height).clamped();
    let grid = Rect::new(
        headers.right(),
        below.y,
        below.width - headers.width,
        below.height,
    )
    .clamped();
    TimelineLayout {
        frame,
        toolbar,
        ruler,
        headers,
        grid,
    }
}

/// One control on the arrangement's toolbar.
///
/// The arrangement had none. `TimelineView` has carried a `snap` since it was
/// written and `duplicate` has existed since the arrangement did; neither had
/// anywhere to appear, so both were rumours rather than controls — which is
/// exactly how they were reported (*"I can only change the length and move
/// them around"*, *"I don't see snap controls"*).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineControl {
    /// Draw: a double-click on empty grid makes a clip, and Shift+click puts
    /// down a copy of the clip in hand.
    Draw,
    /// Select: a press on empty grid marquees, the way it always did.
    Select,
    /// Cut: a press on a clip divides it where you pressed.
    Slice,
    /// Cycles the arrangement's snap; the chip says which division is on.
    Snap,
    /// What dragging an audio clip's edge does: **on**, the file is fitted
    /// to the block, so a longer block is a slower file; **off**, the file
    /// keeps its own speed and the block is a window onto it, so a longer
    /// block runs into silence and a shorter one cuts the file off.
    ///
    /// *"i cannot loop clips, whenever i drag them it is ALWAYS stretching
    /// them. we should make it so theres a stretch on/off toggle control
    /// with the arrangement controls and that defines whether it cuts the
    /// clip or stretches it."* Off by default, because a take has to sound
    /// like the take. A switch rather than a modifier because it is a
    /// setting you work in, not a thing you hold.
    Stretch,
    /// The selection again, after itself — the answer to *"I made this drum
    /// loop but I can't repeat it"*.
    ///
    /// A **copy**: new clips with notes of their own, which edit
    /// independently. [`Loop`](Self::Loop) is the other reading of the same
    /// sentence and they are deliberately two buttons, because they are two
    /// features and conflating them is what the arrangement used to do.
    Repeat,
    /// Makes the selection repeat its own content, or stops it.
    ///
    /// One clip, one set of notes, played again every period — so editing bar
    /// one changes every repeat. Shift-dragging a clip's right-hand grip is
    /// the same thing without coming up here for it.
    Loop,
    Cut,
    Copy,
    Paste,
    /// Mutes the selection, or unmutes it when all of it is muted.
    Mute,
    ZoomOut,
    ZoomIn,
}

impl TimelineControl {
    /// What the button says. The snap chip is the exception — it says its
    /// division, because the state is the useful half.
    pub fn label(self) -> &'static str {
        match self {
            Self::Draw => "Draw",
            Self::Select => "Sel",
            Self::Slice => "Cut",
            Self::Snap => "snap",
            Self::Stretch => "Stretch",
            Self::Repeat => "Repeat",
            Self::Loop => "Loop",
            Self::Cut => "Cut",
            Self::Copy => "Copy",
            Self::Paste => "Paste",
            Self::Mute => "Mute",
            Self::ZoomOut => "-",
            Self::ZoomIn => "+",
        }
    }

    /// The glyph this control draws, or `None` when it draws its own value.
    /// The same rule the roll's toolbar follows — see [`crate::canvas::RollControl::icon`].
    pub fn icon(self) -> Option<crate::icon::Icon> {
        use crate::icon::Icon;
        Some(match self {
            Self::Draw => Icon::Pencil,
            Self::Select => Icon::Marquee,
            // A blade, not the scissors `Cut` uses: dividing a clip and taking
            // one away are two different things.
            Self::Slice => Icon::Blade,
            // One clip coming round again, against copies laid after it. Two
            // features, two pictures.
            Self::Loop => Icon::Loop,
            Self::Repeat => Icon::Repeat,
            Self::Cut => Icon::Cut,
            Self::Copy => Icon::Copy,
            Self::Paste => Icon::Paste,
            Self::Mute => Icon::Mute,
            Self::ZoomOut => Icon::ZoomOut,
            Self::ZoomIn => Icon::ZoomIn,
            // The snap chip says its division, which is a value — and the
            // stretch switch says its word, which is the state you read.
            Self::Snap | Self::Stretch => return None,
        })
    }

    /// What a hover tip says (see [`crate::tooltip`]).
    ///
    /// Beside `label` and `icon` rather than in a table somewhere else, so
    /// that a control added without an explanation is a hole in this match
    /// rather than a silent miss.
    ///
    /// The shortcut is **not** repeated here — the renderer appends
    /// [`shortcut`](Self::shortcut) to the tip it draws, so the two cannot
    /// drift.
    pub fn tip(self) -> Option<&'static str> {
        Some(match self {
            Self::Draw => "Double-click empty bars for a clip, Shift+click for a copy",
            Self::Select => "Select clips; drag on empty bars to marquee",
            Self::Slice => "Cut a clip in two where you click it",
            Self::Snap => "What clips snap to \u{2014} click to choose",
            Self::Stretch => {
                // The key is on the tip, the way Legato's is on its menu row:
                // a control nobody can find is a control nobody uses.
                "S \u{2014} an edge drag fits the take to the block; off, it trims"
            }
            // The two readings of "repeat this" are two buttons on purpose,
            // and the tips are where the difference is said out loud.
            Self::Repeat => "Copy the selection after itself \u{2014} edits apart",
            Self::Loop => "Repeat one clip's own notes \u{2014} edits together",
            Self::Cut => "Cut the selected clips",
            Self::Copy => "Copy the selected clips",
            Self::Paste => "Paste at the marker",
            Self::Mute => "Mute the selected clips",
            Self::ZoomOut => "Zoom out",
            Self::ZoomIn => "Zoom in",
        })
    }

    /// The keyboard shortcut worth writing down, if there is one.
    pub fn shortcut(self) -> Option<&'static str> {
        match self {
            Self::Draw => Some("P"),
            Self::Select => Some("E"),
            Self::Slice => Some("C"),
            Self::Repeat => Some("Ctrl+B"),
            Self::Loop => Some("Shift+drag"),
            Self::Cut => Some("Ctrl+X"),
            Self::Copy => Some("Ctrl+C"),
            Self::Paste => Some("Ctrl+V"),
            Self::Mute => Some("Ctrl+Shift+M"),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimelineToolbar {
    pub items: Vec<(TimelineControl, Rect)>,
}

/// The controls, left to right: what the grid is, then what to do to a clip,
/// then how close you are looking. The same order as the roll's toolbar, for
/// the same reason — the two panels are read the same way.
const TIMELINE_TOOLBAR: [(TimelineControl, f32); 13] = [
    // The tools first: they decide what every other press on the grid means,
    // and the roll's toolbar is read the same way round.
    (TimelineControl::Draw, 26.0),
    (TimelineControl::Select, 26.0),
    (TimelineControl::Slice, 26.0),
    (TimelineControl::Snap, 62.0),
    // Beside snap, because it is the same kind of thing: not an action but
    // what the next drag means.
    (TimelineControl::Stretch, 52.0),
    // Loop next to Repeat, deliberately: they are the two readings of "play
    // this again" and seeing them side by side is what teaches the
    // difference. One makes copies you edit separately; the other makes one
    // clip come round again.
    (TimelineControl::Loop, 26.0),
    (TimelineControl::Repeat, 26.0),
    (TimelineControl::Cut, 26.0),
    (TimelineControl::Copy, 26.0),
    (TimelineControl::Paste, 26.0),
    (TimelineControl::Mute, 26.0),
    (TimelineControl::ZoomOut, 26.0),
    (TimelineControl::ZoomIn, 26.0),
];

const TOOLBAR_PAD: f32 = 4.0;
const TOOLBAR_GAP: f32 = 4.0;

/// Lays the arrangement's controls out inside `toolbar`.
///
/// A control that will not fit is **left out** rather than squeezed or drawn
/// off the end — the same rule the roll's toolbar follows, and the reason a
/// narrow panel degrades to the controls it has room for instead of a row of
/// overlapping boxes.
pub fn timeline_toolbar_layout(toolbar: Rect, metrics: &Metrics) -> TimelineToolbar {
    let mut items = Vec::with_capacity(TIMELINE_TOOLBAR.len());
    if toolbar.is_empty() {
        return TimelineToolbar { items };
    }
    // Never taller than a row, so a toolbar in a stretched panel stays a
    // toolbar rather than a band of tall buttons.
    let height = (toolbar.height - TOOLBAR_PAD).clamp(0.0, metrics.row_height);
    let mut x = toolbar.x + TOOLBAR_PAD;
    for (control, width) in TIMELINE_TOOLBAR {
        if x + width > toolbar.right() - TOOLBAR_PAD {
            continue;
        }
        items.push((
            control,
            Rect::new(x, toolbar.y + TOOLBAR_PAD / 2.0, width, height).clamped(),
        ));
        x += width + TOOLBAR_GAP;
    }
    TimelineToolbar { items }
}

/// Which control `(x, y)` is on, if any.
pub fn timeline_toolbar_hit(bar: &TimelineToolbar, x: f32, y: f32) -> Option<TimelineControl> {
    bar.items
        .iter()
        .find(|(_, rect)| rect.contains(x, y))
        .map(|(control, _)| *control)
}

// ------------------------------------------------------------- geometry ---

pub fn timeline_tick_to_x(view: &TimelineView, grid: Rect, tick: Tick) -> f32 {
    grid.x + (tick - view.scroll_tick) as f32 * view.pixels_per_tick
}

/// Clamped at zero: left of bar 1 is the start of the song, not a negative tick
/// every conversion downstream would have to defend against.
pub fn timeline_x_to_tick(view: &TimelineView, grid: Rect, x: f32) -> Tick {
    if view.pixels_per_tick <= 0.0 {
        return view.scroll_tick.max(0);
    }
    let tick = view.scroll_tick as f32 + (x - grid.x) / view.pixels_per_tick;
    (tick.round() as Tick).max(0)
}

pub fn lane_to_y(view: &TimelineView, grid: Rect, lane: usize) -> f32 {
    grid.y + (lane as f32 - view.top_lane as f32) * view.lane_height
}

pub fn y_to_lane(view: &TimelineView, grid: Rect, y: f32) -> usize {
    if view.lane_height <= 0.0 {
        return view.top_lane;
    }
    let rows = ((y - grid.y) / view.lane_height).floor() as i64;
    (view.top_lane as i64 + rows).max(0) as usize
}

/// The tick range the grid can show, inclusive of the partly visible column on
/// the right — build geometry for this and nothing else (§16.4).
pub fn timeline_visible_ticks(view: &TimelineView, grid: Rect) -> Range<Tick> {
    if view.pixels_per_tick <= 0.0 || grid.width <= 0.0 {
        return view.scroll_tick..view.scroll_tick;
    }
    let span = (grid.width / view.pixels_per_tick).ceil() as Tick;
    view.scroll_tick..view.scroll_tick + span + 1
}

/// The lanes the grid can show, clamped to how many the project has.
pub fn visible_lanes(view: &TimelineView, grid: Rect, lane_count: usize) -> Range<usize> {
    if view.lane_height <= 0.0 || grid.height <= 0.0 || lane_count == 0 {
        return 0..0;
    }
    let rows = (grid.height / view.lane_height).ceil() as usize + 1;
    let top = view.top_lane.min(lane_count.saturating_sub(1));
    top..(top + rows).min(lane_count)
}

/// The `top_lane` that brings `lane` into view, leaving the scroll exactly
/// where it is when that lane is already on screen.
///
/// The arrangement's answer to [`crate::canvas::scroll_to_show`], and it
/// exists for the same reason the rack's does: something has just been added
/// to the **end** of a list, and a list scrolled away from what you just made
/// is a gesture that looks like it did nothing. An imported sound arrives on a
/// lane past the bottom of the stack (`fontelle_model::AddAudioClip`), so in
/// any project with a screenful of lanes that is exactly where it lands.
///
/// Whole rows only: a lane brought half into view at the bottom edge is a clip
/// you still cannot read.
pub fn lane_scroll_to_show(view: &TimelineView, grid: Rect, lane: usize) -> usize {
    if view.lane_height <= 0.0 || grid.height < view.lane_height {
        return view.top_lane;
    }
    let rows = (grid.height / view.lane_height).floor().max(1.0) as usize;
    if lane < view.top_lane {
        lane
    } else if lane >= view.top_lane + rows {
        lane + 1 - rows
    } else {
        view.top_lane
    }
}

/// The block a clip draws as.
///
/// Always at least a pixel wide, for the same reason a note is: a clip too
/// short to see is still a clip, and one that vanishes cannot be clicked to
/// find out why.
/// Where a line drawn from `from` to `to` cuts the clips it crosses.
///
/// The arrangement's half of [`crate::canvas::slice_cuts`], and the same rule:
/// **a clip is cut where the line crosses the middle of its own row**, and only
/// if that lands strictly inside the clip. What follows from it is what makes
/// the gesture worth having — a diagonal stroke cuts four rows at four
/// different bars, and a line drawn *along* a row never crosses its middle and
/// so cuts nothing, rather than cutting somewhere nobody aimed at.
///
/// The cut is **snapped**, unlike the roll's, because a clip boundary half a
/// beat off the bar is a boundary somebody has to nudge before they can use it
/// — and a cut snapped out of its own clip lands on the edge, where it is
/// refused, which is the honest answer to "you aimed at a bar this clip does
/// not cover".
pub fn clip_cuts(
    view: &TimelineView,
    grid: Rect,
    clips: &[ClipInfo],
    from: (f32, f32),
    to: (f32, f32),
    snap: SnapDivision,
    beats_per_bar: u32,
) -> Vec<(ClipId, Tick)> {
    let mut cuts = Vec::new();
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    // A press with no drag is somebody putting the pointer down, not a cut.
    if dx.abs() < f32::EPSILON && dy.abs() < f32::EPSILON {
        return cuts;
    }
    if dy.abs() < f32::EPSILON {
        return cuts; // drawn along the rows: it crosses none of their middles
    }

    let snapper = TimelineView { snap, ..*view };
    for clip in clips {
        let row = lane_to_y(view, grid, clip.lane) + view.lane_height / 2.0;
        let along = (row - from.1) / dy;
        if !(0.0..=1.0).contains(&along) {
            continue; // the row is beyond one end of the stroke
        }
        let x = from.0 + dx * along;
        let at = timeline_snap(&snapper, timeline_x_to_tick(view, grid, x), beats_per_bar);
        if at <= clip.start || at >= clip.start + clip.length {
            continue;
        }
        cuts.push((clip.id, at));
    }
    cuts
}

/// **Where the blade will actually cut**, as a mark per cut.
///
/// > *"it is displaying the actual visuals of the tool from the exact pixel of
/// > where I'm clicking and dragging my mouse instead of actually displaying
/// > it rounded to the grid that it's going to cut the actual clip at."*
///
/// The stroke and the cut are two different things and always were: a cut
/// snaps to the arrangement's grid, per clip, in [`clip_cuts`]. Drawing the
/// stroke was drawing the gesture rather than its consequence, so a line
/// aimed a third of a beat late looked like a cut a third of a beat late and
/// landed on the beat.
///
/// This is the consequence: it asks [`clip_cuts`] the same question the
/// release will ask, and turns each answer into a rectangle across that clip's
/// own row. **The same call**, so the two cannot drift apart — a preview
/// computed from a second copy of the snapping rule is a preview that is right
/// until somebody edits one of them.
pub fn slice_marks(
    view: &TimelineView,
    grid: Rect,
    clips: &[ClipInfo],
    from: (f32, f32),
    to: (f32, f32),
    snap: SnapDivision,
    beats_per_bar: u32,
) -> Vec<Rect> {
    /// How wide the mark is. Two pixels: thick enough to see against a clip's
    /// own fill, thin enough that it reads as a *place* rather than as a
    /// region — a cut has no width.
    const WIDTH: f32 = 2.0;
    clip_cuts(view, grid, clips, from, to, snap, beats_per_bar)
        .into_iter()
        .filter_map(|(id, at)| {
            let clip = clips.iter().find(|clip| clip.id == id)?;
            let x = timeline_tick_to_x(view, grid, at);
            let y = lane_to_y(view, grid, clip.lane);
            Some(Rect::new(x - WIDTH / 2.0, y, WIDTH, view.lane_height).intersection(&grid))
        })
        .filter(|mark| !mark.is_empty())
        .collect()
}

/// Two blocks on one row, and the part of the row they both claim.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipOverlap {
    /// What they share, clipped to the grid — where the stripes go.
    pub area: Rect,
    /// The **later** clip coming in over the overlap, as a polyline across
    /// the content band. Empty unless the two actually crossfade — see
    /// [`clip_overlaps`].
    pub fade_in: Vec<(f32, f32)>,
    /// The **earlier** clip going out over it, crossing the other.
    pub fade_out: Vec<(f32, f32)>,
}

/// Where clips on one row lie over each other, and what that overlap sounds
/// like.
///
/// *"when theyre overlapped, there should be a kind of diagonal stripe
/// pattern on the overlapping part so the user can see what parts are
/// overlapping."* One entry per overlapping **pair**, measured on the same
/// block rectangles the pointer is tested against and clipped to the grid,
/// so the stripes fall exactly where two blocks share pixels. Clips that
/// only touch share nothing: end-to-end is the ordinary shape of a song, and
/// a stripe at every seam would say every song was wrong.
///
/// # The curves
///
/// > *"i do want it to also show the graph line drawn to show the fade on
/// > the overlap as well so it resembles the other fades but just with them
/// > crossing through eachother like how fls displays."*
///
/// Two overlapping **audio** clips crossfade, and the stripes alone say that
/// something happens there without saying what. So the pair carries the two
/// curves as well: the later clip rising, the earlier one falling, crossing
/// at three decibels down apiece.
///
/// They are the player's own envelope rather than a decoration, and the
/// rules are the compiler's (`fontelle_sequencer`), so that what is drawn is
/// what is heard:
///
/// - **Equal power** — a sine against a cosine, the shape
///   `AudioPlacement::auto_gain` applies. Two different recordings blended
///   linearly dip three decibels in the middle.
/// - **Times the clip's own fade**, because both apply: a clip's fades go
///   with it wherever it is put, and the crossfade is about the clip beside
///   it.
/// - Only a clip whose **end** the overlap reaches fades out, so a clip
///   dropped wholly inside another comes in and the outer one carries on.
/// - A muted clip, and anything that is not audio, is placed with no
///   crossfade — and is drawn with none. (A clip on a muted *row* is the one
///   case this cannot see, the same blind spot the waveform and the fade
///   handles have.)
pub fn clip_overlaps(view: &TimelineView, grid: Rect, clips: &[ClipInfo]) -> Vec<ClipOverlap> {
    let mut shared = Vec::new();
    for (i, a) in clips.iter().enumerate() {
        for b in &clips[i + 1..] {
            if a.lane != b.lane {
                continue;
            }
            let from = a.start.max(b.start);
            let to = (a.start + a.length).min(b.start + b.length);
            if to <= from {
                continue;
            }
            let area = clip_rect(view, grid, a)
                .intersection(&clip_rect(view, grid, b))
                .intersection(&grid);
            if area.is_empty() {
                continue;
            }
            // Which one is leaving and which is arriving, tie broken the way
            // the compiler breaks it — by position in the list.
            let (earlier, later) = if a.start <= b.start { (a, b) } else { (b, a) };
            let (mut fade_in, mut fade_out) = (Vec::new(), Vec::new());
            let crossfades = [earlier, later]
                .iter()
                .all(|clip| clip.kind == ClipKind::Audio && !clip.muted);
            if crossfades {
                // Against the **whole** span, not the part on screen: a
                // curve measured from the visible part would slide as the
                // arrangement scrolled. The renderer clips it to `area`.
                let x0 = timeline_tick_to_x(view, grid, from);
                let x1 = timeline_tick_to_x(view, grid, to);
                let (_, content) = clip_bands(clip_rect(view, grid, earlier));
                if content.height > 0.0 && x1 > x0 {
                    let point = |clip: &ClipInfo, t: f32, gain: f32| {
                        let x = x0 + (x1 - x0) * t;
                        // The clip's own fade where this point falls on it —
                        // and nothing at all past its file's end, which is
                        // where an unstretched clip has gone quiet.
                        let block = clip_rect(view, grid, clip);
                        let along = if block.width > 0.0 {
                            (x - block.x) * clip.length as f32 / block.width
                        } else {
                            0.0
                        };
                        let own = content_fraction(clip, along)
                            .map_or(0.0, |t| preview_fade(&clip.audio, t));
                        (x, content.bottom() - content.height * gain * own)
                    };
                    fade_in = (0..=FADE_CURVE_POINTS)
                        .map(|i| {
                            let t = i as f32 / FADE_CURVE_POINTS as f32;
                            point(later, t, (t * std::f32::consts::FRAC_PI_2).sin())
                        })
                        .collect();
                    // Only a clip the overlap carries to its end has a tail
                    // to fade: a clip dropped inside another does not end
                    // here, and neither does the one around it.
                    if earlier.start + earlier.length <= later.start + later.length {
                        fade_out = (0..=FADE_CURVE_POINTS)
                            .map(|i| {
                                let t = i as f32 / FADE_CURVE_POINTS as f32;
                                point(earlier, t, (t * std::f32::consts::FRAC_PI_2).cos())
                            })
                            .collect();
                    }
                }
            }
            shared.push(ClipOverlap {
                area,
                fade_in,
                fade_out,
            });
        }
    }
    shared
}

pub fn clip_rect(view: &TimelineView, grid: Rect, clip: &ClipInfo) -> Rect {
    let x0 = timeline_tick_to_x(view, grid, clip.start);
    let x1 = timeline_tick_to_x(view, grid, clip.start + clip.length);
    Rect::new(
        x0,
        lane_to_y(view, grid, clip.lane),
        (x1 - x0).max(1.0),
        view.lane_height,
    )
}

/// Zooms time about `anchor_x`, so whatever is under the pointer stays under
/// it — [`crate::canvas::zoom_x`]'s counterpart, and the same arithmetic.
pub fn timeline_zoom_x(view: &mut TimelineView, grid: Rect, anchor_x: f32, factor: f32) {
    if view.pixels_per_tick <= 0.0 || !factor.is_finite() || factor <= 0.0 {
        return;
    }
    let offset = f64::from(anchor_x - grid.x);
    let anchor_tick = view.scroll_tick as f64 + offset / f64::from(view.pixels_per_tick);
    view.pixels_per_tick =
        (view.pixels_per_tick * factor).clamp(MIN_TIMELINE_PPT, MAX_TIMELINE_PPT);
    let scroll = anchor_tick - offset / f64::from(view.pixels_per_tick);
    view.scroll_tick = (scroll.round() as Tick).max(0);
}

pub fn timeline_zoom_y(view: &mut TimelineView, factor: f32) {
    if !factor.is_finite() || factor <= 0.0 {
        return;
    }
    view.lane_height = (view.lane_height * factor).clamp(MIN_LANE_ROW, MAX_LANE_ROW);
}

// ---------------------------------------------------------- hit-testing ---

/// Which bit of a clip is under the pointer.
///
/// A note block has a body and a grip. An automation block has those — its
/// body is the caption band across its top — and, under the band, the curve
/// it is for: a point of it, or the bare curve between points. See
/// `canvas::automation_block` for the anatomy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClipPart {
    Body,
    RightEdge,
    /// The fade handle at one top corner of an audio block — see
    /// [`fade_anatomy`]. Dragged along the block, it sets how long the fade
    /// at that end is.
    FadeHandle(FadeEnd),
    /// The node at the midpoint of an audio block's fade curve. Dragged up
    /// or down, it bends the fade.
    FadeNode(FadeEnd),
    /// A point of an automation block's curve.
    Point(PointId),
    /// The curve area of an automation block, away from any point, at this
    /// tick **of the clip** (unsnapped) and this value.
    Curve {
        tick: Tick,
        value: f64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TimelineHit {
    Clip(ClipId, ClipPart),
    /// Bare grid. The tick is **unsnapped**, for the reason
    /// [`crate::canvas::RollHit::Empty`] gives.
    Empty {
        tick: Tick,
        lane: usize,
    },
    /// The bar ruler, at this tick — where the time marker is set.
    Ruler(Tick),
    /// A lane's name column.
    Lane(usize),
    Outside,
}

pub fn timeline_hit(
    view: &TimelineView,
    layout: &TimelineLayout,
    clips: &[ClipInfo],
    x: f32,
    y: f32,
) -> TimelineHit {
    if layout.ruler.contains(x, y) {
        // Measured against the *grid*, not the ruler: the ruler runs the whole
        // width of the panel and the grid starts after the header column, so
        // taking the ruler's own x would put every bar line in the wrong place.
        return TimelineHit::Ruler(timeline_x_to_tick(view, layout.grid, x.max(layout.grid.x)));
    }
    if layout.headers.contains(x, y) {
        return TimelineHit::Lane(y_to_lane(view, layout.grid, y));
    }
    if !layout.grid.contains(x, y) {
        return TimelineHit::Outside;
    }

    // Last match wins: later clips are drawn over earlier ones, so the one on
    // top is the one that was clicked.
    let mut found = None;
    for clip in clips {
        let block = clip_rect(view, layout.grid, clip);
        if !block.contains(x, y) {
            continue;
        }
        let handle = clip_grip(block);
        // An audio block's fade handles and nodes come before its grip: the
        // out handle shares the top-right corner with the grip, and a corner
        // that always meant "resize" would leave no way to fade out.
        let fade = fade_anatomy(block, clip).and_then(|anatomy| {
            if anatomy.node_in.is_some_and(|node| node.contains(x, y)) {
                Some(ClipPart::FadeNode(FadeEnd::In))
            } else if anatomy.node_out.is_some_and(|node| node.contains(x, y)) {
                Some(ClipPart::FadeNode(FadeEnd::Out))
            } else if anatomy.handle_in.contains(x, y) {
                Some(ClipPart::FadeHandle(FadeEnd::In))
            } else if anatomy.handle_out.contains(x, y) {
                Some(ClipPart::FadeHandle(FadeEnd::Out))
            } else {
                None
            }
        });
        let part = if let Some(part) = fade {
            part
        } else if x >= block.right() - handle {
            ClipPart::RightEdge
        } else if clip.kind == ClipKind::Automation {
            automation_part(block, clip, x, y)
        } else {
            ClipPart::Body
        };
        found = Some(TimelineHit::Clip(clip.id, part));
    }
    found.unwrap_or(TimelineHit::Empty {
        tick: timeline_x_to_tick(view, layout.grid, x),
        lane: y_to_lane(view, layout.grid, y),
    })
}

/// What is under `(x, y)` inside an automation block, the grip aside.
///
/// The band is the block; a handle is its point — the last one first, since
/// points drawn later sit on top; and anything else under the band is the
/// curve, reported with the tick and value the pointer is at so a press can
/// make a point there.
fn automation_part(block: Rect, clip: &ClipInfo, x: f32, y: f32) -> ClipPart {
    let anatomy = automation_block(block, clip);
    if anatomy.header.contains(x, y) {
        return ClipPart::Body;
    }
    if let Some((id, _)) = anatomy
        .handles
        .iter()
        .rev()
        .find(|(_, rect)| rect.contains(x, y))
    {
        return ClipPart::Point(*id);
    }
    ClipPart::Curve {
        tick: block_tick_at(anatomy.area, clip.length, x),
        value: block_value_at(anatomy.area, y),
    }
}

// ----------------------------------------------------- time selection ---

/// The stretch of time a right-drag along a ruler selects, on the grid.
///
/// *"right click and drag on the time bar to loop a time section you are
/// editing either in the piano roll or arrangement."* One function for both
/// rulers: `anchor` is where the button went down and `to` is where the
/// pointer is now, both unsnapped; the answer is both ends on the grid, in
/// order, never before the song. A drag whose ends land on the same line is
/// a click, and a click **clears** the selection — `None` — which is what
/// makes the same button both make and unmake a loop.
pub fn time_selection(
    anchor: Tick,
    to: Tick,
    snap: SnapDivision,
    beats_per_bar: u32,
) -> Option<(Tick, Tick)> {
    let a = snap_tick(anchor.max(0), snap, beats_per_bar);
    let b = snap_tick(to.max(0), snap, beats_per_bar);
    let (from, to) = (a.min(b), a.max(b));
    (to > from).then_some((from, to))
}

// -------------------------------------------------------------- editing ---

/// What the arrangement wants done to the document.
///
/// Values, not commands, for the reason [`crate::canvas::RollEdit`] gives.
#[derive(Debug, Clone, PartialEq)]
pub enum ArrangeEdit {
    /// Deltas, **relative to the previous step of the same drag** — which is
    /// what `MoveClip` takes and what lets `merge_with` coalesce a drag into
    /// one history entry.
    Move {
        ids: Vec<ClipId>,
        tick_delta: Tick,
        lane_delta: i32,
    },
    Resize {
        ids: Vec<ClipId>,
        tick_delta: Tick,
    },
    Duplicate {
        ids: Vec<ClipId>,
        tick_offset: Tick,
    },
    Remove(Vec<ClipId>),
    SetMuted {
        ids: Vec<ClipId>,
        muted: bool,
    },
    /// Keep these clips, so a later [`Paste`](Self::Paste) can put them down
    /// again.
    ///
    /// The clips themselves are **the host's** to hold, not this canvas's. A
    /// [`ClipInfo`] is a flattened view for drawing — no notes, no channel —
    /// so a canvas holding copied clips would be holding something INVARIANT 2
    /// says it may not see. It is also what makes cut work: paste has to put
    /// something down after the clip it copied from has been deleted.
    Copy(Vec<ClipId>),
    /// Put whatever was copied down, earliest clip at `at`.
    Paste {
        at: Tick,
    },
    /// Make a clip on `lane`, starting at `start` — the draw tool.
    ///
    /// The one thing the arrangement could not do: it could move, resize,
    /// duplicate, delete, copy, cut, paste, mute and loop clips, and a press
    /// on empty grid always started a marquee. What channel the clip plays and
    /// how long it is are the host's to decide — a canvas may not see a
    /// `Project` (INVARIANT 2), and "the channel already on this lane" is a
    /// question only the document can answer.
    Add {
        lane: usize,
        start: Tick,
    },
    /// A copy of `source` — whatever kind of clip it is, notes and settings
    /// and all — put down on `lane` at `start`.
    ///
    /// *"to create a copy of your last selected item its shift + click."* The
    /// host copies; the canvas only says which and where, because it cannot
    /// see what a clip holds (INVARIANT 2).
    Stamp {
        source: ClipId,
        lane: usize,
        start: Tick,
    },
    /// An audio clip's fade at one end, as a **fraction of the block** —
    /// the canvas can see the block and not the file, and the host turns it
    /// into frames (TDD §15.2). Absolute, not a delta: a fade is *how far
    /// the handle was dragged*, which is a position.
    SetFade {
        clip: ClipId,
        end: FadeEnd,
        fraction: f32,
    },
    /// The bend of that fade's curve, −1..1 — see `fontelle_types::Fade`.
    SetFadeTension {
        clip: ClipId,
        end: FadeEnd,
        tension: f32,
    },
    /// Cut each of these clips in two, at the song tick given.
    ///
    /// The arrangement's cut tool. One edit carrying every clip the stroke
    /// crossed, because a line drawn across four rows is **one gesture** and
    /// taking it back should be one press of Ctrl+Z.
    Split {
        cuts: Vec<(ClipId, Tick)>,
    },
    /// Make these clips repeat their content every `loop_length` ticks, or
    /// stop them repeating.
    ///
    /// **Not a duplicate.** A duplicate makes new clips with notes of their
    /// own; this is one clip whose one set of notes plays again every period,
    /// so editing bar 1 changes every repeat. See `fontelle_model::Clip`'s
    /// `loop_length`.
    SetLoop {
        ids: Vec<ClipId>,
        loop_length: Option<Tick>,
    },
    /// Put these audio clips in this mode — see [`TimelineControl::Stretch`].
    ///
    /// Sent once, as the first step of an edge drag, and only for the clips
    /// the switch would actually change; a note clip is never named, since
    /// its edge drag has only ever cut. The host writes it to the clip's own
    /// `ClipStretch`, which is what the player reads, so the switch and the
    /// sound cannot disagree.
    SetStretch {
        ids: Vec<ClipId>,
        stretch: ClipStretch,
    },
    /// Put a point on an automation clip's curve, at `tick` **of the clip**.
    ///
    /// The host hands the new point's id back (`Created::points`), and the
    /// drag that made it carries it — see [`Timeline::points_inserted`].
    AddPoint {
        clip: ClipId,
        tick: Tick,
        value: f64,
    },
    /// Deltas, **relative to the previous step of the same drag**, like
    /// [`Move`](Self::Move) and for the same reason.
    MovePoints {
        clip: ClipId,
        ids: Vec<PointId>,
        tick_delta: Tick,
        value_delta: f64,
    },
    RemovePoints {
        clip: ClipId,
        ids: Vec<PointId>,
    },
    /// The shape of the segment after each of these points.
    SetPointCurve {
        clip: ClipId,
        ids: Vec<PointId>,
        curve: fontelle_model::CurveShape,
    },
}

/// How far a point may go before the clip would refuse it, **measured once
/// when the gesture started** — [`MoveLimits`]'s reason exactly.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct PointLimits {
    min_tick: Tick,
    max_tick: Tick,
    min_value: f64,
    max_value: f64,
}

impl PointLimits {
    fn of(clip: &ClipInfo, selection: &[PointId]) -> Self {
        let mut limits = Self {
            min_tick: Tick::MIN,
            max_tick: Tick::MAX,
            min_value: f64::MIN,
            max_value: f64::MAX,
        };
        for point in clip.curve.iter().filter(|p| selection.contains(&p.id)) {
            limits.min_tick = limits.min_tick.max(-point.tick);
            limits.max_tick = limits.max_tick.min(clip.length - point.tick);
            limits.min_value = limits.min_value.max(-point.value);
            limits.max_value = limits.max_value.min(1.0 - point.value);
        }
        limits
    }

    /// For a point that has just been made at `(tick, value)` and is not yet
    /// in anybody's list.
    fn at(clip: &ClipInfo, tick: Tick, value: f64) -> Self {
        Self {
            min_tick: -tick,
            max_tick: clip.length - tick,
            min_value: -value,
            max_value: 1.0 - value,
        }
    }
}

/// How far a move may go before it would take a clip somewhere the document
/// would refuse, **measured once when the gesture started**.
///
/// The same type and the same reason as the roll's `MoveLimits`: a drag emits
/// deltas relative to its own previous step, so a clamp recomputed from the
/// live clips — which the drag is changing, and which the window refreshes
/// between events — chases the thing it is clamping. The clip reaches bar one,
/// the clamp becomes "cannot move at all", `wanted` snaps to zero against an
/// `applied` of minus four bars, and the clip is asked to jump four bars
/// forward. Then back. See `tests/arrange_gestures.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct MoveLimits {
    /// The furthest back in time the selection may go.
    min_tick: Tick,
    /// And the furthest up the lanes.
    min_lane: i32,
}

impl MoveLimits {
    fn of(selection: &[ClipId], clips: &[ClipInfo]) -> Self {
        let mut earliest = Tick::MAX;
        let mut highest = i32::MAX;
        for clip in clips.iter().filter(|c| selection.contains(&c.id)) {
            earliest = earliest.min(clip.start);
            highest = highest.min(clip.lane as i32);
        }
        if earliest == Tick::MAX {
            return Self::default();
        }
        Self {
            min_tick: -earliest,
            min_lane: -highest,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Gesture {
    None,
    /// The cut tool, drawing its line. Like the roll's, and for the same
    /// reason: the cut lands on **release**, because a line half-drawn is not
    /// a cut.
    Slicing {
        from: (f32, f32),
        to: (f32, f32),
    },
    Moving {
        applied_tick: Tick,
        applied_lane: i32,
        limits: MoveLimits,
    },
    Resizing {
        applied_tick: Tick,
        /// The shortest clip in the selection when the gesture started — same
        /// reason as [`MoveLimits`].
        shortest: Tick,
        /// Shift was held when the grip was taken, so this drag is making the
        /// clip **loop** rather than merely making it longer.
        ///
        /// Decided at the press and kept, not read live: a modifier let go
        /// halfway through a drag must not turn a loop back into a stretch
        /// under the pointer. The same rule the roll's gestures follow.
        looping: bool,
        /// The period to set, measured **when the drag started**: the length
        /// the clip already loops at, or the length it was. Kept for the same
        /// reason `shortest` is — the drag is changing the clip it would
        /// otherwise be reading.
        period: Tick,
        /// The Stretch switch as it was at the press, kept for the reason
        /// `looping` is: flipped halfway through a drag, it must not change
        /// what the drag has been doing.
        stretch: bool,
    },
    Marquee {
        from: (f32, f32),
        to: (f32, f32),
    },
    /// A fade handle, carried along the block. What has been asked for so
    /// far, so a stationary pointer asks for nothing new.
    Fading {
        clip: ClipId,
        end: FadeEnd,
        applied: Option<f32>,
    },
    /// A fade's node, carried up or down the content band.
    Bending {
        clip: ClipId,
        end: FadeEnd,
        applied: Option<f32>,
    },
    /// Points on an automation block's curve, carried. **Relative**, like a
    /// note drag and for the same reason: several can move at once and they
    /// have to keep their shape.
    MovingPoints {
        clip: ClipId,
        /// The clip tick and the value under the pointer when it was pressed.
        origin_tick: Tick,
        origin_value: f64,
        applied_tick: Tick,
        applied_value: f64,
        limits: PointLimits,
        /// The press made the point and the host has not yet said which
        /// point it made. Nothing moves until it does.
        pending: bool,
    },
    /// Rubbing clips out: the right button held down. Stateless, for the same
    /// reason the roll's `Erasing` is — it asks the current clips what is
    /// under the pointer, so one already removed is simply not found again.
    Erasing,
}

/// What a press on empty grid means.
///
/// FL Studio's two, and the roll next door already works this way — so it is
/// one idea rather than two. **Draw is the default**, because the first thing
/// anybody wants from an empty arrangement is a clip, and a marquee over
/// nothing selects nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimelineTool {
    /// A double-click on empty grid makes a clip and a Shift+press puts down
    /// a copy of the clip in hand; a plain press only lets go of the
    /// selection. See `Timeline::press`.
    #[default]
    Draw,
    /// A press on empty grid marquees.
    Select,
    /// **Cut**: a press on a clip divides it where you pressed.
    ///
    /// FL Studio's, and the same key (`C`) the piano roll's slice tool has —
    /// which is what somebody using the studio reached for and did not find:
    /// *"theres no tool for cutting up clips in the arrangement right now
    /// (should be c key)."*
    Slice,
}

/// The arrangement's own state: where it is looking, what is selected, what the
/// mouse is doing.
pub struct Timeline {
    pub view: TimelineView,
    tool: TimelineTool,
    /// The Stretch switch — see [`TimelineControl::Stretch`]. Off until it
    /// is pressed.
    stretch: bool,
    selection: Vec<ClipId>,
    gesture: Gesture,
    /// What is held down. The window pushes these in rather than the canvas
    /// asking, because only the window sees a `ModifiersChanged` — the same
    /// shape `PianoRoll::set_modifiers` has.
    modifiers: Modifiers,
    /// Where the pointer was when the gesture started, in document units.
    origin: (Tick, usize),
    /// A clip the canvas wants the roll to open, waiting to be collected.
    ///
    /// Handed over rather than acted on, because opening a clip is the
    /// *document's* business (it changes which channel is selected) and this
    /// canvas may not touch the document (INVARIANT 2).
    open: Option<ClipId>,
    /// The selected points of an automation block, and whose they are.
    ///
    /// One clip's at a time: a point selection spanning two clips is two
    /// curves moved by one drag, which nobody asks for and nothing draws.
    point_clip: Option<ClipId>,
    point_selection: Vec<PointId>,
    /// A point the right button asked about, waiting to be collected — the
    /// same handshake `open` is, for the same reason: the menu is the
    /// window's.
    point_menu: Option<(ClipId, PointId)>,
    /// **The clip in hand**: the last one chosen, of any kind, which is what
    /// a Shift+press on empty grid puts a copy of down. Outlives the
    /// selection — Escape drops the selection and keeps this, the way FL
    /// keeps the pattern you picked after you click away from it. See
    /// [`ArrangeEdit::Stamp`].
    stamp: Option<ClipId>,
    /// The last press asked for a stamp, and the copy it makes is not known
    /// yet. `clips_inserted` makes the copy the thing in hand.
    stamp_pending: bool,
}

impl Timeline {
    pub fn new(view: TimelineView) -> Self {
        Self {
            view,
            tool: TimelineTool::default(),
            stretch: false,
            selection: Vec::new(),
            gesture: Gesture::None,
            modifiers: Modifiers::default(),
            origin: (0, 0),
            open: None,
            point_clip: None,
            point_selection: Vec::new(),
            point_menu: None,
            stamp: None,
            stamp_pending: false,
        }
    }

    /// The clip a Shift+press on empty grid would copy, if any.
    pub fn stamp_source(&self) -> Option<ClipId> {
        self.stamp
    }

    pub fn selection(&self) -> &[ClipId] {
        &self.selection
    }

    pub fn select(&mut self, ids: Vec<ClipId>) {
        self.selection = ids;
    }

    pub fn clear_selection(&mut self) {
        self.selection.clear();
        self.clear_point_selection();
    }

    /// The selected points of an automation block, if any are.
    pub fn point_selection(&self) -> &[PointId] {
        &self.point_selection
    }

    /// Which block those points belong to.
    pub fn point_clip(&self) -> Option<ClipId> {
        self.point_clip
    }

    fn clear_point_selection(&mut self) {
        self.point_selection.clear();
        self.point_clip = None;
    }

    /// The points a press on a curve just made, as the host reports them —
    /// they become the point selection, and the drag in progress carries
    /// them. The same handshake `PianoRoll::note_added` has.
    pub fn points_inserted(&mut self, clip: ClipId, ids: Vec<PointId>) {
        if ids.is_empty() {
            return;
        }
        self.point_clip = Some(clip);
        self.point_selection = ids;
        if let Gesture::MovingPoints {
            clip: gesture_clip,
            pending,
            ..
        } = &mut self.gesture
            && *gesture_clip == clip
        {
            *pending = false;
        }
    }

    /// The point the right button asked about, once.
    pub fn take_point_menu(&mut self) -> Option<(ClipId, PointId)> {
        self.point_menu.take()
    }

    /// Gives the selected points a shape.
    pub fn set_point_curve(&mut self, curve: fontelle_model::CurveShape) -> Vec<ArrangeEdit> {
        match self.point_clip {
            Some(clip) if !self.point_selection.is_empty() => vec![ArrangeEdit::SetPointCurve {
                clip,
                ids: self.point_selection.clone(),
                curve,
            }],
            _ => Vec::new(),
        }
    }

    /// What `Delete` does while points are selected: takes them off their
    /// curve, and leaves the clip alone.
    pub fn delete_points(&mut self) -> Vec<ArrangeEdit> {
        let Some(clip) = self.point_clip else {
            return Vec::new();
        };
        if self.point_selection.is_empty() {
            return Vec::new();
        }
        let ids = std::mem::take(&mut self.point_selection);
        self.point_clip = None;
        vec![ArrangeEdit::RemovePoints { clip, ids }]
    }

    /// One step of an erase: whatever `hit` found, gone. Shared by the press
    /// and the drag so the rule is written once.
    fn erase_at(&mut self, hit: TimelineHit) -> Vec<ArrangeEdit> {
        match hit {
            TimelineHit::Clip(id, _) => {
                self.selection.retain(|s| *s != id);
                vec![ArrangeEdit::Remove(vec![id])]
            }
            _ => Vec::new(),
        }
    }

    /// The clip the arrangement wants opened in the roll, once.
    pub fn take_open(&mut self) -> Option<ClipId> {
        self.open.take()
    }

    /// The selection box being dragged, for drawing.
    pub fn marquee(&self) -> Option<Rect> {
        match self.gesture {
            Gesture::Marquee { from, to } => Some(box_between(from, to)),
            _ => None,
        }
    }

    /// The cut tool's line, while it is being drawn, so the canvas can show
    /// where the blade is going. A stroke you cannot see is one you aim twice.
    /// Whether the gesture in progress paints something the document does not
    /// know about — the arrangement's half of
    /// [`PianoRoll::draws_overlay`](crate::canvas::PianoRoll::draws_overlay),
    /// and it had the same invisible-cut-tool bug for the same reason.
    pub fn draws_overlay(&self) -> bool {
        matches!(
            self.gesture,
            Gesture::Marquee { .. } | Gesture::Slicing { .. }
        )
    }

    pub fn slice_line(&self) -> Option<((f32, f32), (f32, f32))> {
        match self.gesture {
            Gesture::Slicing { from, to } => Some((from, to)),
            _ => None,
        }
    }

    pub fn press(
        &mut self,
        button: MouseButton,
        x: f32,
        y: f32,
        layout: &TimelineLayout,
        clips: &[ClipInfo],
        beats_per_bar: u32,
    ) -> Vec<ArrangeEdit> {
        let hit = timeline_hit(&self.view, layout, clips, x, y);

        // Right-click deletes, as it does in the roll — and keeps deleting
        // while the button is down. Note what it deliberately does *not* do:
        // set `self.open`. A clip being rubbed out is not a clip being opened
        // for editing, and a sweep across a row would otherwise leave the
        // piano roll showing the last thing it destroyed.
        //
        // Inside an automation block's curve the right button is a question,
        // not an eraser: on a point it asks for that point's menu, and on the
        // bare curve it does nothing at all — the pointer being two pixels
        // off a point must not cost the whole clip.
        if button == MouseButton::Right {
            match hit {
                TimelineHit::Clip(clip, ClipPart::Point(id)) => {
                    self.point_clip = Some(clip);
                    if !self.point_selection.contains(&id) {
                        self.point_selection = vec![id];
                    }
                    self.point_menu = Some((clip, id));
                    self.gesture = Gesture::None;
                    return Vec::new();
                }
                TimelineHit::Clip(_, ClipPart::Curve { .. }) => {
                    self.gesture = Gesture::None;
                    return Vec::new();
                }
                _ => {}
            }
            self.gesture = Gesture::Erasing;
            return self.erase_at(hit);
        }

        // The cut tool draws a line, and it draws it over **anything** — the
        // same rule the roll's has, and the same reason: it is the one tool
        // whose press does not care what is under it.
        if self.tool == TimelineTool::Slice {
            self.gesture = Gesture::Slicing {
                from: (x, y),
                to: (x, y),
            };
            return Vec::new();
        }

        match hit {
            // A point of an automation block: picked up, to be carried.
            TimelineHit::Clip(id, ClipPart::Point(point)) => {
                self.selection = vec![id];
                self.open = Some(id);
                if self.point_clip != Some(id) || !self.point_selection.contains(&point) {
                    self.point_clip = Some(id);
                    self.point_selection = vec![point];
                }
                let Some(clip) = clips.iter().find(|c| c.id == id) else {
                    self.gesture = Gesture::None;
                    return Vec::new();
                };
                let block = clip_rect(&self.view, layout.grid, clip);
                let area = automation_block(block, clip).area;
                self.gesture = Gesture::MovingPoints {
                    clip: id,
                    origin_tick: block_tick_at(area, clip.length, x),
                    origin_value: block_value_at(area, y),
                    applied_tick: 0,
                    applied_value: 0.0,
                    limits: PointLimits::of(clip, &self.point_selection),
                    pending: false,
                };
                Vec::new()
            }
            // The bare curve: a point is made where you pressed, on the grid,
            // and the drag that follows carries it — the same handshake
            // drawing a note has.
            TimelineHit::Clip(id, ClipPart::Curve { tick, value }) => {
                self.selection = vec![id];
                self.open = Some(id);
                self.point_clip = Some(id);
                self.point_selection.clear();
                let Some(clip) = clips.iter().find(|c| c.id == id) else {
                    self.gesture = Gesture::None;
                    return Vec::new();
                };
                let snapped = timeline_snap(&self.view, tick, beats_per_bar).clamp(0, clip.length);
                let block = clip_rect(&self.view, layout.grid, clip);
                let area = automation_block(block, clip).area;
                self.gesture = Gesture::MovingPoints {
                    clip: id,
                    origin_tick: block_tick_at(area, clip.length, x),
                    origin_value: block_value_at(area, y),
                    applied_tick: 0,
                    applied_value: 0.0,
                    limits: PointLimits::at(clip, snapped, value),
                    pending: true,
                };
                vec![ArrangeEdit::AddPoint {
                    clip: id,
                    tick: snapped,
                    value,
                }]
            }
            // A fade handle or node: the fade is what moves, and nothing
            // else — the clip is chosen, not picked up and not opened.
            TimelineHit::Clip(id, ClipPart::FadeHandle(end)) => {
                self.selection = vec![id];
                self.stamp = Some(id);
                self.clear_point_selection();
                self.gesture = Gesture::Fading {
                    clip: id,
                    end,
                    applied: None,
                };
                Vec::new()
            }
            TimelineHit::Clip(id, ClipPart::FadeNode(end)) => {
                self.selection = vec![id];
                self.stamp = Some(id);
                self.clear_point_selection();
                self.gesture = Gesture::Bending {
                    clip: id,
                    end,
                    applied: None,
                };
                Vec::new()
            }
            TimelineHit::Clip(id, part) => {
                if !self.selection.contains(&id) {
                    self.selection = vec![id];
                }
                // Chosen, so it is the thing in hand from here on.
                self.stamp = Some(id);
                // A block picked up is a block, not its points: whatever
                // points were selected — on it or on another — let go.
                self.clear_point_selection();
                // Clicking a clip opens it: the roll and the arrangement are
                // two views of the same piece, and having to find the channel
                // in the rack to edit the clip you just pointed at is the
                // difference between two panels and one workflow.
                self.open = Some(id);
                self.origin = (
                    timeline_x_to_tick(&self.view, layout.grid, x),
                    y_to_lane(&self.view, layout.grid, y),
                );
                self.gesture = match part {
                    ClipPart::RightEdge => Gesture::Resizing {
                        applied_tick: 0,
                        shortest: self.shortest_selected(clips),
                        // Shift on the edge grip means **loop**, and it is
                        // decided here rather than read live: a modifier let
                        // go halfway through a drag must not turn a loop back
                        // into a stretch under the pointer.
                        looping: self.modifiers.shift,
                        period: self.loop_period(clips),
                        stretch: self.stretch,
                    },
                    ClipPart::Body
                    | ClipPart::Point(_)
                    | ClipPart::Curve { .. }
                    | ClipPart::FadeHandle(_)
                    | ClipPart::FadeNode(_) => Gesture::Moving {
                        applied_tick: 0,
                        applied_lane: 0,
                        limits: MoveLimits::of(&self.selection, clips),
                    },
                };
                Vec::new()
            }
            TimelineHit::Empty { tick, lane } => {
                self.selection.clear();
                self.clear_point_selection();
                // Ctrl is the marquee whichever tool is on — the same
                // modifier the roll uses for the same thing, so selecting a
                // few clips does not cost two trips to the toolbar.
                if self.tool == TimelineTool::Select || self.modifiers.ctrl {
                    self.gesture = Gesture::Marquee {
                        from: (x, y),
                        to: (x, y),
                    };
                    return Vec::new();
                }
                self.gesture = Gesture::None;
                // **A plain press makes nothing.** *"single clicking in the
                // arrangement no longer makes anything ... that way single
                // click is freed up so it can be used freely for deselecting
                // things without a hassle."* It has already let go of the
                // selection above, and that is the whole of what it does. A
                // blank clip is a double-click (`double_press`); a copy is
                // the Shift+press below.
                if !self.modifiers.shift {
                    return Vec::new();
                }
                // On the grid, not where the pointer was: a clip half a beat
                // off the bar is one somebody has to nudge before they can use
                // it, and the snap is right there in the toolbar saying what
                // it should have been.
                let start = timeline_snap(&self.view, tick, beats_per_bar).max(0);
                // *"to create a copy of your last selected item its shift +
                // click."* A copy of the clip in hand, when there is one and
                // it is still there. Nothing in hand is nothing to copy, and
                // a press that drew a blank clip instead would be a copy of
                // something you never picked — the double-click is the way to
                // a blank one.
                let Some(source) = self.stamp.filter(|id| clips.iter().any(|c| c.id == *id)) else {
                    return Vec::new();
                };
                self.stamp_pending = true;
                vec![ArrangeEdit::Stamp {
                    source,
                    lane,
                    start,
                }]
            }
            _ => {
                self.gesture = Gesture::None;
                let _ = beats_per_bar;
                Vec::new()
            }
        }
    }

    /// The second press of a double-click.
    ///
    /// *"we leave creating a new empty clip as double click."* A double-click
    /// on empty grid asks for a blank clip there. Its first press has already
    /// gone through [`press`](Self::press), which on empty grid made nothing
    /// — so there is nothing to take back, and one gesture leaves exactly one
    /// clip behind.
    ///
    /// With Shift held the first press *did* make something — a copy of the
    /// clip in hand — and the second asks for nothing, or two quick
    /// Shift+clicks would put a blank clip on top of the copy.
    ///
    /// Anywhere else — on some other block — it asks for nothing: opening a
    /// block on a double-click is the window's business, and a gesture that
    /// deleted the block under it would be the worst kind of surprise.
    pub fn double_press(
        &mut self,
        button: MouseButton,
        x: f32,
        y: f32,
        layout: &TimelineLayout,
        clips: &[ClipInfo],
        beats_per_bar: u32,
    ) -> Vec<ArrangeEdit> {
        self.gesture = Gesture::None;
        if button != MouseButton::Left || self.modifiers.shift {
            return Vec::new();
        }
        // The select tool and Ctrl are the marquee, on the second press as on
        // the first: a box being drawn is not a request for a clip.
        if self.tool == TimelineTool::Select || self.modifiers.ctrl {
            return Vec::new();
        }
        match timeline_hit(&self.view, layout, clips, x, y) {
            TimelineHit::Empty { tick, lane } => {
                let start = timeline_snap(&self.view, tick, beats_per_bar).max(0);
                vec![ArrangeEdit::Add { lane, start }]
            }
            _ => Vec::new(),
        }
    }

    pub fn drag(
        &mut self,
        x: f32,
        y: f32,
        layout: &TimelineLayout,
        // Read by exactly one gesture, and it is worth saying which. A
        // *move* or *resize* must not look here: its limits are the ones its
        // press measured (see `MoveLimits`), and reaching for the live clips
        // — which the drag is itself changing — is the bug this canvas was
        // fixed for. An **erase** is the opposite case and the reason the
        // parameter was kept: it holds no state, so the live clips are the
        // only thing that can tell it what is still under the pointer.
        clips: &[ClipInfo],
        beats_per_bar: u32,
    ) -> Vec<ArrangeEdit> {
        let grid = layout.grid;
        // Clamped for the reason the roll clamps: a drag that strays off the
        // canvas means the nearest cell inside it, not a teleport.
        let (cx, cy) = clamp_to_grid(grid, x, y);
        let tick = timeline_x_to_tick(&self.view, grid, cx);
        let lane = y_to_lane(&self.view, grid, cy);

        match self.gesture.clone() {
            Gesture::None => Vec::new(),

            // The raw pointer, not the clamped one: an erase dragged off the
            // grid must find nothing rather than rubbing out whatever sits at
            // the edge it slid past. Same rule as the roll's.
            Gesture::Erasing => self.erase_at(timeline_hit(&self.view, layout, clips, x, y)),

            Gesture::Marquee { from, .. } => {
                self.gesture = Gesture::Marquee { from, to: (x, y) };
                Vec::new()
            }

            Gesture::Slicing { from, .. } => {
                self.gesture = Gesture::Slicing { from, to: (x, y) };
                Vec::new()
            }

            // The raw pointer, not the clamped one: a handle dragged past
            // the block's end means the whole block, and one dragged back
            // past the corner means no fade. The block is read live and
            // that is fine — a fade drag does not move the block.
            Gesture::Fading {
                clip: id,
                end,
                applied,
            } => {
                let Some(clip) = clips.iter().find(|c| c.id == id) else {
                    return Vec::new();
                };
                let block = clip_rect(&self.view, grid, clip);
                if block.width <= 0.0 {
                    return Vec::new();
                }
                // A fraction of the **file**, which is what the host turns
                // into frames — measured the way `fade_anatomy` places the
                // handles, so the handle lands under the pointer.
                let (file_px, file_end) = content_span(block, clip);
                let raw = match end {
                    FadeEnd::In => (x - block.x) / file_px,
                    FadeEnd::Out => (file_end - x) / file_px,
                };
                let fraction = raw.clamp(0.0, 1.0);
                if applied == Some(fraction) {
                    return Vec::new();
                }
                self.gesture = Gesture::Fading {
                    clip: id,
                    end,
                    applied: Some(fraction),
                };
                vec![ArrangeEdit::SetFade {
                    clip: id,
                    end,
                    fraction,
                }]
            }

            // The node's height is the gain wanted at the fade's midpoint,
            // and the tension is whatever puts the curve through it — so the
            // node lands under the pointer rather than near it.
            Gesture::Bending {
                clip: id,
                end,
                applied,
            } => {
                let Some(clip) = clips.iter().find(|c| c.id == id) else {
                    return Vec::new();
                };
                let (_, content) = clip_bands(clip_rect(&self.view, grid, clip));
                if content.height <= 0.0 {
                    return Vec::new();
                }
                let gain = ((content.bottom() - y) / content.height).clamp(0.0, 1.0);
                let tension = fontelle_types::tension_for_midpoint(gain);
                if applied == Some(tension) {
                    return Vec::new();
                }
                self.gesture = Gesture::Bending {
                    clip: id,
                    end,
                    applied: Some(tension),
                };
                vec![ArrangeEdit::SetFadeTension {
                    clip: id,
                    end,
                    tension,
                }]
            }

            Gesture::MovingPoints {
                clip: id,
                origin_tick,
                origin_value,
                applied_tick,
                applied_value,
                limits,
                pending,
            } => {
                if pending || self.point_selection.is_empty() {
                    return Vec::new();
                }
                // The block's geometry is read live and that is fine: a point
                // drag does not move the block. The *points* are not read —
                // their positions are what the drag is changing, and the
                // deltas below are measured against what this gesture has
                // already asked for. See `MoveLimits`.
                let Some(clip) = clips.iter().find(|c| c.id == id) else {
                    return Vec::new();
                };
                let block = clip_rect(&self.view, grid, clip);
                let area = automation_block(block, clip).area;
                let unit = snap_unit(self.view.snap, beats_per_bar);
                let raw = block_tick_at(area, clip.length, cx) - origin_tick;
                let wanted_tick = if unit > 0 {
                    (raw as f64 / unit as f64).round() as Tick * unit
                } else {
                    raw
                }
                .clamp(limits.min_tick, limits.max_tick.max(limits.min_tick));
                let wanted_value = (block_value_at(area, cy) - origin_value)
                    .clamp(limits.min_value, limits.max_value.max(limits.min_value));
                let d_tick = wanted_tick - applied_tick;
                let d_value = wanted_value - applied_value;
                if d_tick == 0 && d_value.abs() < 1e-9 {
                    return Vec::new();
                }
                self.gesture = Gesture::MovingPoints {
                    clip: id,
                    origin_tick,
                    origin_value,
                    applied_tick: wanted_tick,
                    applied_value: wanted_value,
                    limits,
                    pending,
                };
                vec![ArrangeEdit::MovePoints {
                    clip: id,
                    ids: self.point_selection.clone(),
                    tick_delta: d_tick,
                    value_delta: d_value,
                }]
            }

            Gesture::Moving {
                applied_tick,
                applied_lane,
                limits,
            } => {
                if self.selection.is_empty() {
                    return Vec::new();
                }
                let unit = snap_unit(self.view.snap, beats_per_bar);
                let raw = tick - self.origin.0;
                let wanted_tick = if unit > 0 {
                    (raw as f64 / unit as f64).round() as Tick * unit
                } else {
                    raw
                };
                // Never before the start of the song, and never above the
                // first lane — against the limits this gesture *started* with.
                let wanted_tick = wanted_tick.max(limits.min_tick);
                let wanted_lane = (lane as i32 - self.origin.1 as i32).max(limits.min_lane);

                let d_tick = wanted_tick - applied_tick;
                let d_lane = wanted_lane - applied_lane;
                if d_tick == 0 && d_lane == 0 {
                    // No zero-delta edits: a sub-bar mouse move is not a
                    // history entry and not a timeline recompile.
                    return Vec::new();
                }
                self.gesture = Gesture::Moving {
                    applied_tick: wanted_tick,
                    applied_lane: wanted_lane,
                    limits,
                };
                vec![ArrangeEdit::Move {
                    ids: self.selection.clone(),
                    tick_delta: d_tick,
                    lane_delta: d_lane,
                }]
            }

            Gesture::Resizing {
                applied_tick,
                shortest,
                looping,
                period,
                stretch,
            } => {
                if self.selection.is_empty() {
                    return Vec::new();
                }
                let unit = snap_unit(self.view.snap, beats_per_bar);
                let raw = tick - self.origin.0;
                let wanted = if unit > 0 {
                    (raw as f64 / unit as f64).round() as Tick * unit
                } else {
                    raw
                };
                // Never past nothing: the shortest a clip may become is one
                // snap unit, measured against the length it had when the drag
                // started.
                let wanted = wanted.max(-(shortest - unit.max(1)).max(0));
                let delta = wanted - applied_tick;
                if delta == 0 {
                    return Vec::new();
                }
                let first = applied_tick == 0;
                self.gesture = Gesture::Resizing {
                    applied_tick: wanted,
                    shortest,
                    looping,
                    period,
                    stretch,
                };
                let mut edits = Vec::new();
                // **A loop dragged back to its period stops being one.**
                // Reported from using the window: *"i cant figure out (if
                // there even is a way) how to turn it back into just a normal
                // clip i can extend the length of ... if i drag it back to the
                // original length it is no longer a looping clip."*
                //
                // The gesture is the one that made it, without Shift, and the
                // rule is the arithmetic of what a loop is: a block no longer
                // than one pass has nothing to repeat, so calling it a loop is
                // a state you can be in and cannot see. Undoing it here is
                // what makes the edge grip a toggle rather than a trapdoor.
                //
                // Not while Shift is held: that drag is *asking* for a loop,
                // and one that unlooped on its way past its own period could
                // never make a short one.
                if !looping {
                    let ids: Vec<ClipId> = self
                        .selected(clips)
                        .filter(|clip| {
                            clip.loop_length
                                .is_some_and(|period| period > 0 && clip.length + delta <= period)
                        })
                        .map(|clip| clip.id)
                        .collect();
                    if !ids.is_empty() {
                        edits.push(ArrangeEdit::SetLoop {
                            ids,
                            loop_length: None,
                        });
                    }
                }
                // The mode first, once, and **only ever turning stretching
                // on**: what a longer block means has to be settled before the
                // block gets longer, and a clip already stretching has nothing
                // to be told. Note clips are never named — their edge has only
                // ever cut, and the switch is not about them.
                //
                // **A drag does not turn stretching off.** It used to, whenever
                // the switch was off, and that is the fault behind both reports
                // of a take "going blank" after being shortened and grown back:
                // turning a stretched clip off *freezes* the rate it was being
                // played at into the clip's own speed (`with_stretch`, and for
                // a good reason — the sound must not jump). So a clip stretched
                // down to a quarter and then dragged became a clip genuinely
                // playing four times too fast, whose take really was a quarter
                // as long, and no drag could bring the rest back.
                //
                // A trim must never be lossy. Turning stretching off is a
                // deliberate act with a switch for it, and doing it
                // deliberately still freezes; a drag only ever turns it on.
                if first && stretch {
                    let ids: Vec<ClipId> = self
                        .selected(clips)
                        .filter(|clip| clip.kind == ClipKind::Audio && !clip.audio.stretched)
                        .map(|clip| clip.id)
                        .collect();
                    if !ids.is_empty() {
                        edits.push(ArrangeEdit::SetStretch {
                            ids,
                            stretch: ClipStretch::Resample,
                        });
                    }
                }
                // The loop is set once, on the first step of the drag, and the
                // clip then simply grows. Setting it every step would work and
                // would also mean a history entry per pixel that says nothing
                // new; `SetClipLoop::merge_with` would swallow them, but a
                // command that is only ever a no-op is a command not to send.
                if looping && first && period > 0 {
                    edits.push(ArrangeEdit::SetLoop {
                        ids: self.selection.clone(),
                        loop_length: Some(period),
                    });
                }
                edits.push(ArrangeEdit::Resize {
                    ids: self.selection.clone(),
                    tick_delta: delta,
                });
                edits
            }
        }
    }

    pub fn release(&mut self) {
        self.gesture = Gesture::None;
    }

    /// [`release`](Self::release), knowing where the button came up — which a
    /// marquee needs, because that is when it decides what it caught.
    pub fn release_over(
        &mut self,
        x: f32,
        y: f32,
        layout: &TimelineLayout,
        clips: &[ClipInfo],
        beats_per_bar: u32,
    ) -> Vec<ArrangeEdit> {
        let mut edits = Vec::new();
        match self.gesture {
            Gesture::Marquee { from, .. } => {
                let box_ = box_between(from, (x, y));
                self.selection = clips
                    .iter()
                    .filter(|clip| clip_rect(&self.view, layout.grid, clip).intersects(&taut(box_)))
                    .map(|clip| clip.id)
                    .collect();
                // A box around one clip chose it as surely as a click did.
                if let Some(last) = self.selection.last() {
                    self.stamp = Some(*last);
                }
            }
            // The cut tool's whole edit lands here: a line half-drawn is not a
            // cut, and one gesture is one history entry.
            Gesture::Slicing { from, .. } => {
                let cuts = clip_cuts(
                    &self.view,
                    layout.grid,
                    clips,
                    from,
                    (x, y),
                    self.view.snap,
                    beats_per_bar,
                );
                if !cuts.is_empty() {
                    edits.push(ArrangeEdit::Split { cuts });
                }
            }
            _ => {}
        }
        self.gesture = Gesture::None;
        edits
    }

    /// The shortest selected clip, which is what a resize is floored against.
    fn shortest_selected(&self, clips: &[ClipInfo]) -> Tick {
        self.selected(clips).map(|c| c.length).min().unwrap_or(1)
    }

    /// The period a Shift-drag would set: the one the selection **already**
    /// loops at, or the length it has.
    ///
    /// Already-looping wins, which is the whole of the rule people get wrong:
    /// dragging a one-bar loop out to eight bars must not make it an eight-bar
    /// loop. The period is the content, and stretching the window does not
    /// change the content.
    fn loop_period(&self, clips: &[ClipInfo]) -> Tick {
        self.selected(clips)
            .map(|clip| clip.loop_length.unwrap_or(clip.length))
            .min()
            .unwrap_or(0)
    }

    /// What is held down, from the window.
    pub fn set_modifiers(&mut self, modifiers: Modifiers) {
        self.modifiers = modifiers;
    }

    pub fn tool(&self) -> TimelineTool {
        self.tool
    }

    pub fn set_tool(&mut self, tool: TimelineTool) {
        self.tool = tool;
    }

    /// Whether an edge drag on an audio clip fits the file to the block —
    /// see [`TimelineControl::Stretch`].
    pub fn stretch(&self) -> bool {
        self.stretch
    }

    pub fn set_stretch(&mut self, stretch: bool) {
        self.stretch = stretch;
    }

    /// Flips the switch, and says what that means for the clips selected now.
    ///
    /// **Turning it off is the deliberate act that freezes a stretch.** A drag
    /// only ever turns stretching *on* (see `press`'s edge arm), because a
    /// drag that turned it off was silently making trims lossy. So this is the
    /// one place a stretched clip comes back down, and it does it the way it
    /// always did — through `with_stretch`, which keeps the sound where it is
    /// by writing the rate the clip was being played at into its own speed.
    ///
    /// Turning it **on** changes nothing by itself: what a longer block means
    /// is settled when the block is actually dragged, and a clip switched to
    /// stretching without being dragged would change length for no gesture.
    pub fn toggle_stretch(&mut self, clips: &[ClipInfo]) -> Vec<ArrangeEdit> {
        self.stretch = !self.stretch;
        if self.stretch {
            return Vec::new();
        }
        let ids: Vec<ClipId> = self
            .selected(clips)
            .filter(|clip| clip.kind == ClipKind::Audio && clip.audio.stretched)
            .map(|clip| clip.id)
            .collect();
        if ids.is_empty() {
            return Vec::new();
        }
        vec![ArrangeEdit::SetStretch {
            ids,
            stretch: ClipStretch::Off,
        }]
    }

    // ---------------------------------------------------- from the keyboard ---

    /// Moves the selection, the way a drag does but by a known amount.
    ///
    /// Clamped exactly as the drag is, and empty when there is nothing to move
    /// or nowhere left to move it — an arrow key at the end of its travel is
    /// not an undo entry. The roll's [`PianoRoll::nudge`] in every respect
    /// except that clips have lanes where notes have keys.
    ///
    /// [`PianoRoll::nudge`]: crate::canvas::PianoRoll::nudge
    pub fn nudge(
        &mut self,
        clips: &[ClipInfo],
        tick_delta: Tick,
        lane_delta: i32,
    ) -> Vec<ArrangeEdit> {
        if self.selection.is_empty() {
            return Vec::new();
        }
        let limits = MoveLimits::of(&self.selection, clips);
        let tick_delta = tick_delta.max(limits.min_tick);
        let lane_delta = lane_delta.max(limits.min_lane);
        if tick_delta == 0 && lane_delta == 0 {
            return Vec::new();
        }
        vec![ArrangeEdit::Move {
            ids: self.selection.clone(),
            tick_delta,
            lane_delta,
        }]
    }

    /// Lengthens or shortens the selection, floored at its shortest clip.
    pub fn resize_selection(&mut self, clips: &[ClipInfo], tick_delta: Tick) -> Vec<ArrangeEdit> {
        if self.selection.is_empty() {
            return Vec::new();
        }
        let tick_delta = tick_delta.max(-(self.shortest_selected(clips) - 1).max(0));
        if tick_delta == 0 {
            return Vec::new();
        }
        vec![ArrangeEdit::Resize {
            ids: self.selection.clone(),
            tick_delta,
        }]
    }

    /// What `Delete` should do.
    pub fn delete_selection(&mut self) -> Vec<ArrangeEdit> {
        if self.selection.is_empty() {
            // Not an empty removal — that would still be a history entry, and
            // an undo that puts nothing back is worse than a key that did
            // nothing.
            return Vec::new();
        }
        vec![ArrangeEdit::Remove(std::mem::take(&mut self.selection))]
    }

    /// `Ctrl+B`: the selection again, starting where it ends.
    ///
    /// Rounded up to the next bar, so duplicating a phrase produces a phrase
    /// twice as long rather than an overlap nobody asked for — the same rule
    /// the roll's duplicate follows.
    pub fn duplicate(&mut self, clips: &[ClipInfo], beats_per_bar: u32) -> Vec<ArrangeEdit> {
        if self.selection.is_empty() {
            return Vec::new();
        }
        let earliest = self.selected(clips).map(|c| c.start).min().unwrap_or(0);
        let end = self
            .selected(clips)
            .map(|c| c.start + c.length)
            .max()
            .unwrap_or(earliest);
        let bar = PPQN * Tick::from(beats_per_bar.max(1));
        let landing = (end + bar - 1) / bar * bar;
        let offset = (landing - earliest).max(bar);
        vec![ArrangeEdit::Duplicate {
            ids: self.selection.clone(),
            tick_offset: offset,
        }]
    }

    /// Steps the arrangement's snap to the next division.
    ///
    /// Its own, not the roll's: the two are different grids and a person sets
    /// them separately — bars on the arrangement while writing sixteenths in
    /// the roll is the ordinary case, not an edge one.
    pub fn cycle_snap(&mut self) {
        self.view.snap = self.view.snap.next();
    }

    /// The clips a duplicate or a paste just created, as the host reports
    /// them — they become the selection.
    ///
    /// The same handshake `PianoRoll::notes_inserted` has, and for a sharper
    /// reason: a duplicate's offset is measured from the selection, so leaving
    /// the selection on the *original* makes pressing the key twice put two
    /// copies at the same offset, on top of each other. Selecting the copy is
    /// what turns "duplicate" into "repeat".
    ///
    /// An empty list leaves the selection alone: a duplicate that created
    /// nothing must not clear it.
    pub fn clips_inserted(&mut self, ids: Vec<ClipId>) {
        if ids.is_empty() {
            return;
        }
        // The copy a Shift+press just stamped is the thing in hand now —
        // which is what makes a row of Shift+presses a row of the same clip.
        if std::mem::take(&mut self.stamp_pending)
            && let Some(copy) = ids.first()
        {
            self.stamp = Some(*copy);
        } else if let Some(last) = ids.last() {
            self.stamp = Some(*last);
        }
        self.selection = ids;
    }

    /// `Ctrl+C`: keep the selection. See [`ArrangeEdit::Copy`] for why the
    /// clips go to the host rather than into this canvas.
    pub fn copy(&mut self) -> Vec<ArrangeEdit> {
        if self.selection.is_empty() {
            return Vec::new();
        }
        vec![ArrangeEdit::Copy(self.selection.clone())]
    }

    /// `Ctrl+X`: keep it, then take it away.
    ///
    /// In that order, and the order is the whole of it — removing first copies
    /// a clip that is already gone.
    pub fn cut(&mut self) -> Vec<ArrangeEdit> {
        if self.selection.is_empty() {
            return Vec::new();
        }
        let ids = std::mem::take(&mut self.selection);
        vec![ArrangeEdit::Copy(ids.clone()), ArrangeEdit::Remove(ids)]
    }

    /// `Ctrl+V`: the clipboard down at `at`, **snapped**.
    ///
    /// Snapped here for the reason `PianoRoll::paste` snaps there: `at` comes
    /// from the playhead or from the pointer, and neither of those is ever on
    /// a bar line by itself.
    pub fn paste(&mut self, at: Tick, beats_per_bar: u32) -> Vec<ArrangeEdit> {
        let at = timeline_snap(&self.view, at.max(0), beats_per_bar).max(0);
        vec![ArrangeEdit::Paste { at }]
    }

    /// The selection again, `times` more, each copy starting where the last
    /// one ends.
    ///
    /// Reported as *"I made this drum loop but I can't repeat it"*. One edit
    /// per copy rather than one edit meaning "n copies", because each copy's
    /// offset is different and a single edit carrying a count would have to
    /// re-derive them somewhere that cannot see the clips.
    pub fn repeat(
        &mut self,
        clips: &[ClipInfo],
        times: usize,
        beats_per_bar: u32,
    ) -> Vec<ArrangeEdit> {
        if self.selection.is_empty() || times == 0 {
            return Vec::new();
        }
        let earliest = self.selected(clips).map(|c| c.start).min().unwrap_or(0);
        let end = self
            .selected(clips)
            .map(|c| c.start + c.length)
            .max()
            .unwrap_or(earliest);
        // The span the selection occupies, rounded up to a bar so a loop that
        // is not quite a whole number of bars still repeats on the beat.
        let bar = PPQN * Tick::from(beats_per_bar.max(1));
        let span = (end - earliest).max(1);
        let stride = (span + bar - 1) / bar * bar;
        (1..=times as Tick)
            .map(|n| ArrangeEdit::Duplicate {
                ids: self.selection.clone(),
                tick_offset: stride * n,
            })
            .collect()
    }

    /// Mutes the selection, or unmutes it when all of it is already muted —
    /// one key for both, which is what a person means by "mute this".
    pub fn toggle_mute(&mut self, clips: &[ClipInfo]) -> Vec<ArrangeEdit> {
        if self.selection.is_empty() {
            return Vec::new();
        }
        let all_muted = self.selected(clips).all(|c| c.muted);
        vec![ArrangeEdit::SetMuted {
            ids: self.selection.clone(),
            muted: !all_muted,
        }]
    }

    fn selected<'a>(&'a self, clips: &'a [ClipInfo]) -> impl Iterator<Item = &'a ClipInfo> + 'a {
        clips.iter().filter(|c| self.selection.contains(&c.id))
    }
}

/// The rectangle two corners describe, whichever way round they came.
fn box_between(a: (f32, f32), b: (f32, f32)) -> Rect {
    Rect::new(
        a.0.min(b.0),
        a.1.min(b.1),
        (a.0 - b.0).abs(),
        (a.1 - b.1).abs(),
    )
}

/// A selection box with no zero dimension: a box dragged along one axis is a
/// line, and a line through a row of clips is an ordinary way to select them.
fn taut(box_: Rect) -> Rect {
    Rect::new(box_.x, box_.y, box_.width.max(1.0), box_.height.max(1.0))
}

/// Snaps a tick to the arrangement's grid — re-exported here so callers that
/// have a `TimelineView` do not have to reach into the roll for it.
pub fn timeline_snap(view: &TimelineView, tick: Tick, beats_per_bar: u32) -> Tick {
    snap_tick(tick, view.snap, beats_per_bar)
}

// -------------------------------------------------------- loop seams ---

/// How few pixels a repeat may be before the seams stop being drawn.
///
/// At one pixel per repeat a mark per seam is a filled rectangle, which reads
/// as a solid block — the exact opposite of what the marks are for. Below this
/// the clip is drawn plain and the loop is a fact you find by zooming in.
const MIN_REPEAT_PX: f32 = 6.0;

/// Where the seams between a looped clip's repeats fall, in screen x.
///
/// **Between** them, not around them: a line at the start and the end is a
/// border, and a block with a border is exactly the copy-and-paste picture
/// looping is meant to stop looking like.
///
/// Empty for a clip that does not loop, and for one zoomed out past legibility.
pub fn loop_marks(view: &TimelineView, grid: Rect, clip: &ClipInfo) -> Vec<f32> {
    let Some(period) = clip.loop_length.filter(|p| *p > 0) else {
        return Vec::new();
    };
    let step = period as f32 * view.pixels_per_tick;
    if step < MIN_REPEAT_PX {
        return Vec::new();
    }
    let block = clip_rect(view, grid, clip);
    if block.is_empty() {
        return Vec::new();
    }

    let mut marks = Vec::new();
    let mut tick = period;
    while tick < clip.length {
        let x = block.x + tick as f32 * view.pixels_per_tick;
        // Clipped to the block rather than to the grid: a seam scrolled off
        // the left is not drawn, and the renderer clips the rest.
        if x > block.x && x < block.right() {
            marks.push(x);
        }
        tick += period;
    }
    marks
}

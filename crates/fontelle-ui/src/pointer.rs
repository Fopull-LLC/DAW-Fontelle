//! What the mouse cursor says about what a click would do.
//!
//! Reported from using the window: *"my mouse cursor should change to reflect
//! the action I can take."* It never changed, so the only way to find out
//! whether you were about to move a note or resize it was to try — which, on a
//! canvas where those two are eight pixels apart, is the difference between a
//! tool and a guessing game.
//!
//! **A pure function of the geometry the window already has.** §2.5's rule
//! again: nothing here needs a window, so all of it is tested without one.

use fontelle_model::{Arena, Note};
use fontelle_types::NoteId;

use crate::canvas::{
    BrowserHit, BrowserLayout, ClipPart, InstrumentLayout, InstrumentView, NotePart, RackHit,
    RackLayout, RollHit, RollLayout, RollView, TimelineHit, TimelineLayout, TimelineToolbar,
    TimelineView, Tool, ToolbarLayout, browser_hit, hit_test, instrument_hit, rack_hit,
    timeline_hit, timeline_toolbar_hit, toolbar_hit,
};
use crate::document::ClipInfo;
use crate::layout::{EditorTab, EditorTabs, WindowLayout, editor_tab_at};
use crate::transport::TransportBarLayout;

/// What the pointer should look like.
///
/// Our own vocabulary rather than the windowing system's, so the decision is
/// testable and the backend mapping is one match in the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Pointer {
    /// Nothing here does anything.
    #[default]
    Default,
    /// Something clickable that is not dragged: a button, a tab, a list row.
    Hand,
    /// A text field.
    Text,
    /// A vertical seam that moves left and right — a note's or a clip's end.
    ResizeX,
    /// A horizontal seam that moves up and down — the arrangement's divider,
    /// the property lane's, and a knob, which is dragged vertically.
    ResizeY,
    /// Something that can be picked up.
    Grab,
    /// Something that has been.
    Grabbing,
    /// Empty canvas under the draw tool: this click writes a note.
    ///
    /// Drawn as an actual pencil (see [`crate::icon`]), because "the
    /// crosshair" is what every canvas in every program shows and says
    /// nothing about *which* tool is live — which is the report this answers.
    Draw,
    /// The paint tool: this drag writes a note per cell it crosses.
    Paint,
    /// The delete tool, or the right button held down.
    Erase,
    /// Over a marquee's empty canvas: this drag selects.
    Select,
    /// The cut tool: this drag draws a line and cuts what it crosses.
    Cut,
    /// A drag holding something the thing under the pointer will not take.
    ///
    /// The desktop's own "no drop" arrow rather than one of ours: refusing a
    /// drop is a gesture every window manager already has a picture for, and
    /// this is the one signal that is there even when the chip under the
    /// pointer is somewhere the eye is not.
    Deny,
}

impl Pointer {
    /// The glyph this pointer is drawn as, when it is one of ours.
    ///
    /// `None` for the ones the windowing system already has a good shape for.
    /// A resize arrow drawn by hand is a worse resize arrow — the value of a
    /// custom cursor is entirely in the shapes a desktop has no name for, and
    /// those are exactly the tools.
    pub fn icon(self) -> Option<crate::icon::Icon> {
        Some(match self {
            Self::Draw => crate::icon::Icon::Pencil,
            Self::Paint => crate::icon::Icon::Brush,
            Self::Erase => crate::icon::Icon::Eraser,
            Self::Select => crate::icon::Icon::Marquee,
            Self::Cut => crate::icon::Icon::Cut,
            _ => return None,
        })
    }
}

/// Everything the decision needs, borrowed from the window.
///
/// A wide struct rather than fifteen arguments: every field is geometry the
/// window has computed anyway, and naming them at the call site is what stops
/// two of them being passed the wrong way round.
pub struct PointerScene<'a> {
    pub layout: &'a WindowLayout,
    pub bar: &'a TransportBarLayout,
    pub rack: &'a RackLayout,
    pub browser: &'a BrowserLayout,
    pub roll: &'a RollLayout,
    pub roll_view: &'a RollView,
    pub roll_toolbar: &'a ToolbarLayout,
    pub tool: Tool,
    pub notes: &'a Arena<NoteId, Note>,
    pub timeline: &'a TimelineLayout,
    /// The arrangement's toolbar, so its buttons say they are buttons.
    pub timeline_bar: &'a TimelineToolbar,
    pub timeline_view: &'a TimelineView,
    pub clips: &'a [ClipInfo],
    pub instrument: &'a InstrumentLayout,
    pub instrument_view: Option<&'a InstrumentView>,
    pub mixer: &'a crate::canvas::MixerLayout,
    pub tabs: &'a EditorTabs,
    pub tab: EditorTab,
    /// What a held button is already doing. **Overrides everything**: dragging
    /// a note over the keyboard must not turn the cursor into a hand halfway
    /// through the gesture.
    pub dragging: Option<Pointer>,
}

/// The cursor for `(x, y)`.
pub fn pointer_at(scene: &PointerScene<'_>, x: f32, y: f32) -> Pointer {
    if let Some(dragging) = scene.dragging {
        return dragging;
    }

    // The transport bar, which is above everything.
    let bar = scene.bar;
    if bar.bar.contains(x, y) {
        if bar.play.contains(x, y) || bar.stop.contains(x, y) || bar.loop_toggle.contains(x, y) {
            return Pointer::Hand;
        }
        // The tempo is dragged up and down; the signature is stepped by a
        // click. Until the cursor said so, both read as labels — which is the
        // same complaint the snap chip drew before it grew a frame.
        if bar.tempo.contains(x, y) {
            return Pointer::ResizeY;
        }
        if bar.signature.contains(x, y) {
            return Pointer::Hand;
        }
        // The ruler is scrubbed, not clicked — see the transport's `Scrub`.
        return if bar.ruler.contains(x, y) {
            Pointer::Grab
        } else {
            Pointer::Default
        };
    }

    // Before the panels: the seams are the margin between them, so they are
    // outside every panel's frame and can be tested in any order — but tested
    // first they read as one rule rather than as three scattered ones.
    if scene.layout.sidebar_seam.contains(x, y) {
        return Pointer::ResizeX;
    }
    if scene.layout.sidebar_split.contains(x, y) {
        return Pointer::ResizeY;
    }
    if scene.layout.divider.contains(x, y) {
        return Pointer::ResizeY;
    }

    if scene.layout.rack.frame.contains(x, y) {
        return match rack_hit(scene.rack, x, y) {
            RackHit::Nothing => Pointer::Default,
            _ => Pointer::Hand,
        };
    }

    if scene.layout.browser.frame.contains(x, y) {
        return match browser_hit(scene.browser, x, y) {
            BrowserHit::Search(_) => Pointer::Text,
            BrowserHit::Nothing => Pointer::Default,
            _ => Pointer::Hand,
        };
    }

    if scene.timeline.toolbar.contains(x, y) {
        return match timeline_toolbar_hit(scene.timeline_bar, x, y) {
            Some(_) => Pointer::Hand,
            None => Pointer::Default,
        };
    }

    if scene.layout.timeline.frame.contains(x, y) {
        return match timeline_hit(scene.timeline_view, scene.timeline, scene.clips, x, y) {
            TimelineHit::Clip(_, ClipPart::RightEdge) => Pointer::ResizeX,
            TimelineHit::Clip(_, ClipPart::Body) => Pointer::Grab,
            TimelineHit::Ruler(_) => Pointer::Grab,
            TimelineHit::Lane(_) => Pointer::Hand,
            _ => Pointer::Default,
        };
    }

    if editor_tab_at(scene.tabs, x, y).is_some() {
        return Pointer::Hand;
    }

    if scene.tab == EditorTab::Mixer {
        use crate::canvas::MixerHit;
        return match crate::canvas::mixer_hit(scene.mixer, x, y) {
            // A fader is a vertical throw and a pan a horizontal one. The two
            // switches and the name are clicked.
            MixerHit::Fader(_) => Pointer::ResizeY,
            MixerHit::Pan(_) => Pointer::ResizeX,
            MixerHit::Mute(_)
            | MixerHit::Solo(_)
            | MixerHit::Name(_)
            | MixerHit::Strip(_)
            | MixerHit::AddTrack => Pointer::Hand,
            // The grip is the one thing in the options column you pick up
            // rather than press, and saying so is the only cue that a row can
            // be reordered at all.
            MixerHit::Options(crate::canvas::OptionsHit::Grip(_)) => Pointer::Grab,
            MixerHit::Options(_) => Pointer::Hand,
            MixerHit::Nothing => Pointer::Default,
        };
    }

    // The instrument's own knobs are not here any more: it is a window of its
    // own (see `crate::layout::EditorKind`), and this function answers "what
    // is under the pointer in the *main* window". Its cursor is
    // `instrument_pointer` below, which the editor window asks directly.

    // --- the piano roll ---
    if scene.roll.toolbar.contains(x, y) {
        return match toolbar_hit(scene.roll_toolbar, x, y) {
            Some(_) => Pointer::Hand,
            None => Pointer::Default,
        };
    }
    // Before the grid, because the grip is the last few pixels of it.
    if !scene.roll.velocity.is_empty() && scene.roll.lane_grip.contains(x, y) {
        return Pointer::ResizeY;
    }
    if scene.roll.velocity.contains(x, y) {
        return Pointer::ResizeY;
    }
    if scene.roll.keys.contains(x, y) {
        return Pointer::Hand;
    }
    if scene.roll.ruler.contains(x, y) {
        return Pointer::Grab;
    }
    match hit_test(scene.roll_view, scene.roll.grid, scene.notes, x, y) {
        // The delete tool erases whatever is under it, note or not, so it
        // says so over both. The cut tool is the same shape of answer: it
        // draws its line over anything.
        _ if scene.tool == Tool::Delete => Pointer::Erase,
        _ if scene.tool == Tool::Slice => Pointer::Cut,
        RollHit::Note(_, NotePart::RightEdge) => Pointer::ResizeX,
        // A left edge moves the note, like its body — see the roll's `press`.
        RollHit::Note(_, _) => Pointer::Grab,
        // Which tool is live, said by the pointer rather than only by the
        // toolbar: a crosshair is what every canvas in every program shows,
        // and it answers "you can click here" without answering "and this is
        // what will happen".
        RollHit::Empty { .. } => match scene.tool {
            Tool::Draw => Pointer::Draw,
            Tool::Paint => Pointer::Paint,
            Tool::Select => Pointer::Select,
            _ => Pointer::Default,
        },
        RollHit::Outside => Pointer::Default,
    }
}

/// The cursor for a point inside the **instrument editor's own window**
/// (TDD §7.2).
///
/// Split out of [`pointer_at`] when the instrument stopped being a tab of the
/// main window: the question is the same one and the answer is the same
/// answer, but it is asked about a different surface, so it cannot be reached
/// through a `PointerScene` describing the main one.
pub fn instrument_pointer(
    layout: &crate::canvas::InstrumentLayout,
    view: Option<&crate::canvas::InstrumentView>,
    x: f32,
    y: f32,
) -> Pointer {
    // The key row first, the way the press does: it sits above the controls
    // and is not among them, so a pointer over a chip has to say "this is a
    // button" rather than "this is a knob you cannot see".
    if crate::canvas::instrument_key_hit(layout, x, y).is_some() {
        return Pointer::Hand;
    }
    match (view, instrument_hit(layout, x, y)) {
        (Some(view), Some((group, param))) => match view.param(group, param) {
            // A knob is dragged up and down; a switch and a choice are
            // clicked, and promising a drag on them is a small lie that costs
            // somebody a gesture.
            Some(param) if param.kind == crate::canvas::ParamKind::Knob => Pointer::ResizeY,
            Some(_) => Pointer::Hand,
            None => Pointer::Default,
        },
        _ => Pointer::Default,
    }
}

// ---------------------------------------------------------- double clicks ---

/// How long two presses may be apart and still be one double-click.
///
/// The common desktop default, and the one a hand is used to. Longer and a
/// slow pair of deliberate single clicks becomes a double; shorter and a
/// double-click is something you have to practise.
pub const DOUBLE_CLICK_WINDOW: std::time::Duration = std::time::Duration::from_millis(400);

/// And how far apart on screen, in points.
///
/// A press is never perfectly still, and a hand moving back to the same place
/// twice is less still again. Four points is well inside one clip block and
/// well outside "the same spot".
pub const DOUBLE_CLICK_SLOP: f32 = 4.0;

/// Whether the press that just happened was the second of a pair.
///
/// Reported from using the window: *"double clicking on an audio clip should
/// open a menu."* Nothing in this window had any notion of a double-click —
/// every other panel opens on a single one, because a clip you clicked is a
/// clip you meant. An audio clip is the exception and the report says why:
/// clicking one is how you **move** it, and a window that appeared every time
/// you nudged a take along the bar would be in the way of the thing you were
/// doing.
///
/// A pure decision about two presses and where they were, so it is decided
/// here rather than inside an event loop where nothing could test it.
///
/// The `now` it is given is [`InputClock`]'s, **not** the wall clock — read
/// that type for why, and for the report that made it necessary.
#[derive(Debug, Clone, Copy, Default)]
pub struct DoubleClick {
    last: Option<(f32, f32, std::time::Instant)>,
}

impl DoubleClick {
    /// Records a press and says whether it completed a double-click.
    ///
    /// A double-click **consumes** the pair: a third press starts a fresh one
    /// rather than completing a second double, or a mash on the button would
    /// open a window per click.
    pub fn press(&mut self, x: f32, y: f32, now: std::time::Instant) -> bool {
        let doubled = self.last.is_some_and(|(px, py, at)| {
            now.duration_since(at) <= DOUBLE_CLICK_WINDOW
                && (x - px).abs() <= DOUBLE_CLICK_SLOP
                && (y - py).abs() <= DOUBLE_CLICK_SLOP
        });
        self.last = if doubled { None } else { Some((x, y, now)) };
        doubled
    }
}

/// The clock a gesture with a deadline is measured against: the wall clock,
/// less the time the window spent not listening.
///
/// > *"after working in a project for a while double clicking just doesnt make
/// > new clips anymore like it just stops letting me do that."*
///
/// A press waits its turn. winit hands the window a batch of events, the
/// window answers them and draws, and only then does it look at the queue
/// again — so `Instant::now()` inside a press handler is not when the press
/// **happened**, it is when the window got to it. Stamping a double-click with
/// that measures the pair against the window's own responsiveness: any stretch
/// longer than [`DOUBLE_CLICK_WINDOW`] — a big project's repaint, an autosave,
/// a plugin scan — turns one double-click into two single clicks, and a single
/// click on empty grid makes nothing by design (`Timeline::press`). Driving the
/// real window, two presses sent 80ms apart were handled 871ms apart.
///
/// So the time the window was not listening comes off the clock. **One frame's
/// worth of each busy stretch is charged**, because drawing is what a window is
/// for and nobody clicks twice inside a frame; what is longer than that is a
/// hitch the hand never saw, and a hand that saw nothing did not wait.
///
/// The one thing this trades away: while the window is hitching badly, two
/// deliberate clicks in the same spot can read as a double. That is the right
/// way round — an extra empty clip is one Ctrl+Z, and the gesture not working
/// at all is what was reported.
#[derive(Debug, Clone, Copy, Default)]
pub struct InputClock {
    /// How much has been taken off the clock so far — the hitches.
    skipped: std::time::Duration,
    /// When the window stopped listening, while it is not.
    busy_since: Option<std::time::Instant>,
}

impl InputClock {
    /// An event arrived: the window is busy from here until it waits again.
    ///
    /// Every event in a batch comes through here and the batch is **one**
    /// stretch — the second event of a batch does not start a second one, or a
    /// long batch would be charged a frame per event it happens to contain.
    pub fn busy(&mut self, now: std::time::Instant) {
        self.busy_since.get_or_insert(now);
    }

    /// Everything has been answered and the window is about to wait for input.
    pub fn listening(&mut self, now: std::time::Instant) {
        if let Some(since) = self.busy_since.take() {
            let busy = now.saturating_duration_since(since);
            self.skipped += busy.saturating_sub(crate::widget::FRAME_INTERVAL);
        }
    }

    /// What to stamp a press with: `now` on this clock.
    pub fn stamp(&self, now: std::time::Instant) -> std::time::Instant {
        // `checked_sub` for the one case it can fail: a monotonic clock counts
        // from boot, so a window opened seconds after one and hitching for
        // longer than it has been running would run off the bottom.
        now.checked_sub(self.skipped).unwrap_or(now)
    }
}

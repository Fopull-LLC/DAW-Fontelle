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
            BrowserHit::Search => Pointer::Text,
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
            | MixerHit::Insert(_, _)
            | MixerHit::BypassInsert(_, _)
            | MixerHit::AddInsert(_) => Pointer::Hand,
            MixerHit::Nothing => Pointer::Default,
        };
    }

    if scene.tab == EditorTab::Instrument {
        return match (
            scene.instrument_view,
            instrument_hit(scene.instrument, x, y),
        ) {
            (Some(view), Some((group, param))) => match view.param(group, param) {
                // A knob is dragged up and down; a switch and a choice are
                // clicked, and promising a drag on them is a small lie that
                // costs somebody a gesture.
                Some(param) if param.kind == crate::canvas::ParamKind::Knob => Pointer::ResizeY,
                Some(_) => Pointer::Hand,
                None => Pointer::Default,
            },
            _ => Pointer::Default,
        };
    }

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

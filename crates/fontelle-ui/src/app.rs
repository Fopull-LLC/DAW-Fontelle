//! The window: a `winit` event loop over a `wgpu` surface, drawing a `vello`
//! scene (TDD §16.2).
//!
//! This is deliberately the thinnest layer in the crate. Everything it decides
//! — the theme, the geometry, the glyph positions, and *whether a frame happens
//! at all* — is decided by a tested function somewhere else and read here
//! (`docs/first-usable-plan.md` §2.5). What is left is the part that genuinely
//! needs a window, and it is small enough to see all of.
//!
//! # Zero frames when idle (§16.3)
//!
//! The hard requirement, and the reason the loop is shaped the way it is:
//!
//! 1. The control flow is [`ControlFlow::Wait`]. With nothing to do the thread
//!    blocks in the compositor, using no CPU at all — not a timer, not a poll.
//! 2. Nothing calls `request_redraw` except a change: a resize, a theme swap, a
//!    widget invalidating itself. There is no "draw every frame" path to
//!    accidentally take.
//! 3. An animation (item 7's playhead) turns the loop continuous by holding a
//!    [`Redraw`] animator, and gives it back when it stops.
//!
//! [`WindowApp::frames_drawn`] counts what actually reached the GPU, so the
//! claim is checkable rather than asserted.

use std::sync::Arc;

use vello::util::{RenderContext, RenderSurface};
use vello::wgpu;
use vello::{AaConfig, RenderParams, Renderer, RendererOptions, Scene};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use crate::audition::{AuditionAction, Auditions};
use crate::canvas::{
    BrowserHit, BrowserLayout, BrowserMode, DEFAULT_LANE_HEIGHT, EdgeScroll, InstrumentLayout,
    InstrumentView, MixerHit, MixerLayout, Modifiers, MouseButton, ParamKind, PianoRoll, RackHit,
    RackLayout, RollControl, RollLayout, RouteChoice, RouteMenu, SnapDivision, Timeline,
    TimelineHit, TimelineLayout, Tool, ToolbarLayout, browser_file_share_at, browser_hit,
    browser_layout, browser_layout_split, clamp_to_grid, edge_scroll_rate, fader_db_at,
    format_gain_db, format_pan, instrument_hit, instrument_layout, knob_value, lane_height_at,
    mixer_hit, next_value, pan_at, rack_hit, rack_layout, roll_layout_with_keys, route_label,
    route_menu_hit, route_menu_layout, scrolled, snap_tick, timeline_hit, timeline_layout,
    timeline_snap, timeline_toolbar_hit, timeline_toolbar_layout, timeline_x_to_tick,
    timeline_zoom_x, timeline_zoom_y, toolbar_hit, toolbar_layout, x_to_tick, y_to_key, zoom_x,
    zoom_y,
};
use crate::canvas::{
    ToolAction, ToolKind, Tools, ToolsDialog, lane_menu_hit, lane_menu_layout, tools_dialog_hit,
    tools_dialog_layout,
};
use crate::document::{ChannelInfo, ClipInfo, LaneInfo, LibraryEntry, MixerStrip, StudioHost};
use crate::layout::{
    DEFAULT_TIMELINE_HEIGHT, Docks, EditorKind, EditorTab, EditorTabs, PanelLayout, WindowLayout,
    editor_tab_at, editor_tabs, editor_window_layout, rack_share_at, sidebar_width_at,
    timeline_height_at, window_layout_with,
};
use crate::pointer::{Pointer, PointerScene, pointer_at};
use crate::render::{
    ADD_CHANNEL, ARRANGEMENT, BrowserChrome, CHOOSE_FOLDER, Chrome, EMPTY_LANE, EXPORT,
    EditorWindowChrome, InstrumentChrome, MixerChrome, NEW_PROJECT, NO_INSTRUMENT, OPEN_FOLDER,
    RackChrome, RenderError, RollChrome, TAB_MIXER, TAB_ROLL, TimelineChrome, TransportChrome,
    draw_editor_window, draw_window, key_name, label_stride, labelled_bar,
};
use crate::text::{Labels, TextContext, TextLayout};
use crate::theme::Theme;
use crate::transport::{
    Meter, TransportAction, TransportBarLayout, TransportHit, TransportHost, TransportView, action,
    apply, cycle_beats_per_bar, format_readout, format_signature, format_tempo, hit, nudge_tempo,
    sample_at, step_beats_per_bar, tempo_at, transport_bar_layout,
};
use crate::widget::{Sleep, WidgetId, WidgetTree, autosave_due, sleep_budget};

/// What a held mouse button is in the middle of doing.
///
/// One value rather than a `bool` plus a guess. The old shape — "a button is
/// down, so send every pointer move to the piano roll" — is why dragging the
/// transport's ruler did nothing at all and why a drag that started on a list
/// still reached the roll. What a drag *is* is decided once, by the press, and
/// every move after it goes exactly one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Drag {
    None,
    /// The open menu's scrollbar. How far down the thumb it was taken hold of
    /// is in `menu_grab` — a `Drag` is compared and a float is not a thing to
    /// compare, the same arrangement `flop_knob` has.
    MenuScroll,
    /// Notes: drawing, moving, resizing, or a selection box.
    Roll,
    /// Values in the property lane.
    Lane,
    /// A knob on Flopsynth's window. Its own variant rather than `Knob`,
    /// because the two windows index their controls differently — a card and
    /// a group are not the same thing.
    FlopKnob,
    /// A picture on Flopsynth's window being dragged (§8.7): the oscillator's
    /// wave sideways, the filter's response in both directions.
    ///
    /// Its own variant per picture rather than one with a mode, because the
    /// two answer different numbers of controls — a wave moves the position
    /// and a response moves the corner *and* the resonance.
    FlopWave(usize),
    FlopResponse(usize),
    /// One corner of an envelope. Which one, and where the drag started, are
    /// in `flop_node` — the same arrangement `flop_knob` has, and for the same
    /// reason: a `Drag` is compared and a float is not a thing to compare.
    FlopEnvNode,
    /// A modulation ring: the depth of the newest route to that control.
    FlopRing,
    /// A source badge being carried to a knob (§8.4). Which source is in
    /// `flop_assign`, with where the pointer is, so the badge follows it.
    FlopAssign,
    /// A matrix row's depth slider.
    FlopMatrix(usize),
    /// The seam between the grid and the lane, which resizes the lane.
    LaneGrip,
    /// The roll's bar ruler — moves the time marker, in the clip's own ticks.
    RollRuler,
    /// The transport bar's ruler — the same, over the whole song.
    BarRuler,
    /// The on-screen keyboard: sliding down it sounds each key in turn.
    Keys,
    /// Clips on the arrangement: moving, sizing, or a selection box.
    Timeline,
    /// The arrangement's bar ruler — the time marker, in song ticks.
    TimelineRuler,
    /// The seam between the arrangement and the editor.
    Divider,
    /// The seam down the right of the sidebar, which sets its width.
    SidebarSeam,
    /// The seam across the sidebar, between the rack and the browser.
    SidebarSplit,
    /// The seam inside the soundfont panel, between the bank and its presets.
    BrowserSplit,
    /// A row carried **out** of the soundfont panel — a preset, or an audio
    /// file in the Import tab.
    ///
    /// *"i cannot drag an audio clip from the audio import tab into the
    /// channel rack to turn it into a sampler"*, and *"i should be able to
    /// drag soundfonts into it from the soundfonts window"*. Where it lands
    /// decides what it means, so the row is only carried here and acted on
    /// when the button comes up.
    BrowserRow(BrowserRow),
    /// A knob on the instrument editor.
    Knob,
    /// A track on the audio clip editor. Absolute, like the mixer's fader and
    /// for the same reason: the value goes where the press landed.
    AudioRow(crate::canvas::AudioField),
    /// One on an effect's own panel — a compressor's threshold, say. Its own
    /// variant because it writes to a different place, not because it behaves
    /// differently.
    InsertKnob,
    /// A fader on the mixer. Absolute, not relative: a press on the groove
    /// jumps the level to where it landed and then follows, which is how a
    /// fader in every DAW behaves and what makes "pull it right down" one
    /// gesture instead of a long drag.
    Fader(usize),
    /// A track's balance control, the same way.
    Pan(usize),
    /// The transport bar's tempo box. **Relative**, from where it was grabbed
    /// — see `value_drag`.
    Tempo,
    /// A band's handle on the EQ curve. Absolute: the handle goes where the
    /// pointer is, because the handle *is* the frequency and the gain.
    EqHandle(usize),
    /// The right button dragging a time selection out along the arrangement's
    /// ruler (TDD §6.3). Where it started is `select_anchor`, in song ticks.
    TimelineSelect,
    /// The same, along the roll's ruler, in the clip's ticks converted to the
    /// song's — one selection, drawn on both rulers.
    RollSelect,
    /// A send's level, dragged along its groove. **Absolute**, like a fader:
    /// a press takes it to where it landed and then follows.
    SendLevel(usize),
    /// One insert's wet/dry, dragged along its groove. Absolute, like the
    /// send's level and for the same reason.
    InsertMix(usize),
    /// One of the EQ's numbers, dragged up and down. **Relative**, unlike the
    /// handle on the curve: a read-out is not a position, so it moves from
    /// where it was by how far the mouse went.
    EqField(crate::canvas::EqField),
    /// An insert being dragged up or down its chain in the track-options
    /// column. The slot it started in and the slot it is over live in
    /// `insert_drag`; this only says who a `CursorMoved` belongs to.
    InsertRow,
}

/// Where the insert that was in `slot` ends up after one is moved from `from`
/// to `to`.
///
/// The chain is a `Vec` and the move is a remove-then-insert, so every slot
/// between the two shifts by one. The editor addresses an insert by its slot,
/// and a reorder that left it pointing at the neighbour would silently swap
/// which effect the panel of knobs is writing to.
fn reslot(slot: usize, from: usize, to: usize) -> usize {
    if slot == from {
        return to;
    }
    if from < to && (from + 1..=to).contains(&slot) {
        slot - 1
    } else if to < from && (to..from).contains(&slot) {
        slot + 1
    } else {
        slot
    }
}

/// Which canvas the keyboard is talking to.
///
/// Delete means "delete the selected notes" in one and "delete the selected
/// clips" in the other, and there is no reading of a keystroke that means both.
/// The last canvas pressed is the one that owns them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Roll,
    Timeline,
}

/// A row being dragged out of the soundfont panel — see [`Drag::BrowserRow`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserRow {
    /// A preset of the open soundfont, by row.
    Preset(usize),
    /// A file in the Import tab, by row. Only audio can be carried: a MIDI
    /// file dropped on the rack is not an instrument.
    File(usize),
}

/// What a right-click menu — or a rename — is about.
///
/// The menu canvas lists **strings** and knows nothing about what they do (see
/// [`crate::canvas::context_menu_layout`]); this is the other half, and it is
/// what turns "the third row was chosen" into an edit.
/// How long the window sleeps between frames while a plugin editor is open.
///
/// Sixty a second: a plugin's editor is repainted from this loop, so this is
/// its frame rate. Slower and a knob drags in steps; faster and an idle studio
/// is spinning for a window somebody else is drawing.
const PLUGIN_EDITOR_FRAME: std::time::Duration = std::time::Duration::from_millis(16);

/// How many rows one notch of the wheel moves a menu that scrolls.
///
/// Three is what every list in every desktop does, and a menu of 357 plugins
/// is the first list in this window long enough for anybody to notice.
const MENU_WHEEL_ROWS: f32 = 3.0;

/// What a name being typed is for.
///
/// > *"when i make a new project i need to be prompted to name it and also
/// > when im not in a project yet, i currently cant save that blank no project
/// > into a new project."*
///
/// The two are one gesture with two endings — somebody types a name and
/// presses Enter — and they differ only in what happens to what is already
/// open: *New* leaves it behind, *Save* takes it with them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NameFor {
    /// A fresh, empty project, made and opened. What is open now is left.
    NewProject,
    /// What is open now, saved into a project of that name.
    SaveAs,
}

impl NameFor {
    /// What the prompt says it is for.
    fn title(self) -> &'static str {
        match self {
            Self::NewProject => "Name the new project",
            Self::SaveAs => "Save this project as",
        }
    }
}

/// What a plugin chosen from the browser is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PluginPurpose {
    /// A new channel playing it.
    NewChannel,
    /// The channel at this rack position, playing it instead.
    ChangeChannel(usize),
    /// The end of this strip's insert chain.
    Insert(usize),
}

#[derive(Debug, Clone, PartialEq)]
enum MenuTarget {
    /// A channel in the rack, by row.
    Channel(usize),
    /// The mixer's *+ fx* row on this strip: which effect to put on the end
    /// of its chain. See [`crate::canvas::effect_menu_rows`].
    AddEffect(usize),
    /// Which plugin, of the ones installed on the machine (TDD §8.4).
    ///
    /// A second menu rather than rows on the first, because the two lists are
    /// different kinds of thing — see [`crate::canvas::EffectChoice::Plugin`].
    /// It carries what the choice is *for*, so that one browser serves the
    /// rack's add button, a channel's change-instrument menu and a strip's
    /// insert chain.
    PluginPicker(PluginPurpose),
    /// A row of the arrangement, by lane index.
    Lane(usize),
    /// A prefab in the prefab list, by row (TDD §10.5).
    Prefab(usize),
    /// One control on the instrument panel: the address the panel gave it, and
    /// what it is called.
    ///
    /// **Every** control, now that §8.2's `channel:<id>/patch/...` addresses
    /// exist: the channel's own level and placement, and the knobs inside the
    /// patch. See
    /// [`StudioHost::automate_instrument_param`](crate::document::StudioHost::automate_instrument_param).
    InstrumentParam {
        address: fontelle_types::ParamAddress,
        name: String,
    },
    /// One control on an effect's own panel, by the effect's stable id for it
    /// — a compressor's `threshold`. §12.4's "right-click any control", on the
    /// window that did not exist.
    InsertParam { param: String, name: String },
    /// The transport bar's tempo box. §12.3 names the tempo as automatable
    /// and it is a control like any other; the menu is how it becomes a lane.
    Tempo,
    /// Which part of a `.mid` file to bring in. The entries come from the
    /// host — see `StudioHost::import_prompt` — because it is the half that
    /// read the file.
    ImportChoice,
    /// The roll's Tools chip. Three tools that open a dialog and two importers
    /// that do not — see `canvas::TOOL_MENU`.
    RollTools,
    /// Asking for a project's name — see [`NameFor`].
    ///
    /// The typed name lives in `menu_filter`, which is the same field the
    /// plugin picker types into and for the same reason: a menu that is open
    /// already owns the keyboard.
    NameProject(NameFor),
    /// Which of the three instruments to **make**, from the rack's add button.
    /// *"when you select new instrument it lets you select one of those."*
    NewInstrument,
    /// And which to **turn channel `usize` into**, from its right-click menu.
    /// *"cant replace an instrument with a different instrument."*
    ChangeInstrument(usize),
    /// Whether to bounce a row's time selection or the whole of it.
    ///
    /// Only opened when there **is** a selection to ask about; with none there
    /// is nothing to choose between and the render goes straight ahead.
    RenderChoice(usize),
    /// The record button: *"when i click record it prompts me what i would
    /// like to record: notes, audio from mic, automation, etc."*
    RecordMode,
    /// A mixer strip, by position — what its name row renames. See
    /// [`crate::canvas::name_press`].
    MixerTrack(usize),
    /// A mixer strip's input button (TDD §15.4): which microphone feeds it.
    /// The strip, by position.
    TrackInput(usize),
    /// One point of an automation block: its shape, or its removal.
    Point {
        clip: fontelle_types::ClipId,
        id: fontelle_types::PointId,
    },
    /// A drop-down row on the audio clip editor — which mixer track, which
    /// filter shape, which fade curve. *"options that could be ... dropdowns
    /// for some reason are instead shown as buttons you click to toggle
    /// through a list of options in order."*
    AudioRow(crate::canvas::AudioField),
    /// The snap chip, on the roll's toolbar or the arrangement's — which grid
    /// things land on. `true` is the arrangement's; they are two views with
    /// two snaps, which is deliberate (a phrase is written on a finer grid
    /// than the blocks it is arranged into).
    Snap { timeline: bool },
    /// A [`ParamKind::Choice`] on the instrument panel or on an effect's own
    /// — a filter's slope, an LFO's shape. The same complaint as
    /// [`MenuTarget::AudioRow`] and the same answer: the list drops down.
    ///
    /// By **position** in the view rather than by address, because that is
    /// what `instrument_hit` hands back and what `InstrumentView::param`
    /// takes; the address is read off the parameter when the entry is chosen.
    ParamChoice {
        /// Which of the two panels — they are two windows and two views.
        editor: EditorKind,
        group: usize,
        param: usize,
    },
    /// The preset bar's drop-down, in the editor window of this kind
    /// (`docs/flopsynth-plan.md` §P.7): favourites first, then the bank
    /// grouped by category.
    PresetMenu(EditorKind),
    /// "Save as…" asking what to call it.
    PresetSaveName(EditorKind),
    /// And then which category to put it in — the device's own, plus a row
    /// that makes a new one. Two prompts because a preset needs a name *and*
    /// a folder, and one box cannot ask two questions.
    PresetCategory(EditorKind),
    /// The new category's name.
    PresetNewCategory(EditorKind),
    /// Flopsynth's `+ effect` list (`docs/flopsynth-plan.md` §8.5): which
    /// kind to put on the end of the instrument's own chain.
    AddPatchEffect,
}

impl MenuTarget {
    /// Which window draws a menu about this, when one is open.
    ///
    /// **Asked once, in one place**, because the two draw calls have to agree
    /// and there is no way to notice when they do not: a menu the main window
    /// declines to draw and the editor window also declines to draw is a menu
    /// that is *open* — holding the next click, swallowing Escape — and
    /// invisible. That is what happened the first time the import question
    /// went up, and no unit test could have seen it: the state was right, the
    /// geometry was right, and only this filter was wrong.
    ///
    /// It is written as "which of the two editor windows", with everything
    /// else belonging to the main one, so that adding a menu to the main
    /// window is nothing to remember.
    fn editor_window(&self) -> Option<EditorKind> {
        match self {
            Self::InstrumentParam { .. } => Some(EditorKind::Instrument),
            Self::InsertParam { .. } => Some(EditorKind::Effect),
            Self::AudioRow(_) => Some(EditorKind::AudioClip),
            Self::ParamChoice { editor, .. } => Some(*editor),
            // Every one of these is opened from a bar in an editor window's
            // header, so it is that window that draws it.
            Self::PresetMenu(editor)
            | Self::PresetSaveName(editor)
            | Self::PresetCategory(editor)
            | Self::PresetNewCategory(editor) => Some(*editor),
            Self::AddPatchEffect => Some(EditorKind::Instrument),
            Self::Snap { .. } => None,
            Self::Channel(_)
            | Self::AddEffect(_)
            | Self::NewInstrument
            | Self::ChangeInstrument(_)
            | Self::RenderChoice(_)
            | Self::Lane(_)
            | Self::Prefab(_)
            | Self::Tempo
            | Self::RollTools
            | Self::RecordMode
            | Self::MixerTrack(_)
            | Self::TrackInput(_)
            | Self::Point { .. }
            | Self::PluginPicker(_)
            | Self::NameProject(_)
            | Self::ImportChoice => None,
        }
    }
}

/// The effect's own id for a parameter, out of the full automation address the
/// panel labels its controls with.
///
/// `effect_view` addresses each control the way a saved automation lane does —
/// `mixer:<track>/insert[<slot>]/param/<id>` — which is what makes right-
/// clicking one need no translation. Writing one, and naming one to
/// `automate_insert`, both want the `<id>` on the end.
fn insert_param_id(address: &str) -> String {
    match fontelle_types::ParamTarget::parse(&fontelle_types::ParamAddress::new(address)) {
        Some(fontelle_types::ParamTarget::Insert { param, .. }) => param,
        // Already an id — the mixer's own rows name one directly.
        _ => address.to_string(),
    }
}

/// How fast the analyser's bars fall back, in decibels a second.
///
/// Thirty-six: a note's decay is visible as a decay rather than as a step, and
/// a bar that was hit half a second ago is no longer standing there claiming to
/// be sound.
const SPECTRUM_FALL_DB_PER_S: f32 = 36.0;

/// How often a backup is taken, while the window is being used.
///
/// A minute: often enough that losing the last one is losing a phrase rather
/// than an afternoon, and rare enough that it is invisible on a project whose
/// document is a few tens of kilobytes of JSON.
const AUTOSAVE_EVERY: std::time::Duration = std::time::Duration::from_secs(60);

/// How big a tool cursor's bitmap is.
///
/// Thirty-two, which is what every desktop expects and what a pencil needs to
/// still read as a pencil. Scaled by the compositor on a high-density display
/// like every other cursor is.
const CURSOR_SIZE: u32 = 32;

/// The piano roll's panel.
const PANEL: WidgetId = WidgetId::new(0);
/// The channel rack, and the soundfont browser under it (item 9). Their own
/// widgets so clicking a soundfont redraws the browser and not the roll.
const RACK: WidgetId = WidgetId::new(2);
const BROWSER: WidgetId = WidgetId::new(3);
/// The arrangement. Its own widget so dragging a clip redraws the strip and
/// not the roll under it.
const TIMELINE: WidgetId = WidgetId::new(4);
/// The transport bar (item 7). Its own widget so a moving playhead dirties the
/// bar and nothing else — §16.4's rule, applied before there is any geometry
/// expensive enough for it to matter, because that is the only time it is
/// cheap to get right.
const TRANSPORT: WidgetId = WidgetId::new(1);

/// Until the time signature is in the document, 4/4 — which is what the
/// importer and the demo both assume anyway.
const BEATS_PER_BAR: u32 = 4;

/// What the window is opened with.
pub struct WindowOptions {
    /// The OS window's title.
    pub title: String,
    /// The name in the panel's header.
    pub panel_title: String,
    pub theme: Theme,
    /// Logical pixels.
    pub size: (u32, u32),
    /// Close the window by itself after this long. `None` is the normal case;
    /// a duration is how an unattended run proves it started, drew, and idled
    /// without someone sitting there to close it.
    pub run_for: Option<std::time::Duration>,
    /// The engine the transport bar drives. `None` opens a window with the bar
    /// drawn but inert — no audio device, or no project yet.
    pub host: Option<Box<dyn TransportHost>>,
    /// The studio the window shows and edits: the open clip, the channel rack
    /// and the soundfont bank. `None` opens an empty window.
    pub document: Option<Box<dyn StudioHost>>,
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            title: "Fontelle".to_string(),
            panel_title: "Fontelle".to_string(),
            theme: Theme::dark_default(),
            size: (1280, 720),
            run_for: None,
            host: None,
            document: None,
        }
    }
}

#[derive(Debug)]
pub enum WindowError {
    EventLoop(String),
    Window(String),
    Render(RenderError),
}

impl std::fmt::Display for WindowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EventLoop(why) => write!(f, "the event loop could not start: {why}"),
            Self::Window(why) => write!(f, "the window could not be opened: {why}"),
            Self::Render(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for WindowError {}

/// Opens the window and runs until it is closed.
pub fn run_window(options: WindowOptions) -> Result<WindowApp, WindowError> {
    let event_loop = EventLoop::new().map_err(|e| WindowError::EventLoop(e.to_string()))?;
    // §16.3: sleep until the OS has something to say. This one line is the
    // idle-CPU target; `Poll` here would burn a core doing nothing.
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut app = WindowApp::new(options);
    event_loop
        .run_app(&mut app)
        .map_err(|e| WindowError::EventLoop(e.to_string()))?;
    match app.failure.take() {
        Some(e) => Err(e),
        None => Ok(app),
    }
}

/// One of this crate's cursors as the desktop's own vocabulary.
///
/// Its own function because two windows ask it now — the main one and every
/// floating editor — and a second copy of this match is a second place for a
/// shape to go stale.
fn system_cursor(wanted: Pointer) -> winit::window::CursorIcon {
    match wanted {
        Pointer::Default => winit::window::CursorIcon::Default,
        Pointer::Hand => winit::window::CursorIcon::Pointer,
        Pointer::Text => winit::window::CursorIcon::Text,
        // A note's or a clip's right-hand edge: it moves left and right,
        // which `EResize` says more precisely than a two-headed arrow.
        Pointer::ResizeX => winit::window::CursorIcon::EResize,
        Pointer::ResizeY => winit::window::CursorIcon::NsResize,
        Pointer::Grab => winit::window::CursorIcon::Grab,
        Pointer::Grabbing => winit::window::CursorIcon::Grabbing,
        // Only reached when the custom cursor could not be built — an old
        // compositor, or a platform with no custom cursors at all.
        Pointer::Draw | Pointer::Paint | Pointer::Select | Pointer::Cut => {
            winit::window::CursorIcon::Crosshair
        }
        Pointer::Erase => winit::window::CursorIcon::NotAllowed,
    }
}

/// The live window. Public so a caller can read [`WindowApp::frames_drawn`]
/// after the loop returns.
pub struct WindowApp {
    options: WindowOptions,
    text: TextContext,
    tree: WidgetTree,
    scene: Scene,
    context: RenderContext,
    /// One per device `RenderContext` has opened, indexed by its `dev_id`.
    renderers: Vec<Option<Renderer>>,
    live: Option<Live>,
    /// The compositor's activation protocol, where there is one — how an
    /// editor window is brought to the front under Wayland. See
    /// [`crate::activation`]. Opened on first use from the studio window,
    /// and `None` for good on X11.
    activation: Option<crate::activation::Activation>,
    activation_tried: bool,
    /// The floating editor windows that are open (TDD §7.2, §12, §13.4).
    /// At most one of each kind — see [`EditorKind`].
    editors: Vec<Editor>,
    /// Editors a click has asked for, opened on the next pass of the loop.
    ///
    /// A window can only be created with an `&ActiveEventLoop` in hand, and
    /// the handlers that decide to open one — a press on a rack row, on an
    /// insert, on an automation clip — are several calls deep inside one.
    /// Threading the loop down to all of them would put winit in every
    /// gesture; a queue keeps it at the edge.
    pending_editors: Vec<EditorKind>,
    /// Which window the pointer is in. `None` is the main one.
    ///
    /// `cursor` below is *that* window's coordinates, whichever it is: only
    /// one window has the pointer at a time, so one pair of numbers and a note
    /// of whose they are says everything two pairs would.
    pointer_window: Option<EditorKind>,
    title: TextLayout,
    layout: WindowLayout,
    frames: u64,
    started: std::time::Instant,
    failure: Option<WindowError>,

    // --- the transport bar (item 7) ---
    bar: TransportBarLayout,
    /// The engine's state as of the last tick. Compared against the next one
    /// to decide whether anything moved, which is what keeps a stopped,
    /// silent window at zero frames.
    view: TransportView,
    meters: [Meter; 2],
    readout: TextLayout,
    cursor: (f32, f32),
    /// Where the pointer was at the last step of a **relative** drag — what a
    /// delta is measured from. `None` while nothing relative is being dragged.
    pointer_anchor: Option<(f32, f32)>,
    modifiers: winit::keyboard::ModifiersState,
    hover: Option<TransportHit>,
    last_tick: std::time::Instant,
    roll: PianoRoll,
    roll_layout: RollLayout,
    roll_bar: ToolbarLayout,

    // --- the arrangement ---
    timeline: Timeline,
    timeline_layout: TimelineLayout,
    /// How tall the arrangement strip is. **Zero hides it**, and the editor
    /// takes the room.
    timeline_height: f32,
    /// The height it had when it was last showing, so the key that hides it can
    /// put it back the size it was.
    timeline_height_shown: f32,
    /// The lane chip's drop-down, while it is open.
    ///
    /// The chip said `vel`, which is a perfectly good *read-out* of what the
    /// lane is showing and a completely invisible control — hence
    /// *"I'm still not seeing panning options"* about a lane that could
    /// already draw all six. A menu lists what it can do without being asked.
    lane_menu: Option<crate::canvas::LaneMenu>,
    /// The menu a press has just **dismissed**, until the button comes up.
    ///
    /// A press that misses an open menu closes it and then carries on to
    /// whatever it hit, which is what every desktop does — and what makes a
    /// second press on a chip that *drops* a menu close it and open it again
    /// in one event. So the chip is told which menu it just shut, and declines
    /// to put the same one straight back. Cleared on release, so the press
    /// after it opens normally.
    dismissed: Option<MenuTarget>,
    /// What the Tools panel is set to — the transpose distance, the property
    /// the offset acts on, the randomizer's strength. Window state, not the
    /// document's: which property you last adjusted is no more part of a song
    /// than which tool is selected is.
    tools: Tools,
    /// The Import tab's rows, re-read when the studio's revision moves.
    imports: Vec<crate::document::LibraryEntry>,
    /// Which kind of file the Import tab is showing. Mirrored from the host
    /// on every refresh, like every other list this panel draws.
    import_kind: fontelle_types::FolderKind,
    /// The audio clip the editor window has open, if it has one — see
    /// [`OpenAudioClip`].
    audio_clip: Option<OpenAudioClip>,
    /// Where its rows are, inside the window's body.
    audio_layout: crate::canvas::AudioEditorLayout,
    /// The Tools panel while it is open, laid out. The same shape as
    /// `lane_menu` and for the same reason.
    tools_panel: Option<ToolsDialog>,
    /// How wide the sidebar has been dragged, and how its two panels share it.
    /// `None` until somebody drags the seam — see [`crate::layout::Docks`].
    sidebar_width: Option<f32>,
    rack_share: Option<f32>,
    lanes: Vec<LaneInfo>,
    clips: Vec<ClipInfo>,
    /// The other instruments' notes, ghosted behind the roll's own.
    ghosts: Vec<crate::document::GhostNote>,

    // --- the instrument editor (TDD §7.2) ---
    /// Which of the editor column's two views is showing.
    tab: EditorTab,
    tabs: EditorTabs,
    /// The selected channel's instrument, read with the rest of the studio's
    /// lists rather than once a frame.
    instrument: Option<InstrumentView>,
    instrument_layout: InstrumentLayout,
    /// The same channel as **Flopsynth's own window**, when that is what it is
    /// (`docs/flopsynth-plan.md` §8).
    ///
    /// Beside `instrument` rather than instead of it: exactly one of the two
    /// is drawn, and which is decided by whether the host offered this one.
    flopsynth: Option<crate::canvas::FlopsynthView>,
    flopsynth_layout: crate::canvas::FlopsynthLayout,
    /// Which of Flopsynth's four pages is showing (§8.3–§8.6).
    ///
    /// Window state rather than document state: which page you were last
    /// looking at is not a property of the song, and a project that reopened
    /// on the Effects page because that is where somebody left it would be
    /// saving a fact about a session.
    flop_page: crate::canvas::FlopsynthPage,
    /// A source badge being carried to a knob (§8.4): which source, and where
    /// the pointer is now, so the badge can be drawn under it.
    flop_assign: Option<(usize, (f32, f32))>,
    /// Which of Flopsynth's controls something modulates, and how deeply.
    ///
    /// Worked out with the rest of the studio's lists rather than per knob per
    /// frame: answering it walks the matrix, and there are a hundred and fifty
    /// knobs on that window.
    flop_modulated: Vec<((usize, usize), f32)>,
    /// And which of them *could* take a route — what lights up while a badge
    /// is in flight.
    flop_destinations: Vec<(usize, usize)>,
    /// How the Presets page is being looked at (§8.6): which shelf, what has
    /// been typed into its search, how far it is scrolled. Window state, like
    /// the page itself, and copied onto the view whenever the view is built.
    flop_browse: crate::canvas::PresetBrowse,
    /// Which insert the effect tab is showing: the strip, and the slot in its
    /// chain. `None` closes the tab — a tab for a thing that is not there is a
    /// tab that does nothing when clicked.
    open_insert: Option<(usize, usize)>,
    /// That insert's parameters, read with the rest of the studio's lists
    /// rather than once a frame.
    eq: Option<fontelle_types::EqConfig>,
    eq_layout: crate::canvas::EqLayout,
    /// The panel for an insert that is **not** an EQ — a grid of knobs read
    /// off the effect's own parameter list. Exactly one of this and `eq` is
    /// `Some` while an effect window is open. See
    /// [`StudioHost::insert_view`](crate::document::StudioHost::insert_view).
    insert_view: Option<InstrumentView>,
    /// Where its controls are, in that window.
    insert_layout: InstrumentLayout,
    /// One of them being dragged: which control, where the drag started, and
    /// the value it started from. The same shape as `knob`, for the same
    /// reason.
    insert_knob: Option<((usize, usize), f32, f32)>,
    eq_curve: Vec<(f32, f32)>,
    /// The analyser behind the curve, in decibels per band, **smoothed**.
    ///
    /// Read once a frame while the EQ window is open — like the mixer's meters
    /// and for the same reason (see [`StudioHost::spectrum`]) — and eased
    /// towards rather than replaced: a raw transform 60 times a second is a
    /// flicker rather than a picture. Fast up and slow down, which is what
    /// every analyser does and what makes a transient visible for long enough
    /// to see where it was.
    spectrum: Vec<f32>,
    /// The outline it is drawn as, in the curve's own rectangle.
    spectrum_points: Vec<(f32, f32)>,
    /// The selected band's own response, drawn faintly behind the sum.
    eq_band_curve: Vec<(f32, f32)>,
    hover_band: Option<usize>,
    /// The time selection, in song ticks, as the document holds it — and,
    /// while one is being dragged out on a ruler, the one under the pointer
    /// instead, so both rulers show the loop as it is drawn.
    loop_range: Option<(fontelle_types::Tick, fontelle_types::Tick)>,
    /// Where a ruler drag started, in song ticks, and what it has selected
    /// so far. See `Drag::TimelineSelect`.
    select_anchor: fontelle_types::Tick,
    select_preview: Option<(fontelle_types::Tick, fontelle_types::Tick)>,
    /// Song or clip — what pressing play plays. Read off the studio on its
    /// revision, like every other list here.
    play_mode: crate::document::PlayMode,
    /// The mode chip's word, shaped.
    mode_text: TextLayout,
    hover_param: Option<(usize, usize)>,
    /// Which of Flopsynth's controls the pointer is over, as `(card, param)`.
    ///
    /// Its own field rather than `hover_param`: the two windows index their
    /// controls differently — a card and a group are not the same thing — and
    /// one field meaning both would light the wrong knob the moment a channel
    /// changed kind.
    hover_card: Option<(usize, usize)>,
    /// The audio editor row the pointer is over, for its hover tip.
    hover_audio: Option<crate::canvas::AudioField>,
    /// Two presses in one place, which is how an audio clip's editor is opened
    /// — see [`crate::pointer::DoubleClick`].
    double_click: crate::pointer::DoubleClick,
    /// While an audio take is counting in: the song sample the tape starts at.
    ///
    /// A count-in is **not** a delay before the transport rolls. The transport
    /// rolls a bar early, the click sounds over it, and the tape starts at the
    /// marker — which is what makes the first beat of the take land on the
    /// first beat of the bar rather than a hand's reaction time after it.
    count_in_until: Option<fontelle_types::Sample>,
    /// Where the tape actually started, so the take lands where it was played.
    take_from: Option<fontelle_types::Sample>,
    /// Which preset chip the pointer is over, in the effect panel. Its own
    /// field rather than a `hover_param`, because a chip has no address —
    /// see `instrument_preset_hit`.
    hover_preset: Option<usize>,
    /// The preset bar in each editor window's header (`docs/flopsynth-plan.md`
    /// §P.7): what it says, where it is, and what the pointer is over.
    ///
    /// One per window rather than one shared, because the two windows are open
    /// at once and show two devices — a synth and the reverb on its bus have
    /// different presets and different `*`s.
    preset_view: [crate::canvas::PresetBarView; 2],
    /// What the pointer is over on one of them: which window, and which
    /// control. The geometry itself is recomputed from the window's header
    /// (`preset_bar_geometry`) rather than kept, because it depends on the
    /// measured width of the window's title and that changes with the name.
    hover_preset_bar: Option<(usize, crate::canvas::PresetBarHit)>,
    /// An envelope node being dragged: which card, which corner, where the
    /// drag started and the value it started from.
    flop_node: Option<(usize, crate::canvas::EnvNode, (f32, f32), f32)>,
    /// A modulation ring being dragged: which control, where it started, and
    /// the depth it started from.
    flop_ring: Option<((usize, usize), f32, f32)>,
    /// A "Save as…" half-finished: which window, and the name typed, waiting
    /// for the category to be chosen. Two prompts because a preset needs both
    /// and one box cannot ask two questions.
    pending_save_as: Option<(usize, String)>,
    /// And which key chip, in the same panel.
    hover_key: Option<usize>,
    hover_tab: Option<EditorTab>,
    /// The control being dragged, where the drag started, and the value it
    /// started from — a knob measures from where it was grabbed, not from
    /// wherever the pointer happens to be.
    knob: Option<((usize, usize), f32, f32)>,
    /// The same, for Flopsynth's window: which control, where the drag
    /// started, and the value it started from.
    flop_knob: Option<((usize, usize), f32, f32)>,

    // --- the mixer (TDD §13) ---
    mixer: MixerLayout,
    /// The tracks, read with the rest of the studio's lists on its revision.
    mixer_strips: Vec<MixerStrip>,
    /// Their meters, read once a **frame** instead, and only while the mixer
    /// is the tab showing — a level moves every block, and putting it on the
    /// revision would rebuild every panel in the window sixty times a second.
    mixer_peaks: Vec<[f32; 2]>,
    mixer_scroll: usize,
    hover_mixer: Option<MixerHit>,
    /// Which strip the track-options column is about, read with the rest of
    /// the studio's lists.
    selected_track: usize,
    /// Where that strip's output goes, as an index into `route_names` —
    /// `None` is the master. Read with the lists rather than per frame,
    /// because the column's one caption is shaped from it.
    track_output: Option<usize>,
    /// Every mixer track a clip can be routed to, master first as `None`,
    /// paired with what it is called.
    ///
    /// One list, read with the studio's others, because three things want it:
    /// the audio editor's route row draws the name, its click steps through
    /// the ids, and both have to agree with the order the mixer lays its
    /// strips out in.
    clip_routes: Vec<(Option<fontelle_types::MixerTrackId>, String)>,
    /// What the open audio clip's route row says, kept rather than formatted
    /// per frame — the chrome takes a `&str` and a temporary would not outlive
    /// the borrow it goes into.
    audio_route: String,
    /// And whether it is connected at all — see
    /// `fontelle_model::MixerTrack::output_on`. Read with the rest of the
    /// studio's lists, because the row that draws it is drawn every frame and
    /// the answer changes only when a command runs.
    track_output_on: bool,
    /// The send menu, while it is open, and which send opened it — `None` for
    /// the row that makes a new one.
    send_menu: Option<(Option<usize>, crate::canvas::RouteMenu)>,
    /// The output row's menu, while it is open. The same shape as
    /// `route_menu`, and deliberately a *different* field: the rack's chip and
    /// the column's row are two menus over the same list, and one of them
    /// being open must not dismiss the other's press.
    output_menu: Option<crate::canvas::RouteMenu>,
    /// An insert being dragged up or down its chain: which slot it started in,
    /// and which it is over now.
    insert_drag: Option<(usize, usize)>,

    // --- the transport bar's two document boxes ---
    /// The tempo at the start of the piece, read with the studio's lists.
    tempo: f64,
    tempo_text: TextLayout,
    signature_text: TextLayout,
    /// What the two boxes above were last shaped *from*, so a frame where
    /// neither moved does no shaping at all. `NAN` and zero are the "nothing
    /// yet" values, and both compare unequal to any real one.
    shaped_tempo: f64,
    shaped_beats: u32,
    /// What a tempo drag started from, and where the pointer was when it did.
    /// The same shape as `knob`, for the same reason.
    value_drag: Option<(f64, f32)>,
    /// Which canvas Delete and Ctrl+B are addressed to.
    focus: Focus,

    // --- the docked panels (item 9) ---
    /// Everything the chrome draws that had to be shaped first. See
    /// [`crate::text::Labels`].
    labels: Labels,
    rack: RackLayout,
    browser: BrowserLayout,
    /// The studio's revision as of the last read, so the lists below are
    /// rebuilt when something changes them and not once a frame.
    studio_revision: u64,
    channels: Vec<ChannelInfo>,
    files: Vec<LibraryEntry>,
    presets: Vec<LibraryEntry>,
    selected_channel: usize,
    selected_file: Option<usize>,
    /// Which preset the selected channel is playing, so the browser can point
    /// at it. Read from the studio with the rest of the lists.
    selected_preset: Option<usize>,
    /// Where the playhead and the marker are in *song* ticks, for the
    /// arrangement. Read once per tick, like everything else about the engine.
    song_playhead: fontelle_types::Tick,
    song_marker: fontelle_types::Tick,
    /// Which of the browser's two lists is showing — the soundfont bank or
    /// the projects folder. Window state, not the studio's: it is a view, and
    /// the host answers both questions whichever is on screen.
    browser_mode: BrowserMode,
    /// The projects in the configured folder, read with the studio's other
    /// lists.
    projects: Vec<LibraryEntry>,
    /// What Fontelle is set to, as name-and-value rows — read with the
    /// studio's other lists, and a `LibraryEntry` for the reason
    /// [`StudioHost::settings`] gives.
    settings: Vec<LibraryEntry>,
    /// How many soundfonts the whole collection holds — the browser panel's
    /// heading, which does not change as you browse into a folder.
    library_count: usize,
    /// The live search (TDD §17.5), and whether it has the keyboard.
    query: String,
    searching: bool,
    /// Which preset row the keyboard is on, so the arrow keys can walk the
    /// list and Enter can choose from it — *"go through the selected
    /// instruments with arrow keys after it being clicked on to focus it"*.
    /// `None` when the browser does not hold the keyboard, which is when the
    /// arrows go back to nudging notes and clips.
    preset_focus: Option<usize>,
    status: String,
    rack_scroll: usize,
    /// The prefab list's own scroll offset — its own, because the two lists in
    /// the panel scroll independently and a shared one would jump when you
    /// switched tabs.
    prefab_scroll: usize,
    file_scroll: usize,
    preset_scroll: usize,
    hover_control: Option<RollControl>,
    /// The arrangement's toolbar, laid out, and which control the pointer is
    /// over — the same pair the roll's toolbar has.
    timeline_bar: crate::canvas::TimelineToolbar,
    hover_timeline: Option<crate::canvas::TimelineControl>,
    /// What in the browser the pointer is over — a button *or* a row, because
    /// a list whose rows do not light up under the pointer does not look like a
    /// list you can click.
    hover_browser: Option<BrowserHit>,
    /// The same for the channel rack.
    hover_rack: Option<RackHit>,
    /// And for the prefab list under the panel's other tab.
    hover_prefab: Option<crate::canvas::PrefabHit>,
    /// Which list the left-hand panel is showing, and the list itself. Cached
    /// on the studio's revision like `channels`, for the reason
    /// `StudioHost::revision` gives.
    rack_tab: crate::document::RackTab,
    prefabs: Vec<crate::document::PrefabInfo>,
    /// The prefab list's geometry, when that is the tab showing.
    prefab_panel: crate::canvas::PrefabLayout,
    /// The right-click menu, while it is open: what it is about, and where it
    /// was dropped. See [`MenuTarget`].
    menu: Option<(MenuTarget, crate::canvas::ContextMenu)>,
    /// Where the open menu was dropped from, and in what room.
    ///
    /// Kept because the plugin picker is **re-laid-out as you type** — see
    /// [`Self::menu_filter`] — and a menu that jumped to the pointer on every
    /// keystroke would be a menu you cannot read.
    menu_at: ((f32, f32), crate::layout::Rect),
    /// What has been typed into the plugin picker since it opened.
    ///
    /// > *"its still not seeing my plugins"* — with 357 effects installed,
    /// > a list you can only scroll is sixteen screens deep. Empty for every
    /// > other menu, which do not filter: six entries need no search box.
    menu_filter: String,
    /// What is being renamed, while somebody is typing a name.
    ///
    /// There is no text buffer beside it, and deliberately: every keystroke
    /// goes straight through the rename command, which coalesces (see
    /// `RenameChannel::merge_with`), so the *document* is the buffer, the row
    /// redraws as you type without anything special being drawn, and Ctrl+Z
    /// takes back the whole name rather than the last letter.
    renaming: Option<MenuTarget>,
    /// The route chip's menu, while it is open, and whose row opened it.
    ///
    /// The same shape as `lane_menu` and for the same reason: a menu is state
    /// the window holds between a press and the press that dismisses it, and
    /// there is exactly one open at a time.
    route_menu: Option<(usize, RouteMenu)>,
    /// What every mixer strip is called, read with the rest of the studio's
    /// lists — the route chip's numbering and the menu's captions both come
    /// out of it.
    route_names: Vec<String>,
    /// Which input the selected strip records from — see
    /// `StudioHost::track_input`.
    track_input: Option<String>,
    /// What the cursor is showing, so it is only set when it changes — a
    /// `set_cursor` on every pointer move is a round trip to the compositor
    /// per event.
    pointer: Pointer,
    /// The cursors drawn from this crate's own icons, built once when the
    /// window comes up. Empty on a platform that will not take them, and then
    /// the desktop's own shapes are used instead.
    tool_cursors: std::collections::HashMap<Pointer, winit::window::CustomCursor>,
    /// The browser panel's heading, kept rather than formatted per frame — the
    /// renderer takes a `&str` and `Labels` is keyed by the string itself.
    browser_title: String,
    /// Which keys the selected channel's instrument can play, and what each
    /// one is called. Cached with the other lists and refreshed on the host's
    /// revision, because it is read three times a frame — to size the key
    /// strip, to shape the names, and to draw — and rebuilding it in each of
    /// those is how the three come to disagree.
    key_map: crate::document::KeyMap,
    /// Whether the roll's key strip is a keyboard or a list of names, for the
    /// selected channel. Cached with the other lists and refreshed on the
    /// host's revision, like the key map it is drawn beside.
    key_style: crate::canvas::KeyStyle,
    /// Which band of the open EQ the controls under the curve describe, and
    /// which handle is drawn as the one in hand.
    ///
    /// The editor's own state rather than the document's: which band you are
    /// looking at is not part of the song, and a project that reopened with a
    /// selected band would be a project storing where somebody's mouse was.
    eq_band: usize,
    /// A number in the EQ being dragged: what it was when the drag started,
    /// and where the pointer was. See [`Drag::EqField`].
    eq_drag: Option<(f32, f32)>,
    /// The same, for an insert's wet/dry dial in the track-options column.
    mix_drag: Option<(f32, f32)>,
    /// Which of the EQ's controls the pointer is over, so it lights before it
    /// is pressed.
    hover_eq_field: Option<crate::canvas::EqField>,
    /// Which keys a MIDI keyboard is holding down, as of the last frame, one
    /// bit each. Read from the host once a frame like the mixer's meters —
    /// see [`StudioHost::live_keys`](crate::document::StudioHost::live_keys).
    live_keys: u128,
    /// What the held mouse button is doing, so a `CursorMoved` reaches the one
    /// thing that asked for it.
    drag: Drag,
    /// How far down the scrollbar's thumb a menu-scroll drag took hold of
    /// it — see [`Drag::MenuScroll`]. Meaningless while that drag is not
    /// running.
    menu_grab: f32,
    /// How much of the soundfont panel the bank gets, against its presets.
    /// `None` until the seam between them is dragged.
    browser_file_share: Option<f32>,
    /// What an edge-held drag has scrolled but not yet spent — see
    /// [`EdgeScroll`]. Beside `drag` because it belongs to the gesture and is
    /// cleared with it.
    edge_scroll: EdgeScroll,
    /// When the edge scroll last advanced, so the next step knows how much
    /// time to charge for. `None` between gestures.
    edge_scroll_at: Option<std::time::Instant>,
    /// The **time marker**: the last place the user clicked on either ruler.
    /// Play starts here and a stop comes back here — see
    /// [`crate::transport::TransportAction`].
    marker: fontelle_types::Sample,
    /// How tall the property lane was when it was last showing, so the button
    /// that hides it can put it back the size it was.
    lane_height_shown: f32,
    /// The live-path note the pointer is holding — see
    /// [`crate::audition::Auditions`], which is where the bookkeeping and both
    /// of its regression tests live.
    audition: Auditions,
    /// What the thing under the pointer says it does, and when the pointer
    /// arrived on it. See [`crate::tooltip`].
    ///
    /// The **string** is what the dwell is measured against, not the hover
    /// enum: moving along a row of four mute buttons changes the enum every
    /// time and the explanation not at all, and a tip that restarted its
    /// countdown at each one would never appear.
    hover_tip: Option<String>,
    hover_since: std::time::Instant,
    /// Where the tip was last drawn, so the region it covered can be dirtied
    /// when it goes away.
    tip_rect: crate::layout::Rect,
    /// When a backup was last considered. See `maybe_autosave`.
    last_autosave: std::time::Instant,
    /// Whether a plugin is showing an editor of its own.
    ///
    /// Kept so `arm_deadline` can hold the loop awake for it: a plugin's
    /// editor is drawn by the plugin, on this thread, out of the timer this
    /// loop fires — see `tick_plugin_editors`.
    plugin_editor_open: bool,
    /// Whether this window currently holds an animator on the tree's
    /// [`crate::widget::Redraw`]. Kept so `begin`/`end` stay paired — the
    /// counter is there so several moving things can coexist, and a caller
    /// that begins twice for one thing defeats it.
    animating: bool,
}

/// The window and its surface, which only exist between `resumed` and
/// `suspended` — on mobile the surface really does go away and come back, and
/// pretending otherwise is how a desktop-only assumption gets baked in.
struct Live {
    window: Arc<Window>,
    surface: RenderSurface<'static>,
}

/// One floating editor window (TDD §7.2, §12, §13.4).
///
/// The instrument, the effect and the automation curve were tabs of the editor
/// column, and §7.5 is why they are not any more: a VST or a CLAP editor is
/// handed a parent window and draws into it, so an editor that can only exist
/// as one of that column's tabs is one the plugin path can never be built on.
/// Making them windows now — while the only devices are Fontelle's own — is
/// what makes hosting somebody else's a matter of putting a different surface
/// behind the same window rather than of rebuilding how an editor is opened.
///
/// It shares the main window's `RenderContext` and its per-device `Renderer`:
/// shader compilation is most of the cold-start budget (§19), and paying it
/// again per window would make opening an EQ feel like launching an app.
/// The audio clip the editor window is showing (TDD §15.1).
///
/// Its properties are held here rather than read from the document per frame
/// for the reason every other list in this window is cached: the window redraws
/// on a pointer move and the document only changes when somebody clicks
/// something. `revision` is what puts them back in step.
struct OpenAudioClip {
    id: fontelle_types::ClipId,
    /// What the title bar says — the file it came from.
    name: String,
    data: fontelle_types::AudioClipData,
    /// The file's own rate, so a fade reads in milliseconds.
    sample_rate: u32,
}

struct Editor {
    kind: EditorKind,
    window: Arc<Window>,
    surface: RenderSurface<'static>,
    /// Its header and the body the panel is drawn in, in logical pixels.
    panel: PanelLayout,
    /// What its header says, shaped.
    title: TextLayout,
    /// Its own scene. One per window, because a `Scene` is the picture of a
    /// surface and two surfaces are two pictures.
    scene: Scene,
    /// This window was just asked to come to the front and is being held above
    /// the others until it has been drawn once — see [`WindowApp::raise_editor`].
    /// Cleared on its next frame, which puts it back to an ordinary window.
    pinned: bool,
}

impl WindowApp {
    fn new(options: WindowOptions) -> Self {
        let mut text = TextContext::new();
        let title = text.layout(&options.panel_title, &options.theme.font, None);
        let layout = window_layout_with(
            options.size.0 as f32,
            options.size.1 as f32,
            &options.theme.metrics,
            &Docks::default(),
        );
        Self {
            text,
            title,
            layout,
            tree: WidgetTree::new(),
            scene: Scene::new(),
            context: RenderContext::new(),
            renderers: Vec::new(),
            live: None,
            editors: Vec::new(),
            activation: None,
            activation_tried: false,
            pending_editors: Vec::new(),
            pointer_window: None,
            frames: 0,
            started: std::time::Instant::now(),
            failure: None,
            bar: transport_bar_layout(layout.transport, &options.theme.metrics),
            view: TransportView::unavailable(),
            meters: [Meter::new(); 2],
            readout: TextLayout::default(),
            cursor: (f32::MIN, f32::MIN),
            pointer_anchor: None,
            modifiers: winit::keyboard::ModifiersState::empty(),
            hover: None,
            last_tick: std::time::Instant::now(),
            roll: PianoRoll::new(Default::default()),
            // The narrow strip to start with: nothing is known about the
            // channel's instrument until the first refresh, and `unknown`
            // names nothing. `relayout_panels` widens it if it turns out to
            // be a kit.
            roll_layout: roll_layout_with_keys(
                layout.panel.body,
                &options.theme.metrics,
                DEFAULT_LANE_HEIGHT,
                crate::canvas::KEYBOARD_WIDTH,
            ),
            roll_bar: ToolbarLayout { items: Vec::new() },
            timeline: Timeline::new(Default::default()),
            timeline_layout: timeline_layout(layout.timeline.body, &options.theme.metrics),
            timeline_height: DEFAULT_TIMELINE_HEIGHT,
            timeline_height_shown: DEFAULT_TIMELINE_HEIGHT,
            lane_menu: None,
            dismissed: None,
            plugin_editor_open: false,
            tools: Tools::default(),
            imports: Vec::new(),
            import_kind: fontelle_types::FolderKind::Midi,
            audio_clip: None,
            audio_layout: crate::canvas::AudioEditorLayout {
                waveform: crate::layout::Rect::ZERO,
                rows: Vec::new(),
            },
            tools_panel: None,
            sidebar_width: None,
            rack_share: None,
            lanes: Vec::new(),
            clips: Vec::new(),
            ghosts: Vec::new(),
            focus: Focus::Roll,
            tab: EditorTab::Roll,
            tabs: editor_tabs(layout.panel.header, &options.theme.metrics),
            instrument: None,
            instrument_layout: InstrumentLayout {
                name: crate::layout::Rect::ZERO,
                body: layout.panel.body,
                keys: Vec::new(),
                headings: Vec::new(),
                cells: Vec::new(),
                content_height: 0.0,
            },
            flopsynth: None,
            flop_page: crate::canvas::FlopsynthPage::Synth,
            flop_assign: None,
            flop_modulated: Vec::new(),
            flop_destinations: Vec::new(),
            flop_browse: Default::default(),
            flopsynth_layout: crate::canvas::FlopsynthLayout {
                body: layout.panel.body,
                ..Default::default()
            },
            open_insert: None,
            eq: None,
            eq_layout: crate::canvas::eq_layout(
                layout.panel.body,
                &options.theme.metrics,
                &fontelle_types::EqConfig::new(),
            ),
            insert_view: None,
            insert_layout: InstrumentLayout {
                name: crate::layout::Rect::ZERO,
                body: layout.panel.body,
                keys: Vec::new(),
                headings: Vec::new(),
                cells: Vec::new(),
                content_height: 0.0,
            },
            insert_knob: None,
            eq_curve: Vec::new(),
            spectrum: Vec::new(),
            spectrum_points: Vec::new(),
            eq_band_curve: Vec::new(),
            hover_band: None,
            loop_range: None,
            select_anchor: 0,
            select_preview: None,
            play_mode: crate::document::PlayMode::Song,
            mode_text: TextLayout::default(),
            hover_param: None,
            hover_card: None,
            hover_audio: None,
            double_click: crate::pointer::DoubleClick::default(),
            count_in_until: None,
            take_from: None,
            hover_preset: None,
            preset_view: [
                crate::canvas::PresetBarView::default(),
                crate::canvas::PresetBarView::default(),
            ],
            hover_preset_bar: None,
            flop_node: None,
            flop_ring: None,
            pending_save_as: None,
            hover_key: None,
            hover_tab: None,
            knob: None,
            flop_knob: None,
            mixer: MixerLayout {
                body: layout.panel.body,
                list: layout.panel.body,
                strips: Vec::new(),
                master: None,
                add_track: crate::layout::Rect::ZERO,
                options: None,
                total: 0,
                scroll: 0,
            },
            mixer_strips: Vec::new(),
            mixer_peaks: Vec::new(),
            mixer_scroll: 0,
            hover_mixer: None,
            selected_track: 0,
            track_output: None,
            clip_routes: Vec::new(),
            audio_route: String::new(),
            track_output_on: true,
            output_menu: None,
            send_menu: None,
            insert_drag: None,
            tempo: 120.0,
            tempo_text: TextLayout::default(),
            signature_text: TextLayout::default(),
            shaped_tempo: f64::NAN,
            shaped_beats: 0,
            value_drag: None,
            labels: Labels::new(),
            rack: rack_layout(layout.rack.body, &options.theme.metrics, 0, 0),
            browser: browser_layout(layout.browser.body, &options.theme.metrics, 0, 0, 0, 0),
            studio_revision: u64::MAX,
            channels: Vec::new(),
            files: Vec::new(),
            presets: Vec::new(),
            selected_channel: 0,
            selected_file: None,
            selected_preset: None,
            song_playhead: 0,
            song_marker: 0,
            browser_mode: BrowserMode::default(),
            settings: Vec::new(),
            projects: Vec::new(),
            library_count: 0,
            query: String::new(),
            searching: false,
            preset_focus: None,
            status: String::new(),
            key_map: crate::document::KeyMap::unknown(),
            key_style: crate::canvas::KeyStyle::default(),
            eq_band: 0,
            eq_drag: None,
            mix_drag: None,
            hover_eq_field: None,
            live_keys: 0,
            timeline_bar: crate::canvas::TimelineToolbar { items: Vec::new() },
            hover_timeline: None,
            rack_scroll: 0,
            prefab_scroll: 0,
            hover_prefab: None,
            rack_tab: crate::document::RackTab::default(),
            prefabs: Vec::new(),
            prefab_panel: crate::canvas::prefab_layout(
                layout.rack.body,
                &options.theme.metrics,
                0,
                0,
            ),
            file_scroll: 0,
            preset_scroll: 0,
            hover_control: None,
            hover_browser: None,
            hover_rack: None,
            menu: None,
            menu_at: ((0.0, 0.0), crate::layout::Rect::ZERO),
            menu_filter: String::new(),
            renaming: None,
            route_menu: None,
            route_names: Vec::new(),
            track_input: None,
            pointer: Pointer::Default,
            tool_cursors: std::collections::HashMap::new(),
            browser_title: "Soundfonts".to_string(),
            drag: Drag::None,
            menu_grab: 0.0,
            browser_file_share: None,
            edge_scroll: EdgeScroll::default(),
            edge_scroll_at: None,
            audition: Auditions::default(),
            hover_tip: None,
            hover_since: std::time::Instant::now(),
            tip_rect: crate::layout::Rect::ZERO,
            last_autosave: std::time::Instant::now(),
            marker: 0,
            lane_height_shown: DEFAULT_LANE_HEIGHT,
            animating: false,
            options,
        }
    }

    /// The time signature's numerator, from the document — or 4/4 when there
    /// is no document behind the window.
    ///
    /// One accessor rather than the constant scattered about, because the
    /// signature is a document value now and a grid that counts by a different
    /// number from the read-out beside it is worse than one that is wrong
    /// everywhere.
    fn beats_per_bar(&self) -> u32 {
        self.options
            .document
            .as_ref()
            .map_or(BEATS_PER_BAR, |doc| doc.beats_per_bar())
    }

    /// How many frames actually reached the GPU. The evidence for §16.3.
    pub fn frames_drawn(&self) -> u64 {
        self.frames
    }

    /// Recomputes the geometry for a new size and marks everything dirty.
    fn resize(&mut self, width: u32, height: u32, scale: f64) {
        let logical = (width as f64 / scale, height as f64 / scale);
        self.layout = window_layout_with(
            logical.0 as f32,
            logical.1 as f32,
            &self.options.theme.metrics,
            &self.docks(),
        );
        self.bar = transport_bar_layout(self.layout.transport, &self.options.theme.metrics);
        self.relayout_panels();
        self.tree.insert(TRANSPORT, self.layout.transport);
        self.tree.insert(PANEL, self.layout.panel.frame);
        self.tree.insert(RACK, self.layout.rack.frame);
        self.tree.insert(BROWSER, self.layout.browser.frame);
        self.tree.insert(TIMELINE, self.layout.timeline.frame);
        // Re-shaped against the header it has to fit in: a title wider than its
        // panel should wrap inside the header rather than run out over the
        // window, and how wide that is only becomes known here.
        let room = self.layout.panel.header.width - 2.0 * self.options.theme.metrics.panel_padding;
        self.title = self.text.layout(
            &self.options.panel_title,
            &self.options.theme.font,
            (room > 0.0).then_some(room),
        );
        // A resize invalidates the lot: the compositor hands back a surface
        // with nothing in it.
        self.tree.invalidate_rect(self.layout.window);
    }

    fn draw(&mut self) {
        if self.live.is_none() {
            return;
        }
        // Everything the frame is about to need, before the frame: the panels'
        // contents when the studio has changed them, and a shaped form of
        // every string the (pure) renderer will look up.
        self.refresh_studio();
        self.shape_labels();
        // Read before the surface is borrowed: the chrome below is built
        // inside a closure that already holds a mutable borrow of `self`.
        let beats_per_bar = self.beats_per_bar();
        let output_label = self.output_caption();
        let input_label = self.input_caption();
        // The tip's box needs the shaped width of its own words, so it is
        // measured here — after `shape_labels`, before the borrow.
        let tooltip = self.due_tip().map(str::to_string).and_then(|caption| {
            let text = self.labels.get(&caption)?;
            let rect = crate::tooltip::tooltip_layout(
                (text.width, text.height),
                self.cursor,
                self.layout.window,
            );
            (!rect.is_empty()).then_some((caption, rect))
        });
        // Remembered so the region it covered can be painted over when it goes
        // away — it floats outside every widget's own bounds.
        self.tip_rect = tooltip
            .as_ref()
            .map_or(crate::layout::Rect::ZERO, |(_, r)| *r);

        // §16.3, the whole of it: no dirty region, no frame.
        let Some(_region) = self.tree.take_dirty() else {
            return;
        };
        let Some(live) = &self.live else { return };

        let device = &self.context.devices[live.surface.dev_id];
        // Worked out before the renderer is borrowed mutably: it reads the
        // document and the transport view, which the chrome below borrows too.
        let recording = self.recording_span();
        let Some(Some(renderer)) = self.renderers.get_mut(live.surface.dev_id) else {
            return;
        };

        draw_window(
            &mut self.scene,
            &self.options.theme,
            &self.layout,
            &Chrome {
                panel_title: &self.title,
                transport: TransportChrome {
                    layout: self.bar,
                    view: self.view,
                    meters: self.meters,
                    readout: &self.readout,
                    tempo: &self.tempo_text,
                    signature: &self.signature_text,
                    mode: &self.mode_text,
                    hover: self.hover,
                    marker_sample: self.marker,
                    clip_mode: self.play_mode == crate::document::PlayMode::Clip,
                },
                roll: self.options.document.as_ref().map(|doc| RollChrome {
                    key_map: &self.key_map,
                    layout: self.roll_layout,
                    toolbar: self.roll_bar.clone(),
                    view: self.roll.view,
                    notes: doc.notes(),
                    selection: self.roll.selection(),
                    playhead_tick: doc.playhead_tick(self.view.position_sample),
                    beats_per_bar: doc.beats_per_bar(),
                    tool: self.roll.tool,
                    snap: self.roll.view.snap,
                    lane_property: self.roll.lane_property,
                    ghosts: &self.ghosts,
                    ghost_filter: self.roll.ghosts,
                    key_style: self.key_style,
                    clip_length: doc.clip_length(),
                    marker_tick: doc.playhead_tick(self.marker),
                    loop_range: self.loop_range.map(|(from, to)| {
                        (
                            doc.clip_tick_of_song_tick(from),
                            doc.clip_tick_of_song_tick(to),
                        )
                    }),
                    marquee: self.roll.marquee(),
                    hover: self.hover_control,
                    lane_menu: self.lane_menu.as_ref(),
                    tools_panel: self.tools_panel.as_ref(),
                    tools: &self.tools,
                    slice: self.roll.slice_stroke(),
                    live_keys: self.live_keys,
                }),
                // Exactly one of the two: they are one panel with two tabs
                // (`canvas::tab_strip`), and drawing both would put two lists
                // in the same rectangle.
                rack: self
                    .options
                    .document
                    .as_ref()
                    .filter(|_| self.rack_tab == crate::document::RackTab::Instruments)
                    .map(|_| RackChrome {
                        panel: self.layout.rack,
                        layout: self.rack.clone(),
                        channels: &self.channels,
                        selected: self.selected_channel,
                        hover: self.hover_rack,
                        route_names: &self.route_names,
                        strips: self.route_names.len(),
                        route_menu: self.route_menu.as_ref().map(|(_, menu)| menu),
                        route_menu_open: self.route_menu.as_ref().map(|(index, _)| *index),
                        renaming: match &self.renaming {
                            Some(MenuTarget::Channel(index)) => Some(*index),
                            _ => None,
                        },
                    }),
                prefabs: self
                    .options
                    .document
                    .as_ref()
                    .filter(|_| self.rack_tab == crate::document::RackTab::Prefabs)
                    .map(|_| crate::render::PrefabChrome {
                        panel: self.layout.rack,
                        layout: self.prefab_panel.clone(),
                        prefabs: &self.prefabs,
                        hover: self.hover_prefab,
                        renaming: match &self.renaming {
                            Some(MenuTarget::Prefab(index)) => Some(*index),
                            _ => None,
                        },
                    }),
                browser: self.options.document.as_ref().map(|_| BrowserChrome {
                    panel: self.layout.browser,
                    layout: self.browser.clone(),
                    mode: self.browser_mode,
                    import_kind: self.import_kind,
                    query: &self.query,
                    files: match self.browser_mode {
                        // The Presets tab is the Sounds tab's shape with a
                        // different list in it (§P.8): devices on the left,
                        // their presets on the right. Reusing the shape is
                        // most of why a fifth mode is a variant and not a
                        // panel.
                        BrowserMode::Sounds | BrowserMode::Presets => &self.files,
                        BrowserMode::Projects => &self.projects,
                        BrowserMode::Import => &self.imports,
                        BrowserMode::Settings => &self.settings,
                    },
                    presets: &self.presets,
                    selected_file: self.selected_file,
                    selected_preset: self.selected_preset,
                    searching: self.searching,
                    focus_preset: self.preset_focus,
                    hover: self.hover_browser,
                }),
                timeline: (!self.layout.timeline.frame.is_empty()
                    && self.options.document.is_some())
                .then(|| TimelineChrome {
                    panel: self.layout.timeline,
                    layout: self.timeline_layout,
                    view: self.timeline.view,
                    lanes: &self.lanes,
                    clips: &self.clips,
                    selection: self.timeline.selection(),
                    playhead_tick: self.song_playhead,
                    marker_tick: self.song_marker,
                    beats_per_bar,
                    marquee: self.timeline.marquee(),
                    focused: self.focus == Focus::Timeline,
                    toolbar: self.timeline_bar.clone(),
                    tool: self.timeline.tool(),
                    stretch: self.timeline.stretch(),
                    hover: self.hover_timeline,
                    slice: self.timeline.slice_line(),
                    renaming: match &self.renaming {
                        Some(MenuTarget::Lane(index)) => Some(*index),
                        _ => None,
                    },
                    point_clip: self.timeline.point_clip(),
                    point_selection: self.timeline.point_selection(),
                    loop_range: self.loop_range,
                    recording,
                    can_paste: self
                        .options
                        .document
                        .as_ref()
                        .is_some_and(|doc| doc.clip_clipboard_len() > 0),
                }),
                mixer: (self.tab == EditorTab::Mixer).then(|| MixerChrome {
                    layout: self.mixer.clone(),
                    strips: &self.mixer_strips,
                    peaks: &self.mixer_peaks,
                    hover: self.hover_mixer,
                    active: match self.drag {
                        Drag::Fader(strip) => Some(MixerHit::Fader(strip)),
                        Drag::Pan(strip) => Some(MixerHit::Pan(strip)),
                        _ => None,
                    },
                    selected: self.selected_track,
                    renaming: match &self.renaming {
                        Some(MenuTarget::MixerTrack(index)) => Some(*index),
                        _ => None,
                    },
                    output_label: output_label.clone(),
                    input_label: input_label.clone(),
                    insert_drag: self.insert_drag,
                    output_menu: self.output_menu.as_ref(),
                    send_menu: self.send_menu.as_ref().map(|(_, menu)| menu),
                    route_names: &self.route_names,
                    output: self.track_output,
                }),
                tabs: self.tabs,
                tab: self.tab,
                hover_tab: self.hover_tab,
                browser_title: &self.browser_title,
                labels: &self.labels,
                status: &self.status,
                tooltip: tooltip.as_ref().map(|(text, rect)| (text.as_str(), *rect)),
                // Only the studio's own menus: one opened on a knob belongs
                // to a floating editor's window and is drawn there. See
                // `MenuTarget::editor_window` for why that question is asked
                // in one place rather than listed at both draw calls.
                menu: self
                    .menu
                    .as_ref()
                    .filter(|(target, _)| target.editor_window().is_none())
                    .map(|(_, menu)| menu),
            },
        );

        let surface_texture = match live.surface.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            // A surface going out of date mid-resize, or being occluded, is
            // ordinary. Put the region back and ask again rather than tearing
            // the loop down over it.
            _ => {
                self.tree.invalidate_rect(self.layout.window);
                live.window.request_redraw();
                return;
            }
        };
        if let Err(e) = renderer.render_to_texture(
            &device.device,
            &device.queue,
            &self.scene,
            &live.surface.target_view,
            &RenderParams {
                base_color: self.options.theme.palette.window.to_peniko(),
                width: live.surface.config.width,
                height: live.surface.config.height,
                antialiasing_method: AaConfig::Area,
            },
        ) {
            self.failure = Some(WindowError::Render(RenderError::Render(e.to_string())));
            return;
        }

        let mut encoder = device
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("fontelle-ui present"),
            });
        live.surface.blitter.copy(
            &device.device,
            &mut encoder,
            &live.surface.target_view,
            // The surface's own format, not an sRGB view of it: vello
            // configures the surface with an empty `view_formats`, and builds
            // its blitter for the plain format, so asking for the sRGB
            // variant here is a validation error rather than a colour space.
            &surface_texture
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default()),
        );
        device.queue.submit([encoder.finish()]);
        live.window.pre_present_notify();
        surface_texture.present();
        let _ = device.device.poll(wgpu::PollType::Poll);

        self.frames += 1;
    }

    /// Reads the engine once, folds it into the meters, and dirties only what
    /// moved.
    ///
    /// This is the "state up" half of TDD §2.2, and the place the §16.3
    /// promise is kept: it is called on every pass of the loop, and on a
    /// stopped, silent window it finds nothing changed and marks nothing
    /// dirty, so no frame is issued. The comparison is against the whole
    /// view *and* the meter state, because a meter still falling after the
    /// last note is something moving even though the transport is not.
    fn tick(&mut self) {
        let now = std::time::Instant::now();
        // Clamped: a window that was dragged, minimised, or simply not
        // scheduled for a second must not make the meters jump a second's
        // worth of release in one step.
        let dt = (now - self.last_tick).as_secs_f32().min(0.25);
        self.last_tick = now;

        // A tip that has just fallen due. Nothing else will ask for the frame:
        // the pointer coming to rest is the last event there was, which is the
        // whole reason the dwell exists.
        if self.due_tip().is_some() && self.tip_rect.is_empty() {
            self.tree.invalidate_rect(self.tooltip_region());
        }

        let mut view = match &mut self.options.host {
            Some(host) => host.view(),
            None => TransportView::unavailable(),
        };
        // The count-in, if one is running: the transport rolled a bar early
        // over a click, and the tape starts when the playhead reaches where
        // recording was actually asked for.
        if let Some(target) = self.count_in_until
            && view.recording
            && view.position_sample >= target
        {
            self.count_in_until = None;
            self.take_from = Some(target);
            if let Some(doc) = &mut self.options.document {
                doc.discard_audio_take();
            }
            self.status = "recording".to_string();
            self.tree.invalidate(BROWSER);
        }
        // The bars-and-beats read-out through the document's map, not the
        // engine host's copy of it: the host was handed the map at start-up
        // and a tempo change — or a tempo lane — has moved it since. Only
        // the document holds the current one (INVARIANT 5).
        if let Some(doc) = &self.options.document {
            view.position_beats =
                doc.playhead_song_tick(view.position_sample) as f64 / fontelle_types::PPQN as f64;
        }
        let mut meters = self.meters;
        for (index, meter) in meters.iter_mut().enumerate() {
            meter.update(view.peaks.get(index).copied().unwrap_or(0.0), dt);
        }

        // The mixer's meters, read once a frame and only while the panel that
        // draws them is showing — a level moves every block, and reading it on
        // the studio's revision would rebuild every list in the window sixty
        // times a second. Reading takes the peak, so a frame that is skipped
        // is not a peak that is missed.
        if self.tab == EditorTab::Mixer
            && let Some(doc) = &mut self.options.document
        {
            let peaks = doc.mixer_peaks();
            let quiet = peaks.iter().all(|[l, r]| *l == 0.0 && *r == 0.0);
            if !quiet || !self.mixer_peaks.is_empty() {
                self.mixer_peaks = peaks;
                self.tree.invalidate(PANEL);
            }
        }

        // The EQ's analyser, read once a frame and only while its window is
        // open — the same rule the mixer's meters follow, and for the same
        // reason: it moves every block, and reading it on the studio's
        // revision would rebuild the whole window sixty times a second.
        if self.is_editor_open(EditorKind::Effect) {
            self.tick_spectrum(dt);
        } else if !self.spectrum.is_empty() {
            // The window closed. Nothing is drawing it, and a stale spectrum
            // waiting to be shown at whatever it last was is worse than none.
            self.spectrum.clear();
            self.spectrum_points.clear();
        }

        // The keys a MIDI keyboard is holding, read once a frame for the same
        // reason the meters are: a key goes down between revisions, and the
        // roll's keyboard is the only thing in the window that draws them.
        // Only while the roll is the panel showing — the mixer has no keyboard
        // on it, and a phrase played over it would repaint the panel for
        // nothing.
        let live_keys = match (&self.options.document, self.tab) {
            (Some(doc), EditorTab::Roll) => doc.live_keys(),
            _ => 0,
        };
        if live_keys != self.live_keys {
            self.live_keys = live_keys;
            self.tree.invalidate(PANEL);
        }

        // Whether anything on screen will change without the user doing
        // something: a rolling transport moves the playhead, and a meter above
        // the floor is still falling. A key held on a keyboard counts: while
        // one is down the window watches at frame rate, so the light goes out
        // when the key does rather than up to an engine poll later. Held as an animator on the tree, which
        // is what `arm_deadline` reads — one source of truth for "may the
        // window sleep", rather than two that can disagree.
        let moving = view.playing
            || self.live_keys != 0
            // An open analyser is a moving picture, and it has to keep moving
            // while it falls back to the floor as well as while it is being
            // fed — a spectrum frozen at the last thing that played is a
            // spectrum that lies.
            || !self.spectrum.is_empty()
            || meters
                .iter()
                .any(|m| m.level_db > crate::transport::METER_FLOOR_DB);
        if moving != self.animating {
            if moving {
                self.tree.redraw_mut().begin_animating();
            } else {
                self.tree.redraw_mut().end_animating();
            }
            self.animating = moving;
        }

        if view == self.view && meters == self.meters {
            return;
        }
        // The read-out is re-shaped only when the position it shows actually
        // changed, not once per frame: shaping allocates, and a stopped
        // transport should not be doing it at all.
        if view.position_sample != self.view.position_sample || !self.view.available {
            let beats_per_bar = self.beats_per_bar();
            self.readout = self.text.layout(
                &format_readout(&view, beats_per_bar),
                &self.options.theme.font,
                None,
            );
        }
        self.view = view;
        self.meters = meters;
        self.tree.invalidate(TRANSPORT);

        // The arrangement draws the playhead in song ticks, which only the
        // document can convert to (INVARIANT 5), and redraws only when it
        // actually moved a tick.
        if let Some(doc) = &self.options.document {
            let playhead = doc.playhead_song_tick(view.position_sample);
            let marker = doc.playhead_song_tick(self.marker);
            if playhead != self.song_playhead || marker != self.song_marker {
                self.song_playhead = playhead;
                self.song_marker = marker;
                self.tree.invalidate(TIMELINE);
            }
        }
    }

    /// What a held button is doing, in the cursor's vocabulary.
    ///
    /// `None` while nothing is held, and for the drags whose cursor is better
    /// decided by what is under the pointer.
    fn drag_pointer_shape(&self) -> Option<Pointer> {
        match self.drag {
            Drag::None => None,
            Drag::Roll | Drag::Timeline => Some(Pointer::Grabbing),
            Drag::RollRuler | Drag::BarRuler | Drag::TimelineRuler => Some(Pointer::Grabbing),
            Drag::MenuScroll
            | Drag::LaneGrip
            | Drag::Divider
            | Drag::Knob
            | Drag::FlopKnob
            | Drag::FlopEnvNode
            | Drag::FlopRing
            | Drag::InsertKnob
            | Drag::Lane
            | Drag::SidebarSplit
            | Drag::BrowserSplit => Some(Pointer::ResizeY),
            Drag::BrowserRow(_) => Some(Pointer::Grabbing),
            Drag::SidebarSeam => Some(Pointer::ResizeX),
            Drag::Keys => Some(Pointer::Hand),
            // A fader and the tempo box are both vertical throws; a pan is a
            // horizontal one.
            Drag::Fader(_) | Drag::Tempo => Some(Pointer::ResizeY),
            Drag::Pan(_) | Drag::AudioRow(_) | Drag::FlopWave(_) | Drag::FlopMatrix(_) => {
                Some(Pointer::ResizeX)
            }
            Drag::FlopAssign => Some(Pointer::Grabbing),
            // A response is dragged in both axes at once, like a band handle.
            Drag::FlopResponse(_) => Some(Pointer::Grabbing),
            // A band handle goes wherever the pointer does, in both axes.
            Drag::EqHandle(_) => Some(Pointer::Grabbing),
            Drag::TimelineSelect | Drag::RollSelect => Some(Pointer::ResizeX),
            // A row being carried up or down its chain.
            Drag::InsertRow => Some(Pointer::Grabbing),
            Drag::SendLevel(_) => Some(Pointer::ResizeX),
            Drag::InsertMix(_) => Some(Pointer::ResizeY),
            // A number being dragged up and down, like the tempo box.
            Drag::EqField(_) => Some(Pointer::ResizeY),
        }
    }

    /// Asks the OS for the cursor the thing under the pointer deserves.
    ///
    /// The decision is [`pointer_at`]'s and is tested without a window; what is
    /// left here is the one match onto winit's vocabulary.
    fn update_cursor(&mut self) {
        let (x, y) = self.cursor;
        let empty_notes = fontelle_model::Arena::default();
        let notes = match &self.options.document {
            Some(doc) => doc.notes(),
            None => &empty_notes,
        };
        let scene = PointerScene {
            layout: &self.layout,
            bar: &self.bar,
            rack: &self.rack,
            browser: &self.browser,
            roll: &self.roll_layout,
            roll_view: &self.roll.view,
            roll_toolbar: &self.roll_bar,
            tool: self.roll.tool,
            notes,
            timeline: &self.timeline_layout,
            timeline_bar: &self.timeline_bar,
            timeline_view: &self.timeline.view,
            clips: &self.clips,
            instrument: &self.instrument_layout,
            instrument_view: self.instrument.as_ref(),
            mixer: &self.mixer,
            tabs: &self.tabs,
            tab: self.tab,
            dragging: self.drag_pointer_shape(),
        };
        let wanted = pointer_at(&scene, x, y);
        if wanted == self.pointer {
            return;
        }
        self.pointer = wanted;
        let Some(live) = &self.live else { return };
        // The tools get a cursor of their own, drawn from the same shapes as
        // the toolbar (see `crate::icon`). Everything else takes the
        // desktop's — a resize arrow drawn by hand is a worse resize arrow,
        // and the value of a custom cursor is entirely in the shapes a
        // desktop has no name for.
        if let Some(cursor) = self.tool_cursors.get(&wanted).cloned() {
            live.window
                .set_cursor(winit::window::Cursor::Custom(cursor));
            return;
        }
        live.window.set_cursor(system_cursor(wanted));
    }

    /// Builds the tool cursors, once, when the window comes up.
    ///
    /// Once because rasterising is cheap but not free and the set never
    /// changes, and here because a `CustomCursor` needs an event loop to be
    /// created against. A platform that refuses simply gets no entry, and
    /// `update_cursor` falls back to the desktop's own shapes.
    fn build_cursors(&mut self, event_loop: &ActiveEventLoop) {
        if !self.tool_cursors.is_empty() {
            return;
        }
        for pointer in [
            Pointer::Draw,
            Pointer::Paint,
            Pointer::Erase,
            Pointer::Select,
            Pointer::Cut,
        ] {
            let Some(icon) = pointer.icon() else { continue };
            let pixels = crate::icon::rasterise(icon, CURSOR_SIZE);
            // The hotspot is the top left for a pencil and a brush — the tip
            // is drawn there — and the middle for the eraser and the marquee,
            // which act on what they are over rather than at a point.
            let hotspot = match pointer {
                Pointer::Draw | Pointer::Paint => (2, CURSOR_SIZE as u16 - 3),
                _ => (CURSOR_SIZE as u16 / 2, CURSOR_SIZE as u16 / 2),
            };
            let Ok(source) = winit::window::CustomCursor::from_rgba(
                pixels,
                CURSOR_SIZE as u16,
                CURSOR_SIZE as u16,
                hotspot.0,
                hotspot.1,
            ) else {
                continue;
            };
            self.tool_cursors
                .insert(pointer, event_loop.create_custom_cursor(source));
        }
    }

    /// Recomputes what the pointer is over, dirtying the bar only if it
    /// changed.
    fn update_hover(&mut self) {
        let hover = hit(&self.bar, &self.view, self.cursor.0, self.cursor.1);
        if hover != self.hover {
            self.hover = hover;
            self.tree.invalidate(TRANSPORT);
        }
        let control = toolbar_hit(&self.roll_bar, self.cursor.0, self.cursor.1);
        if control != self.hover_control {
            self.hover_control = control;
            self.tree.invalidate(PANEL);
        }
        let on_arrangement = timeline_toolbar_hit(&self.timeline_bar, self.cursor.0, self.cursor.1);
        if on_arrangement != self.hover_timeline {
            self.hover_timeline = on_arrangement;
            self.tree.invalidate(TIMELINE);
        }
        let (x, y) = self.cursor;
        let over_browser = self.layout.browser.frame.contains(x, y);
        let browser = over_browser
            .then(|| browser_hit(&self.browser, x, y))
            .filter(|hit| *hit != BrowserHit::Nothing);
        if browser != self.hover_browser {
            self.hover_browser = browser;
            self.tree.invalidate(BROWSER);
        }
        let tab = editor_tab_at(&self.tabs, x, y);
        let strip = (self.tab == EditorTab::Mixer)
            .then(|| mixer_hit(&self.mixer, x, y))
            .filter(|hit| *hit != MixerHit::Nothing);
        if tab != self.hover_tab || strip != self.hover_mixer {
            self.hover_tab = tab;
            self.hover_mixer = strip;
            self.tree.invalidate(PANEL);
        }
        // One panel, two lists: only the one that is showing is hit-tested,
        // so a row under the pointer in the hidden list cannot light up.
        let over_rack = self.layout.rack.frame.contains(x, y);
        let showing_rack = self.rack_tab == crate::document::RackTab::Instruments;
        let rack = (over_rack && showing_rack)
            .then(|| rack_hit(&self.rack, x, y))
            .filter(|hit| *hit != RackHit::Nothing);
        let prefab = (over_rack && !showing_rack)
            .then(|| crate::canvas::prefab_hit(&self.prefab_panel, x, y))
            .filter(|hit| *hit != crate::canvas::PrefabHit::Nothing);
        if rack != self.hover_rack || prefab != self.hover_prefab {
            self.hover_rack = rack;
            self.hover_prefab = prefab;
            self.tree.invalidate(RACK);
        }
        self.refresh_tip();
        self.update_cursor();
    }

    /// What the thing under the pointer does, in one line, and when the
    /// pointer arrived on it (see [`crate::tooltip`]).
    ///
    /// Recomputed with the hovers rather than at draw time so the dwell has
    /// something to be measured from.
    fn refresh_tip(&mut self) {
        let tip = self.current_tip();
        if tip == self.hover_tip {
            return;
        }
        // The old box has to be painted over, and the new one is not due yet.
        if !self.tip_rect.is_empty() {
            self.tree.invalidate_rect(self.tip_rect);
            self.tip_rect = crate::layout::Rect::ZERO;
        }
        self.hover_tip = tip;
        self.hover_since = std::time::Instant::now();
    }

    /// Takes the tip down and starts its dwell again.
    fn dismiss_tip(&mut self) {
        if !self.tip_rect.is_empty() {
            self.tree.invalidate_rect(self.tip_rect);
            self.tip_rect = crate::layout::Rect::ZERO;
        }
        self.hover_tip = None;
        self.hover_since = std::time::Instant::now();
    }

    /// The tip for whatever the pointer is over, or `None`.
    ///
    /// **Nothing while a button is held**: a box appearing under the pointer
    /// halfway through a fader drag is in the way of the fader.
    fn current_tip(&self) -> Option<String> {
        if self.drag != Drag::None {
            return None;
        }
        // The bar is above the panels, and the tabs above the panel they head,
        // so the order here is the order `press` reads them in.
        let shortcut = |tip: &'static str, key: Option<&'static str>| match key {
            Some(key) => format!("{tip}  ({key})"),
            None => tip.to_string(),
        };
        if let Some(what) = self.hover
            && let Some(tip) = what.tip()
        {
            return Some(tip.to_string());
        }
        if let Some(tab) = self.hover_tab
            && let Some(tip) = tab.tip()
        {
            return Some(tip.to_string());
        }
        if let Some(control) = self.hover_control
            && let Some(tip) = control.tip()
        {
            return Some(shortcut(tip, control.shortcut()));
        }
        if let Some(control) = self.hover_timeline
            && let Some(tip) = control.tip()
        {
            return Some(shortcut(tip, control.shortcut()));
        }
        if let Some(what) = self.hover_mixer
            && let Some(tip) = what.tip()
        {
            return Some(tip.to_string());
        }
        if let Some(what) = self.hover_browser
            && let Some(tip) = what.tip()
        {
            return Some(tip.to_string());
        }
        if let Some(what) = self.hover_rack
            && let Some(tip) = what.tip()
        {
            return Some(tip.to_string());
        }
        if let Some(what) = self.hover_prefab
            && let Some(tip) = what.tip()
        {
            return Some(tip.to_string());
        }
        None
    }

    /// Generously, where a tip near the pointer could land.
    ///
    /// Used to dirty a region for a tip that has not been laid out yet: the
    /// box's width comes from its own shaped text, and that is not known until
    /// the frame it appears in. A rectangle bigger than the box is a repaint
    /// of a few thousand pixels once per hover; getting it wrong is a tip that
    /// never appears until something else happens to redraw.
    fn tooltip_region(&self) -> crate::layout::Rect {
        let (x, y) = self.cursor;
        let reach = 420.0;
        crate::layout::Rect::new(x - reach, y - 64.0, reach * 2.0, 128.0)
            .intersection(&self.layout.window)
    }

    /// The tip to draw **now** — `None` until the pointer has sat still long
    /// enough.
    fn due_tip(&self) -> Option<&str> {
        let tip = self.hover_tip.as_deref()?;
        (self.hover_since.elapsed() >= crate::tooltip::TOOLTIP_DELAY).then_some(tip)
    }

    /// The browser panel's own heading, carrying the count so the number of
    /// soundfonts is visible without a line of its own.
    fn browser_heading(&self) -> String {
        // **The mode's own heading.** It said "Soundfonts — 61" over the
        // projects list and over the settings, which is the same conflation
        // the folder buttons had: three lists in one panel, and one of them
        // naming all three.
        match self.browser_mode {
            // How many soundfonts the **collection** holds, which does not
            // change as you walk into a folder. Counting the rows in front of
            // you would say "3" inside a folder of three and read as the
            // collection having shrunk — and would count folders as
            // soundfonts besides.
            BrowserMode::Sounds => match self.library_count {
                0 => "Soundfonts".to_string(),
                n => format!("Soundfonts \u{2014} {n}"),
            },
            // How many *devices* have presets, which is what the left-hand
            // list holds — the same rule the soundfont heading follows: count
            // the collection, not the rows in front of you.
            BrowserMode::Presets => match self.files.len() {
                0 => "Presets".to_string(),
                n => format!("Presets \u{2014} {n} devices"),
            },
            BrowserMode::Projects => match self.projects.len() {
                0 => "Projects".to_string(),
                n => format!("Projects \u{2014} {n}"),
            },
            // The count is of what is in front of you rather than of the
            // whole collection, because unlike the bank this list *is* the
            // folder: walking into one is meant to change the number.
            BrowserMode::Import => match self.imports.len() {
                0 => "Import".to_string(),
                n => format!("Import \u{2014} {n}"),
            },
            // No count: a settings list is as long as there are settings, and
            // "Settings — 7" answers a question nobody asked.
            BrowserMode::Settings => "Settings".to_string(),
        }
    }

    /// True once the `run_for` deadline has passed, if there is one.
    fn expired(&self) -> bool {
        self.options
            .run_for
            .is_some_and(|d| self.started.elapsed() >= d)
    }
}

impl ApplicationHandler for WindowApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.live.is_some() {
            return;
        }
        self.started = std::time::Instant::now();
        self.build_cursors(event_loop);

        let attributes = Window::default_attributes()
            .with_title(self.options.title.clone())
            .with_inner_size(winit::dpi::LogicalSize::new(
                self.options.size.0,
                self.options.size.1,
            ));
        let window = match event_loop.create_window(attributes) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                self.failure = Some(WindowError::Window(e.to_string()));
                event_loop.exit();
                return;
            }
        };

        let physical = window.inner_size();
        let surface = match crate::render::block_on(self.context.create_surface(
            window.clone(),
            physical.width.max(1),
            physical.height.max(1),
            // Vsync: the compositor's cadence is the only one worth drawing at,
            // and it is also what stops a redraw storm turning into a busy loop.
            wgpu::PresentMode::AutoVsync,
        )) {
            Ok(s) => s,
            Err(e) => {
                self.failure = Some(WindowError::Render(RenderError::Render(e.to_string())));
                event_loop.exit();
                return;
            }
        };

        // One renderer per device, built once. Shader compilation is most of
        // the cold-start budget (§19 wants under a second), so it must not
        // happen per frame or per resize.
        if self.renderers.len() <= surface.dev_id {
            self.renderers.resize_with(surface.dev_id + 1, || None);
        }
        if self.renderers[surface.dev_id].is_none() {
            match Renderer::new(
                &self.context.devices[surface.dev_id].device,
                RendererOptions {
                    use_cpu: false,
                    // Only the mode we ask for at render time: compiling the
                    // permutations we will never use is pure startup cost.
                    antialiasing_support: vello::AaSupport::area_only(),
                    num_init_threads: None,
                    pipeline_cache: None,
                },
            ) {
                Ok(r) => self.renderers[surface.dev_id] = Some(r),
                Err(e) => {
                    self.failure = Some(WindowError::Render(RenderError::Render(e.to_string())));
                    event_loop.exit();
                    return;
                }
            }
        }

        let scale = window.scale_factor();
        self.live = Some(Live { window, surface });
        self.resize(physical.width, physical.height, scale);
        if let Some(live) = &self.live {
            live.window.request_redraw();
        }
        self.arm_deadline(event_loop);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        // **Which window.** An editor's events are its own, and its close
        // button closes it rather than the studio — which is the single worst
        // thing this could have got wrong.
        if let Some(index) = self.editor_at(id) {
            self.editor_event(index, event);
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::Resized(size) => {
                let Some(live) = &mut self.live else { return };
                let (w, h) = (size.width.max(1), size.height.max(1));
                self.context.resize_surface(&mut live.surface, w, h);
                let scale = live.window.scale_factor();
                self.resize(w, h, scale);
                if let Some(live) = &self.live {
                    live.window.request_redraw();
                }
            }

            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                let Some(live) = &self.live else { return };
                let size = live.window.inner_size();
                self.resize(size.width.max(1), size.height.max(1), scale_factor);
                if let Some(live) = &self.live {
                    live.window.request_redraw();
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                let Some(live) = &self.live else { return };
                // Logical pixels, because that is what the layout is in.
                let scale = live.window.scale_factor();
                self.cursor = ((position.x / scale) as f32, (position.y / scale) as f32);
                self.pointer_window = None;
                self.update_hover();
                self.drag_pointer();
                self.request_redraw_if_dirty();
            }

            // The compositor took the pointer away mid-gesture — an alt-tab, a
            // dialog, a window drag. Without this the drag stays armed and the
            // next pointer move continues it with the button already up, which
            // is a note that follows the mouse around on its own.
            WindowEvent::Focused(false) => {
                self.drag = Drag::None;
                self.end_edge_scroll();
                self.knob = None;
                self.flop_knob = None;
                self.roll.release();
                self.timeline.release();
                self.silence_audition();
                if let Some(doc) = &mut self.options.document {
                    doc.end_gesture();
                }
                self.update_cursor();
                self.tree.invalidate(PANEL);
                self.request_redraw_if_dirty();
            }

            // A file dragged onto the window. The **only** way into Fontelle
            // that does not begin with a folder somebody configured, which is
            // why it is worth having: a file you can see is a file you can
            // drop, and INVARIANT 10 is about what Fontelle goes looking for
            // rather than about what it is handed.
            WindowEvent::DroppedFile(path) => {
                self.drop_file(&path);
                self.request_redraw_if_dirty();
            }

            // While one is over the window, say what will happen to it. A
            // drop that silently does nothing looks like a broken window.
            WindowEvent::HoveredFile(path) => {
                self.status = format!(
                    "Drop to open {}",
                    path.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string())
                );
                self.tree.invalidate(BROWSER);
                self.request_redraw_if_dirty();
            }
            WindowEvent::HoveredFileCancelled => {
                self.status.clear();
                self.tree.invalidate(BROWSER);
                self.request_redraw_if_dirty();
            }

            WindowEvent::CursorLeft { .. } => {
                self.cursor = (f32::MIN, f32::MIN);
                self.update_hover();
                self.request_redraw_if_dirty();
            }

            WindowEvent::MouseInput {
                state: winit::event::ElementState::Pressed,
                button,
                ..
            } => {
                let (x, y) = self.cursor;
                self.press(button, x, y);
                self.update_cursor();
                self.request_redraw_if_dirty();
            }

            WindowEvent::MouseInput {
                state: winit::event::ElementState::Released,
                ..
            } => {
                let was_control = matches!(
                    self.drag,
                    Drag::Knob
                        | Drag::FlopKnob
                        | Drag::FlopWave(_)
                        | Drag::FlopResponse(_)
                        | Drag::FlopEnvNode
                        | Drag::FlopRing
                        | Drag::FlopMatrix(_)
                        | Drag::Fader(_)
                        | Drag::Pan(_)
                        | Drag::Tempo
                        | Drag::EqHandle(_)
                        | Drag::SendLevel(_)
                        | Drag::InsertMix(_)
                );
                // An EQ band drag coalesces into one history entry while it
                // runs; this is what tells the document it has stopped, the
                // same handshake a note drag has.
                if matches!(self.drag, Drag::EqHandle(_))
                    && let Some(doc) = &mut self.options.document
                {
                    doc.end_gesture();
                }
                // A row carried up or down its chain is moved where it is let
                // go, not while it travels: one edit and one undo entry for
                // one drag.
                if matches!(self.drag, Drag::InsertRow) {
                    self.drop_insert_row();
                }
                // A time selection lands where the button comes up, like a
                // marquee: that is when it decides what it caught.
                if matches!(self.drag, Drag::TimelineSelect | Drag::RollSelect) {
                    self.commit_selection();
                }
                // And a row carried out of the soundfont panel is acted on by
                // **where it was let go** — see `Drag::BrowserRow`.
                if let Drag::BrowserRow(row) = self.drag {
                    let (x, y) = self.cursor;
                    self.drop_browser_row(row, x, y);
                }
                self.drag = Drag::None;
                self.end_edge_scroll();
                // The press that shut a menu is spent; the next one opens it
                // again. See `dismissed`.
                self.dismissed = None;
                self.knob = None;
                self.flop_knob = None;
                self.value_drag = None;
                self.pointer_anchor = None;
                if was_control {
                    self.tree.invalidate(PANEL);
                    self.tree.invalidate(TRANSPORT);
                }
                let (x, y) = self.cursor;
                // Where the button came up is what a marquee needs: that is
                // the moment it decides what it caught.
                let grid = self.roll_layout.grid;
                let edits = match &self.options.document {
                    Some(doc) => self.roll.release_over(x, y, grid, doc.notes()),
                    None => {
                        self.roll.release();
                        Vec::new()
                    }
                };
                // The cut tool's whole edit lands here: a line half-drawn is
                // not a cut.
                self.apply_roll_edits(edits);
                let beats = self.beats_per_bar();
                // **Before** the release, because after it the gesture is gone:
                // a marquee and a cut are both drawn by the canvas and known to
                // nothing in the document, so the frame that clears them has to
                // be asked for on their own account. Without this the stroke
                // stays on screen after the button comes up — the same fault as
                // the one that made it invisible during the drag, pointed the
                // other way.
                let overlay = self.timeline.draws_overlay() || self.roll.draws_overlay();
                let edits =
                    self.timeline
                        .release_over(x, y, &self.timeline_layout, &self.clips, beats);
                // The cut tool's whole edit lands here, like the roll's: a
                // line half-drawn is not a cut.
                self.apply_arrange_edits(edits);
                if overlay {
                    self.tree.invalidate(TIMELINE);
                }
                // **Before** the release, and this order is the whole of it: a
                // click's audition starts here, on the way up, and
                // `Auditions::start` clears any release that was scheduled. Ask
                // for the sound first and the release below schedules *its*
                // stop; ask for it after and nothing ever stops it.
                self.sound_roll_request();
                self.stop_audition();
                // One drag, one undo entry (§10.6). Only the caller knows the
                // mouse came up, which is exactly why `History` cannot decide
                // this for itself.
                if let Some(doc) = &mut self.options.document {
                    doc.end_gesture();
                }
                self.tree.invalidate(PANEL);
                self.update_cursor();
                self.request_redraw_if_dirty();
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy) = match delta {
                    winit::event::MouseScrollDelta::LineDelta(x, y) => (x, y),
                    winit::event::MouseScrollDelta::PixelDelta(p) => {
                        (p.x as f32 / 40.0, p.y as f32 / 40.0)
                    }
                };
                self.scroll_roll(dx, dy);
                self.request_redraw_if_dirty();
            }

            WindowEvent::ModifiersChanged(state) => {
                self.modifiers = state.state();
                // §16.5's Alt and Shift apply to whatever gesture is running,
                // so the roll is told rather than asked.
                let modifiers = Modifiers {
                    ctrl: self.modifiers.control_key(),
                    shift: self.modifiers.shift_key(),
                    alt: self.modifiers.alt_key(),
                };
                self.roll.set_modifiers(modifiers);
                // The arrangement wants them too: Shift on a clip's right-hand
                // grip is what turns "make this longer" into "make this loop".
                self.timeline.set_modifiers(modifiers);
            }

            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == winit::event::ElementState::Pressed {
                    self.key(&event);
                    self.request_redraw_if_dirty();
                }
            }

            // The compositor threw our pixels away, or is about to show them
            // again. Either way what we believe is on screen is no longer true.
            WindowEvent::Occluded(false) => {
                self.tree.invalidate_rect(self.layout.window);
                if let Some(live) = &self.live {
                    live.window.request_redraw();
                }
            }

            WindowEvent::RedrawRequested => {
                // The OS can ask for this on its own — after an unminimise, or
                // a compositor restart — and when it does it is asking for the
                // whole surface, because we cannot know what it kept.
                if !self.tree.has_dirty_regions() {
                    self.tree.invalidate_rect(self.layout.window);
                }
                self.draw();
                if self.failure.is_some() {
                    event_loop.exit();
                    return;
                }
                self.arm_deadline(event_loop);
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Whatever a click asked for, now that there is an event loop to
        // create it with. First, so an editor opened by the press that just
        // ran is on screen this pass rather than next.
        self.create_pending_editors(event_loop);
        // The activation helper's own pointer and keyboard hear every input
        // event; handled once a pass so the serial a token carries is the
        // click that just happened, and the queue never grows unread.
        if let Some(activation) = &mut self.activation {
            activation.poll();
        }
        // The graph the audio thread handed back is freed here, on this
        // thread — see `fontelle_engine::GraphPublisher`. Cheap, and it has to
        // happen somewhere that runs whether or not a frame does.
        if let Some(doc) = &mut self.options.document {
            doc.pump();
            // A plugin's own editor has no thread: it repaints on a timer this
            // call fires. See `fontelle_host::gui`.
            self.plugin_editor_open = doc.tick_plugin_editors();
        }
        self.refresh_studio();
        self.settle_audition();
        self.maybe_autosave();
        self.tick();
        self.request_redraw_if_dirty();
        self.arm_deadline(event_loop);
    }
}

impl WindowApp {
    // ------------------------------------------------------------ layout ---

    // ---------------------------------------------- the editor windows ---

    /// One frame of the EQ's analyser: read it, ease towards it, and shape the
    /// outline the renderer draws.
    ///
    /// **Eased, not replaced.** A transform taken sixty times a second and
    /// drawn raw is a flicker rather than a picture; every analyser in every
    /// host smooths it, fast upwards so a transient is visible where it
    /// happened and slow downwards so it is visible for long enough to read.
    fn tick_spectrum(&mut self, dt: f32) {
        let Some((strip, slot)) = self.open_insert else {
            return;
        };
        let Some(doc) = &mut self.options.document else {
            return;
        };
        let bands = doc.spectrum(strip, slot);
        if bands.is_empty() {
            if self.spectrum.is_empty() {
                return;
            }
            // Nothing arriving: fall to the floor rather than holding the last
            // thing that played.
            for band in &mut self.spectrum {
                *band =
                    (*band - SPECTRUM_FALL_DB_PER_S * dt).max(crate::canvas::SPECTRUM_BOTTOM_DB);
            }
            if self
                .spectrum
                .iter()
                .all(|db| *db <= crate::canvas::SPECTRUM_BOTTOM_DB + 0.01)
            {
                self.spectrum.clear();
            }
        } else {
            if self.spectrum.len() != bands.len() {
                self.spectrum = bands.clone();
            }
            let fall = SPECTRUM_FALL_DB_PER_S * dt;
            for (smoothed, fresh) in self.spectrum.iter_mut().zip(bands.iter()) {
                *smoothed = if *fresh >= *smoothed {
                    // Straight to a peak: a transient the eye misses is a
                    // transient the analyser did not show.
                    *fresh
                } else {
                    (*smoothed - fall).max(*fresh)
                };
            }
        }
        self.spectrum_points = crate::canvas::spectrum_points(self.eq_layout.curve, &self.spectrum);
        self.redraw_editor(EditorKind::Effect);
    }

    /// Whether an editor of this kind is open.
    fn is_editor_open(&self, kind: EditorKind) -> bool {
        self.editors.iter().any(|e| e.kind == kind) || self.pending_editors.contains(&kind)
    }

    /// Opens an editor window, or brings the one already open to the front.
    ///
    /// **Asked for, not created.** A window needs an `&ActiveEventLoop`, and
    /// the presses that decide to open one are several calls deep inside a
    /// handler that has it; the queue is drained at the edge of the loop where
    /// it is in hand again. See `pending_editors`.
    fn open_editor(&mut self, kind: EditorKind) {
        // **The plugin's own editor, when it has one.**
        //
        // > *"shouldnt these also be showing the custom plugins own display in
        // > their windows not a auto made one from the parameters."*
        //
        // Asked before the studio's own window is raised or made, so the two
        // are never both up for one plugin. A plugin with no editor of its own
        // — and every LV2 one in this build — answers `false` and gets the
        // panel, which is what the panel is for.
        if self.open_plugins_own_editor(kind) {
            return;
        }
        if self.raise_editor(kind) {
            return;
        }
        if !self.pending_editors.contains(&kind) {
            self.pending_editors.push(kind);
        }
    }

    /// Shows the plugin's **own** editor for whatever `kind` would have been
    /// about, if there is a plugin there and it has one. Whether it went up.
    fn open_plugins_own_editor(&mut self, kind: EditorKind) -> bool {
        let insert = self.open_insert;
        let Some(doc) = &mut self.options.document else {
            return false;
        };
        // **The document's selection, not this window's copy of it.** The
        // press that opens an instrument selects the channel first
        // (`MenuTarget::Channel(_), 0`), and `self.selected_channel` is only
        // refreshed by the next `refresh_studio` — so reading the cache here
        // would open the *previously* selected channel's plugin.
        let selected = doc.selected_channel();
        let opened = match kind {
            EditorKind::Instrument => doc.open_plugin_editor_for_channel(selected),
            EditorKind::Effect => match insert {
                Some((strip, slot)) => doc.open_plugin_editor_for_insert(strip, slot),
                None => false,
            },
            EditorKind::AudioClip => false,
        };
        if opened {
            // Its own window, on the desktop, drawn by the plugin — so there
            // is nothing here to draw and nothing to say beyond where it went.
            self.status = "the plugin's own editor is open".to_string();
            self.tree.invalidate(BROWSER);
        }
        opened
    }

    /// The activation protocol, opened from the studio window the first time
    /// it is asked for. `None` on X11, and `None` for good after one failure:
    /// a display that did not have it a moment ago does not have it now.
    fn activation(&mut self) -> Option<&mut crate::activation::Activation> {
        if !self.activation_tried {
            self.activation_tried = true;
            if let Some(live) = &self.live {
                self.activation = crate::activation::Activation::open(&live.window);
            }
        }
        self.activation.as_mut()
    }

    /// A token the compositor will honour for bringing a window to the front,
    /// asked for on the studio window's behalf — the window the click was in,
    /// which is what makes the request one the compositor grants. `None`
    /// where there is no such protocol.
    fn activation_token(&mut self) -> Option<String> {
        let studio = self.live.as_ref()?.window.clone();
        let token = self.activation()?.token_from(&studio);
        if token.is_none() {
            // Wayland is there and the request went unanswered: worth a line
            // on the terminal, because from the window it is indistinguishable
            // from the compositor refusing, and the two are fixed differently.
            eprintln!("activation: the compositor gave no token for the studio window");
        }
        token
    }

    /// Brings an already-open editor window to the front. Whether there was
    /// one.
    ///
    /// Raising it is the whole of "open" for a window that is behind the one
    /// you are looking at — *"if a plugin or instrument window is already open
    /// clicking on that instrument or plugin in its channel rack or track
    /// effect rack should focus that window again, currently clicking it does
    /// nothing so i have to find where the window is and drag it to the front
    /// manually."*
    ///
    /// **One intention, spelt every way a desktop accepts it.** Under
    /// Wayland the only call the compositor acts on is an xdg-activation
    /// token asked for by the active window and spent on this one — see
    /// [`crate::activation`], and the second report: *"its still not bringing
    /// the windows to the front for me"*, made after the calls below had been
    /// tried alone. `focus_window` is what X11, Windows and macOS take, and
    /// the window level is for a compositor that honours it. On the desktop
    /// that does not need one of them, that one does nothing.
    fn raise_editor(&mut self, kind: EditorKind) -> bool {
        if !self.editors.iter().any(|e| e.kind == kind) {
            return false;
        }
        // **The Wayland answer, and the only one KDE takes.** A token asked
        // for by the studio — the active window — and spent on the editor is
        // a restack and a focus change the compositor performs itself. See
        // `crate::activation` for why every call below this is a no-op there.
        if let Some(token) = self.activation_token()
            && let Some(window) = self
                .editors
                .iter()
                .find(|e| e.kind == kind)
                .map(|e| e.window.clone())
            && let Some(activation) = self.activation()
        {
            activation.activate(token, &window);
        }
        let Some(editor) = self.editors.iter_mut().find(|e| e.kind == kind) else {
            return false;
        };
        // **Un-minimise and un-hide first.** A window in the taskbar is not
        // behind the studio, it is *nowhere*, and every call below is a no-op
        // on one that is not mapped. This is the case that reads most like
        // "clicking it does nothing".
        editor.window.set_minimized(false);
        editor.window.set_visible(true);

        // The plain answer, and what X11, Windows and macOS take.
        editor.window.focus_window();

        // Raised above the others for a moment, for a compositor that
        // honours a window level — winit's Wayland backend does not send
        // this at all, which is why the activation above exists. Dropped back
        // to an ordinary window on its next frame (`draw_editor`), so nothing
        // is left permanently on top.
        editor
            .window
            .set_window_level(winit::window::WindowLevel::AlwaysOnTop);
        editor.pinned = true;

        // Last, and deliberately: on a desktop that granted one of the above
        // this does nothing, and on one that granted none of them it flashes
        // the task bar entry, which at least says where the window went.
        editor
            .window
            .request_user_attention(Some(winit::window::UserAttentionType::Informational));
        editor.window.request_redraw();
        true
    }

    /// Closes it, if it is open. Dropping the `Editor` drops its surface.
    fn close_editor(&mut self, kind: EditorKind) {
        self.pending_editors.retain(|k| *k != kind);
        self.editors.retain(|e| e.kind != kind);
        if self.pointer_window == Some(kind) {
            self.pointer_window = None;
            // A gesture cannot be continued in a window that has gone.
            self.drag = Drag::None;
            self.end_edge_scroll();
            self.knob = None;
        }
    }

    /// Creates whatever `open_editor` has queued. Called from the one place
    /// with an event loop in hand and nothing else to do.
    fn create_pending_editors(&mut self, event_loop: &ActiveEventLoop) {
        while let Some(kind) = self.pending_editors.pop() {
            if self.editors.iter().any(|e| e.kind == kind) {
                continue;
            }
            if let Err(e) = self.create_editor(event_loop, kind) {
                // A window that would not open is worth saying so about, and
                // is not worth tearing the studio down over.
                self.status = e;
                self.tree.invalidate(BROWSER);
            }
        }
    }

    fn create_editor(
        &mut self,
        event_loop: &ActiveEventLoop,
        kind: EditorKind,
    ) -> Result<(), String> {
        // Flopsynth's window is a different shape from the knob grid's, and
        // which one opens is decided by what is on the channel rather than by
        // the editor's kind — see `layout::FLOPSYNTH_SIZE`.
        let flopsynth = kind == EditorKind::Instrument && self.flopsynth.is_some();
        let (w, h) = if flopsynth {
            crate::layout::FLOPSYNTH_SIZE
        } else {
            kind.default_size()
        };
        let (min_w, min_h) = if flopsynth {
            crate::layout::FLOPSYNTH_MINIMUM
        } else {
            kind.minimum_size()
        };
        let mut attributes = Window::default_attributes()
            .with_title(self.editor_title(kind))
            .with_inner_size(winit::dpi::LogicalSize::new(w, h))
            .with_min_inner_size(winit::dpi::LogicalSize::new(min_w, min_h));
        // **A token, taken now, while the studio is still the window the
        // user is looking at**, in the form winit and the protocol both
        // understand for a window being made. One is enough: a compositor
        // keeps a token spent on a window that has not painted yet and uses
        // it when it does (KWin's `Window::setActivationToken`), and asking
        // for a second would only replace this one as the current token and
        // leave it to fail. See `crate::activation`.
        if let Some(token) = self.activation_token() {
            use winit::platform::startup_notify::WindowAttributesExtStartupNotify;
            attributes =
                attributes.with_activation_token(winit::window::ActivationToken::from_raw(token));
        }
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|e| format!("that editor could not be opened: {e}"))?,
        );

        let physical = window.inner_size();
        let surface = crate::render::block_on(self.context.create_surface(
            window.clone(),
            physical.width.max(1),
            physical.height.max(1),
            wgpu::PresentMode::AutoVsync,
        ))
        .map_err(|e| format!("that editor could not be drawn: {e}"))?;

        // The same per-device renderer the main window uses, built once. It
        // exists already unless this is somehow the first surface, which is
        // why the branch is here rather than an assumption.
        if self.renderers.len() <= surface.dev_id {
            self.renderers.resize_with(surface.dev_id + 1, || None);
        }
        if self.renderers[surface.dev_id].is_none() {
            let device = &self.context.devices[surface.dev_id].device;
            match Renderer::new(
                device,
                RendererOptions {
                    use_cpu: false,
                    antialiasing_support: vello::AaSupport::area_only(),
                    num_init_threads: None,
                    pipeline_cache: None,
                },
            ) {
                Ok(r) => self.renderers[surface.dev_id] = Some(r),
                Err(e) => return Err(format!("that editor could not be drawn: {e}")),
            }
        }

        let title = self.editor_title(kind);
        let title = self.text.layout(&title, &self.options.theme.font, None);
        let scale = window.scale_factor() as f32;
        let panel = editor_window_layout(
            physical.width as f32 / scale,
            physical.height as f32 / scale,
            &self.options.theme.metrics,
        );
        self.editors.push(Editor {
            kind,
            window,
            surface,
            panel,
            title,
            scene: Scene::new(),
            pinned: false,
        });
        self.relayout_editors();
        if let Some(editor) = self.editors.last() {
            editor.window.request_redraw();
        }
        Ok(())
    }

    /// What an editor window's title bar and header say.
    ///
    /// The name of the *thing being edited* after the kind, because three
    /// windows called "Effect" is three windows you have to click to tell
    /// apart — which is exactly the complaint a tab strip had.
    fn editor_title(&self, kind: EditorKind) -> String {
        match kind {
            // Flopsynth's window is named for what it is: "Flopsynth — Flute"
            // rather than "Instrument — Flute", because the window is the
            // synthesiser's and the channel's name is already the preset's.
            EditorKind::Instrument if self.flopsynth.is_some() => {
                match self.channels.get(self.selected_channel) {
                    Some(channel) => format!("Flopsynth \u{2014} {}", channel.name),
                    None => "Flopsynth".to_string(),
                }
            }
            EditorKind::Instrument => match self.channels.get(self.selected_channel) {
                Some(channel) => format!("{} \u{2014} {}", kind.title(), channel.name),
                None => kind.title().to_string(),
            },
            EditorKind::Effect => self.effect_title(),
            EditorKind::AudioClip => match &self.audio_clip {
                Some(open) => format!("{} \u{2014} {}", kind.title(), open.name),
                None => kind.title().to_string(),
            },
        }
    }

    /// The panel geometry inside each open editor window.
    ///
    /// The layouts stay on `WindowApp` rather than inside `Editor` because
    /// every gesture that reads them — `press_instrument`, `press_effect` —
    /// already does, and there is at most one window of each kind for the
    /// honest reason [`EditorKind`] gives: the host answers one instrument
    /// and one insert.
    fn relayout_editors(&mut self) {
        let m = self.options.theme.metrics;
        let bodies: Vec<(EditorKind, crate::layout::Rect)> = self
            .editors
            .iter()
            .map(|e| (e.kind, e.panel.body))
            .collect();
        for (kind, body) in bodies {
            match kind {
                EditorKind::Instrument => {
                    // Flopsynth's window and the knob grid are laid out from
                    // the same body; only one of them is drawn, and the host
                    // decides which by whether it offered a Flopsynth view.
                    self.flopsynth_layout = match &self.flopsynth {
                        Some(view) => crate::canvas::flopsynth_layout(body, &m, view),
                        None => crate::canvas::FlopsynthLayout {
                            body,
                            ..Default::default()
                        },
                    };
                    self.instrument_layout = match &self.instrument {
                        Some(view) => instrument_layout(body, &m, view),
                        None => InstrumentLayout {
                            name: crate::layout::Rect::ZERO,
                            body,
                            keys: Vec::new(),
                            headings: Vec::new(),
                            cells: Vec::new(),
                            content_height: 0.0,
                        },
                    };
                }
                EditorKind::Effect => {
                    // The other kind of effect window: a grid of knobs, laid
                    // out by the same function the instrument panel uses.
                    self.insert_layout = match &self.insert_view {
                        Some(view) => instrument_layout(body, &m, view),
                        None => InstrumentLayout {
                            name: crate::layout::Rect::ZERO,
                            body,
                            keys: Vec::new(),
                            headings: Vec::new(),
                            cells: Vec::new(),
                            content_height: 0.0,
                        },
                    };
                    let config = self.eq.unwrap_or_default();
                    self.eq_layout = crate::canvas::eq_layout_for(body, &m, &config, self.eq_band);
                    self.eq_curve = crate::canvas::eq_curve_points(&self.eq_layout, &config);
                    // And the band in hand, drawn behind the sum so a cut can
                    // be seen against the shape it is being made in.
                    self.eq_band_curve =
                        crate::canvas::eq_band_curve_points(&self.eq_layout, &config, self.eq_band);
                }
                EditorKind::AudioClip => {
                    self.audio_layout = crate::canvas::audio_editor_layout(
                        body,
                        &m,
                        crate::canvas::AUDIO_ROWS.len(),
                    );
                }
            }
        }
    }

    /// Retitles every open editor and asks it to redraw. What the main window
    /// calls when the studio has moved underneath them.
    fn refresh_editors(&mut self) {
        let kinds: Vec<EditorKind> = self.editors.iter().map(|e| e.kind).collect();
        for kind in kinds {
            let caption = self.editor_title(kind);
            let title = self.text.layout(&caption, &self.options.theme.font, None);
            if let Some(editor) = self.editors.iter_mut().find(|e| e.kind == kind) {
                editor.title = title;
                editor.window.set_title(&caption);
                editor.window.request_redraw();
            }
        }
        self.relayout_editors();
    }

    /// Which open editor a window id belongs to.
    fn editor_at(&self, id: WindowId) -> Option<usize> {
        self.editors.iter().position(|e| e.window.id() == id)
    }

    /// One editor window's events. The main window's are `window_event`'s own.
    fn editor_event(&mut self, index: usize, event: WindowEvent) {
        let Some(editor) = self.editors.get(index) else {
            return;
        };
        let kind = editor.kind;
        match event {
            // **Closes that window, not the studio.** A child window's close
            // button meaning "quit Fontelle" is the single worst thing this
            // change could have got wrong.
            WindowEvent::CloseRequested => self.close_editor(kind),

            WindowEvent::Resized(size) => {
                let (w, h) = (size.width.max(1), size.height.max(1));
                let scale = editor.window.scale_factor() as f32;
                let Some(editor) = self.editors.get_mut(index) else {
                    return;
                };
                self.context.resize_surface(&mut editor.surface, w, h);
                editor.panel = editor_window_layout(
                    w as f32 / scale,
                    h as f32 / scale,
                    &self.options.theme.metrics,
                );
                editor.window.request_redraw();
                self.relayout_editors();
            }

            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                let size = editor.window.inner_size();
                let Some(editor) = self.editors.get_mut(index) else {
                    return;
                };
                editor.panel = editor_window_layout(
                    size.width as f32 / scale_factor as f32,
                    size.height as f32 / scale_factor as f32,
                    &self.options.theme.metrics,
                );
                editor.window.request_redraw();
                self.relayout_editors();
            }

            WindowEvent::CursorMoved { position, .. } => {
                let scale = editor.window.scale_factor();
                self.cursor = ((position.x / scale) as f32, (position.y / scale) as f32);
                self.pointer_window = Some(kind);
                self.update_editor_hover(kind);
                self.update_editor_cursor(kind);
                // The same drag machinery the docked panels used: a knob, an
                // EQ handle and an automation point are dragged against the
                // layouts `relayout_editors` put in this window.
                self.drag_pointer();
                self.redraw_editor(kind);
            }

            // A file dragged onto the window. The **only** way into Fontelle
            // that does not begin with a folder somebody configured, which is
            // why it is worth having: a file you can see is a file you can
            // drop, and INVARIANT 10 is about what Fontelle goes looking for
            // rather than about what it is handed.
            WindowEvent::DroppedFile(path) => {
                self.drop_file(&path);
                self.request_redraw_if_dirty();
            }

            // While one is over the window, say what will happen to it. A
            // drop that silently does nothing looks like a broken window.
            WindowEvent::HoveredFile(path) => {
                self.status = format!(
                    "Drop to open {}",
                    path.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string())
                );
                self.tree.invalidate(BROWSER);
                self.request_redraw_if_dirty();
            }
            WindowEvent::HoveredFileCancelled => {
                self.status.clear();
                self.tree.invalidate(BROWSER);
                self.request_redraw_if_dirty();
            }

            WindowEvent::CursorLeft { .. } => {
                if self.pointer_window == Some(kind) {
                    self.pointer_window = None;
                }
                self.cursor = (f32::MIN, f32::MIN);
                self.update_editor_hover(kind);
                self.redraw_editor(kind);
            }

            // The compositor took the pointer away mid-gesture. The same
            // answer the main window gives, for the same reason.
            WindowEvent::Focused(false) => {
                self.drag = Drag::None;
                self.end_edge_scroll();
                self.knob = None;
                self.flop_knob = None;
                if let Some(doc) = &mut self.options.document {
                    doc.end_gesture();
                }
                self.redraw_editor(kind);
            }

            WindowEvent::MouseInput {
                state: winit::event::ElementState::Pressed,
                button,
                ..
            } => {
                let (x, y) = self.cursor;
                let button = match button {
                    winit::event::MouseButton::Right => MouseButton::Right,
                    _ => MouseButton::Left,
                };
                // A menu opened on a knob is above this window's own panel,
                // and a press that chose an entry is spent.
                if self.menu.is_some() && self.press_menu(x, y) {
                    self.redraw_editor(kind);
                    return;
                }
                self.press_editor(kind, button, x, y);
                self.redraw_editor(kind);
            }

            WindowEvent::MouseInput {
                state: winit::event::ElementState::Released,
                ..
            } => {
                self.release_controls();
                self.redraw_editor(kind);
            }

            WindowEvent::MouseWheel { delta, .. } => {
                // The wheel is the coarse way at a knob, and the only way at
                // one on a trackpad with no room to drag. Nothing else in an
                // editor window scrolls: these panels are laid out to fit.
                let steps = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, lines) => lines,
                    winit::event::MouseScrollDelta::PixelDelta(position) => {
                        position.y as f32 / 40.0
                    }
                };
                if steps != 0.0 {
                    let (x, y) = self.cursor;
                    // A menu opened from *this* window is drawn in front of
                    // its panel, so the wheel is the menu's before it is a
                    // knob's — the same rule `scroll_roll` follows for the
                    // studio's own menus, and the reason the preset list was
                    // a hundred and twenty-eight presets you could only ever
                    // see the top section of.
                    if self.wheel_menu(x, y, steps) {
                        self.redraw_editor(kind);
                        return;
                    }
                    match kind {
                        EditorKind::Effect => self.wheel_effect(x, y, steps),
                        // *"the pitch changing should be a knob"* — a track
                        // now, and the wheel over it is the fine way: one
                        // semitone, one decibel, one rung of the fade ladder.
                        EditorKind::AudioClip => self.wheel_audio_editor(x, y, steps),
                        EditorKind::Instrument => self.wheel_flopsynth(x, y, steps),
                    }
                }
                self.redraw_editor(kind);
            }

            // An editor window has its own modifiers to keep: it is a separate
            // OS window, so the studio's `ModifiersChanged` never reaches it,
            // and without this a Ctrl+Z pressed in here would arrive with no
            // Ctrl on it.
            WindowEvent::ModifiersChanged(state) => self.modifiers = state.state(),

            WindowEvent::KeyboardInput { event, .. }
                if event.state == winit::event::ElementState::Pressed =>
            {
                self.editor_key(kind, &event);
            }

            WindowEvent::RedrawRequested => self.draw_editor(index),

            _ => {}
        }
    }

    /// A key pressed while an editor window has the keyboard.
    ///
    /// Three layers, nearest first: **Escape** closes the window, which is what
    /// every floating editor in every host does; then whatever this particular
    /// editor makes of the key; then the keys that mean the same thing
    /// everywhere — see [`global_key`](Self::global_key), which is why the
    /// transport and the history work in here now.
    ///
    /// What is deliberately *not* here is the studio's canvas keys. Delete
    /// means "the selected band" in an EQ window and "the selected notes" in
    /// the roll, and a window that answered both would be answering the wrong
    /// one half the time.
    fn editor_key(&mut self, kind: EditorKind, event: &winit::event::KeyEvent) {
        use winit::keyboard::{Key, NamedKey};

        // A menu open over this window has the keyboard first — the preset
        // drop-down filters as you type, and Escape shuts the menu rather
        // than the window it is over. Without this, Escape on an open
        // drop-down took the whole editor with it.
        if self.menu.is_some() && self.menu_filter_key(event) {
            self.redraw_editor(kind);
            return;
        }
        if event.logical_key == Key::Named(NamedKey::Escape)
            && let Some((target, menu)) = self.menu.as_ref()
            && target.editor_window() == Some(kind)
        {
            let frame = menu.frame;
            self.menu = None;
            self.tree.invalidate_rect(frame);
            self.redraw_editor(kind);
            return;
        }
        // The editor's own keys next, because one of them can be Escape:
        // the Presets page's search clears on it before the window closes on
        // it, the way the browser's search box does.
        if self.editor_own_key(kind, event) {
            self.redraw_editor(kind);
            return;
        }
        if event.logical_key == Key::Named(NamedKey::Escape) {
            self.close_editor(kind);
            return;
        }
        if self.global_key(event) {
            // The studio's own panels moved — a transport button lit, a
            // history entry came back — so it redraws too.
            self.redraw_editor(kind);
            self.request_redraw_if_dirty();
        }
    }

    /// What one editor makes of a key of its own. Whether it took it.
    fn editor_own_key(&mut self, kind: EditorKind, event: &winit::event::KeyEvent) -> bool {
        use winit::keyboard::{Key, NamedKey};

        match kind {
            EditorKind::Effect => match &event.logical_key {
                // *"i cant easily delete bands i didnt mean to make in the eq
                // plugin i wanna just be able to press delete with it
                // selected."* The band chip's Ctrl-click does the same thing
                // and is a chip you have to find first.
                Key::Named(NamedKey::Delete) | Key::Named(NamedKey::Backspace) => {
                    self.remove_eq_band(self.eq_band);
                    true
                }
                _ => false,
            },
            // Flopsynth's Presets page has a search box, and while it is
            // showing the keyboard is its.
            EditorKind::Instrument
                if self.flopsynth.is_some()
                    && self.flop_page == crate::canvas::FlopsynthPage::Presets =>
            {
                self.flop_search_key(event)
            }
            EditorKind::Instrument | EditorKind::AudioClip => false,
        }
    }

    /// The cursor an editor window's own contents deserve.
    ///
    /// [`pointer_at`] answers this for the main window and cannot answer it
    /// here: it is handed a `PointerScene` describing that window's panels,
    /// and these are different surfaces entirely.
    fn update_editor_cursor(&mut self, kind: EditorKind) {
        let (x, y) = self.cursor;
        let wanted = match self.drag_pointer_shape() {
            Some(shape) => shape,
            None => match kind {
                EditorKind::Instrument => crate::pointer::instrument_pointer(
                    &self.instrument_layout,
                    self.instrument.as_ref(),
                    x,
                    y,
                ),
                // A band handle and a curve point are both picked up and
                // carried, in both axes.
                EditorKind::Effect if self.eq.is_none() => crate::pointer::instrument_pointer(
                    &self.insert_layout,
                    self.insert_view.as_ref(),
                    x,
                    y,
                ),
                EditorKind::Effect => {
                    use crate::canvas::EqHit;
                    match crate::canvas::eq_hit(&self.eq_layout, x, y) {
                        EqHit::Handle(_) => Pointer::Grab,
                        // The numbers are dragged up and down; the choosers
                        // and the chips are pressed.
                        EqHit::Field(field) if field.is_dragged() => Pointer::ResizeY,
                        EqHit::Field(_) | EqHit::Band(_) => Pointer::Hand,
                        // A press on empty curve puts a band there, which is a
                        // thing you do rather than a thing you aim at.
                        EqHit::Curve => Pointer::Hand,
                        EqHit::Nothing => Pointer::Default,
                    }
                }
                // The cursor says which of the three a row is before the
                // press does: a track is dragged sideways, a switch and a
                // drop-down are pressed.
                EditorKind::AudioClip => {
                    use crate::canvas::AudioControl;
                    match crate::canvas::audio_editor_hit(&self.audio_layout, x, y)
                        .map(crate::canvas::audio_row_control)
                    {
                        None | Some(AudioControl::None) => Pointer::Default,
                        Some(AudioControl::Slider) => Pointer::ResizeX,
                        Some(AudioControl::Switch | AudioControl::Choice) => Pointer::Hand,
                    }
                }
            },
        };
        if self.pointer == wanted {
            return;
        }
        self.pointer = wanted;
        let Some(editor) = self.editors.iter().find(|e| e.kind == kind) else {
            return;
        };
        match self.tool_cursors.get(&wanted) {
            Some(custom) => editor
                .window
                .set_cursor(winit::window::Cursor::Custom(custom.clone())),
            None => editor.window.set_cursor(system_cursor(wanted)),
        }
    }

    /// A press inside an editor window, routed by which editor it is.
    fn press_editor(&mut self, kind: EditorKind, button: MouseButton, x: f32, y: f32) {
        // The bar first, whatever the window is: it is in the *header*, above
        // the panel, so nothing else can be under the pointer there — and a
        // press that reached the panel's hit test instead would land on
        // whichever control happens to be nearest the top.
        if button == MouseButton::Left && self.press_preset_bar(kind, x, y) {
            return;
        }
        match kind {
            EditorKind::Instrument if self.flopsynth.is_some() => match button {
                MouseButton::Left => self.press_flopsynth(x, y),
                // The same rule §12.4 gives every other knob in the program:
                // right-click makes an automation lane for it.
                MouseButton::Right => self.press_flopsynth_menu(x, y),
            },
            EditorKind::Instrument => match button {
                MouseButton::Left => self.press_instrument(x, y),
                // *"i want to be able to right click on a knob and select
                // create automation clip."* §12.4's rule, on the panel that
                // did not have it.
                MouseButton::Right => self.press_instrument_menu(x, y),
            },
            // Which of the two effect windows this is — the EQ's curve, or the
            // grid of knobs every other effect gets.
            EditorKind::Effect if self.eq.is_none() => self.press_insert_panel(button, x, y),
            EditorKind::Effect => self.press_effect_editor(button, x, y),
            EditorKind::AudioClip => self.press_audio_editor(button, x, y),
        }
    }

    /// What the pointer is over inside an editor window.
    fn update_editor_hover(&mut self, kind: EditorKind) {
        let (x, y) = self.cursor;
        self.hover_preset_bar = Self::preset_slot(kind)
            .zip(self.preset_bar_geometry(kind))
            .and_then(|(slot, layout)| {
                crate::canvas::preset_bar_hit(&layout, &self.preset_view[slot], x, y)
                    .map(|hit| (slot, hit))
            });
        match kind {
            EditorKind::Instrument if self.flopsynth.is_some() => {
                self.hover_card = match crate::canvas::flopsynth_hit(&self.flopsynth_layout, x, y) {
                    Some(crate::canvas::FlopsynthHit::Control { card, param }) => {
                        Some((card, param))
                    }
                    _ => None,
                };
            }
            EditorKind::Instrument => {
                self.hover_param = instrument_hit(&self.instrument_layout, x, y);
            }
            EditorKind::Effect if self.eq.is_none() => {
                self.hover_param = instrument_hit(&self.insert_layout, x, y);
                self.hover_key = crate::canvas::instrument_key_hit(&self.insert_layout, x, y);
            }
            EditorKind::Effect => {
                let hit = crate::canvas::eq_hit(&self.eq_layout, x, y);
                self.hover_band = match hit {
                    crate::canvas::EqHit::Handle(band) | crate::canvas::EqHit::Band(band) => {
                        Some(band)
                    }
                    _ => None,
                };
                self.hover_eq_field = match hit {
                    crate::canvas::EqHit::Field(field) => Some(field),
                    _ => None,
                };
            }
            EditorKind::AudioClip => {
                self.hover_audio = crate::canvas::audio_editor_hit(&self.audio_layout, x, y);
            }
        }
    }

    /// A press on the audio clip editor, by what the row **is**.
    ///
    /// > *"a lot of options that could be knobs or sliders or dropdowns for
    /// > some reason are instead shown as buttons you click to toggle through
    /// > a list of options in order iteratively. this is really annoying."*
    ///
    /// Three gestures rather than one, because there are three kinds of value
    /// on this panel and they do not behave alike (see
    /// [`crate::canvas::AudioControl`]): a track is pressed and dragged, a
    /// switch is clicked, and a list drops down. The wheel still steps
    /// whatever the pointer is over, which is what a wheel over a control
    /// should do and is how a value is moved by exactly one of its own units.
    fn press_audio_editor(&mut self, button: MouseButton, x: f32, y: f32) {
        use crate::canvas::AudioControl;
        if button != MouseButton::Left {
            return;
        }
        let Some(field) = crate::canvas::audio_editor_hit(&self.audio_layout, x, y) else {
            return;
        };
        match crate::canvas::audio_row_control(field) {
            AudioControl::None => {}
            AudioControl::Slider => {
                // The press sets the value and the drag carries on from
                // there, the way the mixer's fader does.
                self.drag = Drag::AudioRow(field);
                self.drag_audio_row(field, x);
            }
            AudioControl::Switch => {
                let Some(open) = &mut self.audio_clip else {
                    return;
                };
                crate::canvas::toggle_audio_row(&mut open.data, field);
                self.write_audio_clip();
            }
            AudioControl::Choice => self.open_audio_menu(field),
        }
    }

    /// One step of a track on the audio clip editor.
    fn drag_audio_row(&mut self, field: crate::canvas::AudioField, x: f32) {
        let Some(row) = self
            .audio_layout
            .rows
            .iter()
            .find(|(row, _)| *row == field)
            .map(|(_, rect)| *rect)
        else {
            return;
        };
        let t = crate::canvas::audio_slider_at(row, &self.options.theme.metrics, x);
        let Some(open) = &mut self.audio_clip else {
            return;
        };
        crate::canvas::set_audio_row_fraction(&mut open.data, field, t);
        self.write_audio_clip();
    }

    /// The wheel over a row: one step of whatever that row is measured in.
    fn wheel_audio_editor(&mut self, x: f32, y: f32, steps: f32) {
        let Some(field) = crate::canvas::audio_editor_hit(&self.audio_layout, x, y) else {
            return;
        };
        let direction = if steps > 0.0 { 1 } else { -1 };
        let Some(open) = &mut self.audio_clip else {
            return;
        };
        if field == crate::canvas::AudioField::Route {
            let tracks: Vec<Option<fontelle_types::MixerTrackId>> =
                self.clip_routes.iter().map(|(id, _)| *id).collect();
            crate::canvas::nudge_route(&mut open.data, direction, &tracks);
        } else {
            crate::canvas::nudge_audio_row(&mut open.data, field, direction, open.sample_rate);
        }
        self.write_audio_clip();
    }

    /// Drops `field`'s list under its row.
    ///
    /// Anchored to the row rather than to the pointer, because that is what a
    /// drop-down is: the list appears where the value was, not where the hand
    /// happened to be.
    fn open_audio_menu(&mut self, field: crate::canvas::AudioField) {
        let anchor = self
            .audio_layout
            .rows
            .iter()
            .find(|(row, _)| *row == field)
            .map(|(_, rect)| {
                crate::canvas::audio_row_control_rect(*rect, &self.options.theme.metrics)
            })
            .unwrap_or(crate::layout::Rect::ZERO);
        // The window's own body, so a list too long for it is scrolled to fit
        // rather than drawn off the bottom. No window, no menu — there is
        // nothing for it to hang under.
        let Some(bounds) = self
            .editors
            .iter()
            .find(|e| e.kind == EditorKind::AudioClip)
            .map(|editor| editor.panel.body)
        else {
            return;
        };
        self.open_menu(
            MenuTarget::AudioRow(field),
            anchor.x,
            anchor.bottom(),
            bounds,
        );
        self.redraw_editor(EditorKind::AudioClip);
    }

    /// Puts the open clip's properties back on the document, and repaints the
    /// two places that draw them.
    ///
    /// The arrangement's block draws the fades and the trim, so it has to be
    /// re-read: the picture in the window and the picture on the timeline are
    /// the same picture.
    fn write_audio_clip(&mut self) {
        let Some(open) = &self.audio_clip else { return };
        let (id, data) = (open.id, open.data.clone());
        if let Some(doc) = &mut self.options.document {
            doc.set_audio_clip(id, data);
        }
        self.refresh_studio();
        self.redraw_editor(EditorKind::AudioClip);
        self.tree.invalidate(TIMELINE);
    }

    /// Opens the audio clip editor on `clip`, or retargets the one that is
    /// already open.
    fn open_audio_editor(&mut self, clip: fontelle_types::ClipId) {
        let Some(doc) = &self.options.document else {
            return;
        };
        let Some(data) = doc.audio_clip(clip) else {
            return;
        };
        let sample_rate = doc.audio_clip_rate(clip);
        let name = self
            .clips
            .iter()
            .find(|info| info.id == clip)
            .map(|info| info.name.clone())
            .unwrap_or_else(|| "Audio".to_string());
        self.audio_clip = Some(OpenAudioClip {
            id: clip,
            name,
            data,
            sample_rate,
        });
        self.open_editor(EditorKind::AudioClip);
    }

    fn redraw_editor(&mut self, kind: EditorKind) {
        if let Some(editor) = self.editors.iter().find(|e| e.kind == kind) {
            editor.window.request_redraw();
        }
    }

    /// One editor window's frame.
    ///
    /// Drawn whole rather than by dirty region: a panel of knobs is a few
    /// hundred paths, the window is only up while somebody is using it, and a
    /// second invalidation tree per window would be state to keep in sync for
    /// no measurable gain. §16.3's promise is about the *idle* window, and an
    /// editor that nobody is touching receives no events and draws no frames.
    fn draw_editor(&mut self, index: usize) {
        self.shape_labels();
        // A window held above the others by `raise_editor` has now been drawn
        // there, so it goes back to being an ordinary window. Held for a frame
        // rather than dropped in the same breath because a compositor that
        // batches the two calls would see no change at all, and then the raise
        // would be the no-op it is meant to replace.
        if let Some(editor) = self.editors.get_mut(index)
            && editor.pinned
        {
            editor.pinned = false;
            editor
                .window
                .set_window_level(winit::window::WindowLevel::Normal);
        }
        let Some(editor) = self.editors.get(index) else {
            return;
        };
        let kind = editor.kind;
        let chrome = match kind {
            // Flopsynth draws its own window, and it is checked first: a
            // channel that is one has both views, and the picture of the
            // signal path is the one worth showing.
            EditorKind::Instrument if self.flopsynth.is_some() => {
                let Some(view) = self.flopsynth.as_ref() else {
                    return;
                };
                EditorWindowChrome::Flopsynth(crate::render::FlopsynthChrome {
                    layout: self.flopsynth_layout.clone(),
                    view,
                    hover: self.hover_card,
                    active: self.flop_knob.map(|(which, _, _)| which),
                    modulated: self.flop_modulated.clone(),
                    assigning: self.flop_assign,
                    destinations: self.flop_destinations.clone(),
                    about: self.flop_about(),
                    hover_at: self.cursor,
                })
            }
            EditorKind::Instrument => {
                EditorWindowChrome::Instrument(self.instrument.as_ref().map(|view| {
                    InstrumentChrome {
                        layout: self.instrument_layout.clone(),
                        view,
                        hover: self.hover_param,
                        active: self.knob.map(|(which, _, _)| which),
                        // It does now: the drum machine's kits are chips on this
                        // row, and a chip that does not light up under the pointer
                        // is a chip nobody is sure is a button.
                        hover_preset: self.hover_preset,
                        // Still none. A detector key belongs to an *effect* — see
                        // `InstrumentView::keys`, which is empty on every
                        // instrument panel.
                        hover_key: None,
                    }
                }))
            }
            EditorKind::Effect if self.eq.is_none() => {
                let Some(view) = self.insert_view.as_ref() else {
                    return;
                };
                EditorWindowChrome::Insert(InstrumentChrome {
                    layout: self.insert_layout.clone(),
                    view,
                    hover: self.hover_param,
                    active: self.insert_knob.map(|(which, _, _)| which),
                    hover_preset: self.hover_preset,
                    hover_key: self.hover_key,
                })
            }
            EditorKind::Effect => {
                let Some(config) = self.eq else { return };
                EditorWindowChrome::Effect(crate::render::EffectChrome {
                    layout: self.eq_layout.clone(),
                    config,
                    spectrum: self.spectrum_points.clone(),
                    curve: self.eq_curve.clone(),
                    band_curve: self.eq_band_curve.clone(),
                    hover: self.hover_band,
                    hover_field: self.hover_eq_field,
                    active: match self.drag {
                        Drag::EqHandle(band) => Some(band),
                        _ => None,
                    },
                    title: self.effect_title(),
                    bypassed: self.open_insert_bypassed(),
                })
            }
            EditorKind::AudioClip => {
                let Some(open) = self.audio_clip.as_ref() else {
                    return;
                };
                // The waveform is read off the **arrangement's** own list of
                // blocks here rather than cached beside the numbers, so the
                // strip in the window and the block on the timeline are
                // literally one picture. Cached, it went stale the first time
                // a fade was stepped: the block redrew and the strip did not.
                let preview = self
                    .clips
                    .iter()
                    .find(|info| info.id == open.id)
                    .map(|info| &info.audio);
                let Some(preview) = preview else { return };
                EditorWindowChrome::AudioClip(crate::render::AudioEditorChrome {
                    layout: self.audio_layout.clone(),
                    clip: &open.data,
                    preview,
                    sample_rate: open.sample_rate,
                    route_label: &self.audio_route,
                    hover: self.hover_audio,
                })
            }
        };

        // Worked out before the window is borrowed mutably, because it reads
        // the window's own header and title.
        let preset = Self::preset_slot(kind)
            .zip(self.preset_bar_geometry(kind))
            .map(|(slot, layout)| crate::render::PresetBarChrome {
                layout,
                view: &self.preset_view[slot],
                hover: self
                    .hover_preset_bar
                    .filter(|(which, _)| *which == slot)
                    .map(|(_, hit)| hit),
            });
        let Some(editor) = self.editors.get_mut(index) else {
            return;
        };
        editor.scene.reset();
        draw_editor_window(
            &mut editor.scene,
            &self.options.theme,
            &editor.panel,
            &self.labels,
            &editor.title,
            &chrome,
            preset.as_ref(),
            self.menu
                .as_ref()
                .filter(|(target, _)| target.editor_window() == Some(kind))
                .map(|(_, menu)| menu),
        );

        let device = &self.context.devices[editor.surface.dev_id];
        let Some(Some(renderer)) = self.renderers.get_mut(editor.surface.dev_id) else {
            return;
        };
        let surface_texture = match editor.surface.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            // Out of date mid-resize, or occluded. Ask again rather than
            // tearing anything down over it.
            _ => {
                editor.window.request_redraw();
                return;
            }
        };
        if renderer
            .render_to_texture(
                &device.device,
                &device.queue,
                &editor.scene,
                &editor.surface.target_view,
                &RenderParams {
                    base_color: self.options.theme.palette.window.to_peniko(),
                    width: editor.surface.config.width,
                    height: editor.surface.config.height,
                    antialiasing_method: AaConfig::Area,
                },
            )
            .is_err()
        {
            return;
        }
        let mut encoder = device
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("fontelle-ui editor present"),
            });
        editor.surface.blitter.copy(
            &device.device,
            &mut encoder,
            &editor.surface.target_view,
            &surface_texture
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default()),
        );
        device.queue.submit([encoder.finish()]);
        editor.window.pre_present_notify();
        surface_texture.present();
        let _ = device.device.poll(wgpu::PollType::Poll);
        self.frames += 1;
    }

    /// Recomputes every panel's inner geometry. Called on a resize, and
    /// whenever something changes how many rows a list has.
    fn relayout_panels(&mut self) {
        let m = &self.options.theme.metrics;
        // The key strip is wider when the instrument's keys have names of
        // their own — a drum kit — and exactly what it always was otherwise.
        self.roll_layout = roll_layout_with_keys(
            self.layout.panel.body,
            m,
            self.roll.lane_height,
            crate::canvas::keyboard_width_for(&self.key_map, self.key_style),
        );
        self.roll_bar = toolbar_layout(self.roll_layout.toolbar, m);
        self.timeline_layout = timeline_layout(self.layout.timeline.body, m);
        self.timeline_bar = timeline_toolbar_layout(self.timeline_layout.toolbar, m);
        self.tabs = crate::layout::editor_tabs(self.layout.panel.header, m);
        // The instrument, the effect and the automation curve are **not**
        // laid out here any more. They are windows of their own, against
        // their own bodies — see `relayout_editors`. Computing them here as
        // well would silently overwrite an open editor's geometry with the
        // main panel's every time the sidebar seam was dragged, and every
        // knob in that window would then answer to the wrong pixels.
        self.mixer = crate::canvas::mixer_layout_for(
            self.layout.panel.body,
            m,
            &self.mixer_strips,
            self.mixer_scroll,
            Some(self.selected_track),
        );
        self.rack = rack_layout(
            self.layout.rack.body,
            m,
            self.channels.len(),
            self.rack_scroll,
        );
        self.prefab_panel = crate::canvas::prefab_layout(
            self.layout.rack.body,
            m,
            self.prefabs.len(),
            self.prefab_scroll,
        );
        self.browser = browser_layout_split(
            self.layout.browser.body,
            m,
            self.browser_mode,
            match self.browser_mode {
                BrowserMode::Sounds | BrowserMode::Presets => self.files.len(),
                BrowserMode::Projects => self.projects.len(),
                BrowserMode::Import => self.imports.len(),
                BrowserMode::Settings => self.settings.len(),
            },
            self.presets.len(),
            self.file_scroll,
            self.preset_scroll,
            self.browser_file_share,
        );
    }

    /// Re-reads the studio's lists, but only when it says they have changed.
    ///
    /// The alternative — asking for `Vec<ChannelInfo>` every frame — allocates
    /// once a frame for a list that changes when somebody clicks something.
    fn refresh_studio(&mut self) {
        let Some(doc) = &mut self.options.document else {
            return;
        };
        let revision = doc.revision();
        if revision == self.studio_revision {
            return;
        }
        self.studio_revision = revision;
        self.channels = doc.channels();
        self.rack_tab = doc.rack_tab();
        self.prefabs = doc.prefabs();
        self.files = doc.library_files();
        self.presets = doc.library_presets();
        self.projects = doc.projects();
        self.settings = doc.settings();
        self.library_count = doc.library_count();
        self.selected_channel = doc.selected_channel();
        self.selected_file = doc.selected_file();
        self.selected_preset = doc.selected_preset();
        self.lanes = doc.lanes();
        self.clips = doc.clips();
        self.instrument = doc.instrument();
        self.flopsynth = doc.flopsynth(self.flop_page);
        if let Some(view) = &mut self.flopsynth {
            view.browse = self.flop_browse.clone();
        }
        // Which knobs wear an arc, and which could take one. Both are asked
        // once here, where the view has just been built, rather than per knob
        // while the window is being drawn.
        (self.flop_modulated, self.flop_destinations) = match &self.flopsynth {
            Some(view) => {
                let mut modulated = Vec::new();
                let mut destinations = Vec::new();
                for (card, placed) in view.cards.iter().enumerate() {
                    for (param, control) in placed.group.params.iter().enumerate() {
                        if let Some(route) = doc.routes_to(&control.address).last() {
                            modulated.push(((card, param), route.depth));
                        }
                        if doc.is_mod_destination(&control.address) {
                            destinations.push((card, param));
                        }
                    }
                }
                (modulated, destinations)
            }
            None => (Vec::new(), Vec::new()),
        };
        // The audio editor's working copy, back from the document. It steps
        // its own numbers and writes them through, so it is normally ahead of
        // this — but an **undo** changes the clip underneath it, and an editor
        // still showing the numbers you took back is one that writes them
        // again on the next click. A clip that has gone closes the window's
        // contents rather than editing something nobody can see.
        if let Some(open) = &mut self.audio_clip {
            match doc.audio_clip(open.id) {
                Some(data) => open.data = data,
                None => self.audio_clip = None,
            }
        }
        self.mixer_strips = doc.mixer_strips();
        self.selected_track = doc.selected_mixer_track();
        self.track_output = doc.track_output(self.selected_track);
        self.track_output_on = doc.track_output_on(self.selected_track);
        self.track_input = doc.track_input(self.selected_track);
        self.route_names = doc.route_names();
        // Master first and spelled `None`, then every strip somebody made in
        // the order the mixer draws them — the convention every route control
        // in this window reads.
        self.clip_routes = std::iter::once((None, crate::canvas::MASTER_ROUTE.to_string()))
            .chain(
                self.route_names
                    .iter()
                    .enumerate()
                    .take(self.route_names.len().saturating_sub(1))
                    .map(|(strip, name)| (doc.mixer_track_id(strip), name.clone())),
            )
            .collect();
        // The time selection and the play mode, from the document: a loop
        // reopens with the song, and the chip says what play will do.
        self.loop_range = doc.loop_range();
        let mode = doc.play_mode();
        if mode != self.play_mode || self.mode_text.width == 0.0 {
            self.mode_text = self
                .text
                .layout(mode.label(), &self.options.theme.font, None);
            self.play_mode = mode;
            self.tree.invalidate(TRANSPORT);
        }
        // The open insert may have been removed, or its whole strip may have —
        // in which case its window closes rather than showing the effect that
        // happens to be at that index now.
        let (eq, insert_view) = match self.open_insert {
            Some((strip, slot)) => (doc.eq_config(strip, slot), doc.insert_view(strip, slot)),
            None => (None, None),
        };
        self.eq = eq;
        self.insert_view = insert_view;
        // Which insert is open is the window's own fact, and the host needs
        // it: an effect preset clicked in the browser lands in the insert you
        // are looking at.
        doc.note_open_insert(self.open_insert);
        // The bar, per window. Read here with the other lists rather than once
        // a frame, because working out whether a device is dirty means
        // comparing its state against a file (§P.6) and that is not a thing to
        // do sixty times a second.
        // The devices are worked out first, so the borrow of `doc` below does
        // not overlap the read of `self.open_insert` that names them.
        let devices = [
            self.preset_device(EditorKind::Instrument),
            self.preset_device(EditorKind::Effect),
        ];
        let Some(doc) = &mut self.options.document else {
            return;
        };
        for (slot, device) in devices.into_iter().enumerate() {
            self.preset_view[slot] = match device {
                Some(device) => doc.preset_bar(device),
                None => crate::canvas::PresetBarView::default(),
            };
        }
        if self.eq.is_none() && self.insert_view.is_none() {
            self.open_insert = None;
        }

        self.tempo = doc.tempo();
        self.key_map = doc.key_map();
        self.key_style = doc.key_style();
        let filter = self.roll.ghosts;
        self.ghosts = doc.ghost_notes(filter);
        self.query = doc.query().to_string();
        self.imports = doc.import_files();
        self.import_kind = doc.import_kind();
        let fallback = match self.browser_mode {
            BrowserMode::Sounds => doc.library_status(),
            BrowserMode::Presets => doc.preset_status(),
            BrowserMode::Projects => doc.project_status(),
            BrowserMode::Import => doc.import_status(),
            BrowserMode::Settings => doc.settings_status(),
        };
        self.status = doc.take_message().unwrap_or(fallback);

        // An editor whose subject has gone — the insert was deleted — closes
        // rather than showing whatever is at that index now. The window *is*
        // the editor, so closing one is closing the other.
        if self.eq.is_none() && self.insert_view.is_none() {
            self.close_editor(EditorKind::Effect);
        }
        // And the ones still open follow whatever moved underneath them.
        self.refresh_editors();

        // A list that shrank under a scroll offset leaves a panel that looks
        // empty until somebody scrolls back up. Against the list that is
        // *showing*: the three modes have three lengths.
        self.file_scroll = self.file_scroll.min(self.browser_rows().saturating_sub(1));
        self.preset_scroll = self.preset_scroll.min(self.presets.len().saturating_sub(1));
        // Against the room the list has, not against the last row: clamping
        // to `len - 1` leaves a full panel showing one channel. It is also
        // what makes `usize::MAX` mean "scroll to the end", which is how a
        // newly added channel is brought into view.
        self.rack_scroll = self.rack_scroll.min(
            self.channels
                .len()
                .saturating_sub(self.rack.capacity.max(1)),
        );
        self.prefab_scroll = self.prefab_scroll.min(
            self.prefabs
                .len()
                .saturating_sub(self.prefab_panel.capacity.max(1)),
        );
        // The master is pinned outside the scrolling half, so the offset is
        // clamped against the tracks that actually scroll.
        self.mixer_scroll = self
            .mixer_scroll
            .min(self.mixer_strips.len().saturating_sub(2));

        self.browser_title = self.browser_heading();
        self.relayout_panels();
        self.tree.invalidate(RACK);
        self.tree.invalidate(BROWSER);
        self.tree.invalidate(PANEL);
        self.tree.invalidate(TIMELINE);
    }

    /// Shapes everything the next frame will want to draw.
    ///
    /// `draw_window` is a pure function and cannot shape anything, so every
    /// string it will look up has to be in [`Labels`] first. Cached, so this is
    /// a few dozen hash lookups on a frame where nothing new appeared.
    fn shape_labels(&mut self) {
        // This pass is the frame, as far as the cache is concerned: what it
        // shapes from here on is kept whatever else has to go.
        self.labels.begin_frame();
        let font = self.options.theme.font.clone();
        let want = |labels: &mut Labels, text: &mut TextContext, s: &str| {
            labels.ensure(s, &font, text);
        };

        for fixed in [
            "Channels",
            ADD_CHANNEL,
            OPEN_FOLDER,
            crate::render::OPEN_CONFIG_FOLDER,
            CHOOSE_FOLDER,
            NEW_PROJECT,
            EXPORT,
            "S",
            "M",
            ARRANGEMENT,
            crate::render::RECORDING,
            EMPTY_LANE,
            TAB_ROLL,
            TAB_MIXER,
            NO_INSTRUMENT,
            crate::render::SAVE,
            crate::render::SAVE_AS,
            crate::canvas::NO_PRESET,
            crate::render::MATRIX_HEADING,
            crate::render::NO_ROUTES,
        ] {
            want(&mut self.labels, &mut self.text, fixed);
        }
        for page in crate::canvas::FlopsynthPage::ALL {
            want(&mut self.labels, &mut self.text, page.label());
        }
        if let Some(view) = &self.flopsynth {
            let voices = crate::render::voice_count_label(view.voices);
            want(&mut self.labels, &mut self.text, &voices);
        }
        // The Modulation page's badges and matrix rows, which are the source
        // and destination names the host worked out.
        if let Some(view) = self.flopsynth.clone() {
            for name in &view.sources {
                want(&mut self.labels, &mut self.text, name);
            }
            for route in &view.routes {
                want(&mut self.labels, &mut self.text, &route.source);
                want(&mut self.labels, &mut self.text, &route.destination);
            }
            // The cards' captions and read-outs, at the small size Flopsynth
            // draws them; the card names at the chrome's.
            for card in &view.cards {
                want(&mut self.labels, &mut self.text, &card.group.name);
                for param in &card.group.params {
                    self.labels
                        .ensure_small(&param.label, &font, &mut self.text);
                    self.labels
                        .ensure_small(&param.display, &font, &mut self.text);
                }
            }
            // The Presets page: every row, every shelf, the search box's
            // caption and the About column.
            for preset in &view.bank {
                want(&mut self.labels, &mut self.text, &preset.name);
                // The category tag a row wears on the shelves that mix
                // categories.
                self.labels
                    .ensure_small(&preset.category, &font, &mut self.text);
            }
            for shelf in crate::canvas::preset_shelves(&view.bank) {
                want(&mut self.labels, &mut self.text, &shelf.label());
            }
            let search = crate::render::search_caption(&view.browse.query);
            want(&mut self.labels, &mut self.text, &search);
            for line in self.flop_about() {
                want(&mut self.labels, &mut self.text, &line);
            }
            for fixed in [crate::render::NO_PRESETS_MATCH, crate::render::MINE_TAG] {
                self.labels.ensure_small(fixed, &font, &mut self.text);
            }
            want(&mut self.labels, &mut self.text, crate::canvas::ADD_EFFECT);
        }
        // Each window's preset name and category, `*` and all — two short
        // strings per window, shaped where every other caption is.
        for slot in 0..self.preset_view.len() {
            let name = crate::canvas::preset_bar_name(&self.preset_view[slot]);
            let category = self.preset_view[slot].category.clone();
            want(&mut self.labels, &mut self.text, &name);
            want(&mut self.labels, &mut self.text, &category);
        }
        // Every tab's caption and every mode's search hint, from the one list
        // of modes — see `BrowserMode::ALL`. Written out by hand, this is
        // where the Import tab came to be drawn as an empty box.
        for mode in BrowserMode::ALL {
            want(&mut self.labels, &mut self.text, mode.label());
            want(
                &mut self.labels,
                &mut self.text,
                crate::render::search_hint(mode),
            );
        }
        if self.lane_menu.is_some() {
            for property in crate::canvas::LANE_PROPERTIES {
                want(&mut self.labels, &mut self.text, property.label());
            }
        }
        // Worked out here, where both the clip and the route list are in
        // hand, and kept: the chrome takes a `&str`, and the name is also one
        // of the strings that has to be shaped below.
        self.audio_route = match &self.audio_clip {
            Some(open) => self.clip_route_label(open.data.mixer_track),
            None => String::new(),
        };
        if let Some(open) = &self.audio_clip {
            // Cloned out first: `want` borrows `self.labels` and `self.text`
            // mutably, and the captions are read through `self.audio_clip`.
            let route = self.audio_route.clone();
            let captions: Vec<String> = crate::canvas::AUDIO_ROWS
                .iter()
                .flat_map(|field| {
                    [
                        crate::canvas::audio_row_label(*field).to_string(),
                        crate::canvas::audio_row_value_at(
                            &open.data,
                            *field,
                            open.sample_rate,
                            &route,
                        ),
                    ]
                })
                .filter(|caption| !caption.is_empty())
                .collect();
            for caption in captions {
                want(&mut self.labels, &mut self.text, &caption);
            }
        }
        if let Some(panel) = &self.tools_panel {
            // Cloned out first: `want` borrows `self.labels` and `self.text`
            // mutably, and the captions are read through `self.tools`.
            let mut captions: Vec<String> = panel
                .rows
                .iter()
                .flat_map(|(row, _)| [self.tools.label(*row), self.tools.value(*row)])
                .filter(|caption| !caption.is_empty())
                .collect();
            captions.push(panel.kind.title().to_string());
            for caption in captions {
                want(&mut self.labels, &mut self.text, &caption);
            }
        }
        if let Some((_, menu)) = &self.menu {
            // Cloned out first: `want` borrows `self.labels` and `self.text`
            // mutably, and the entries live behind `self.menu`.
            let captions: Vec<String> = menu
                .entries
                .iter()
                .map(|entry| entry.label.clone())
                .collect();
            for caption in captions {
                want(&mut self.labels, &mut self.text, &caption);
            }
        }
        // The transport's two document boxes. Shaped rather than cached in
        // `Labels`, like the position read-out beside them: a tempo that is
        // being dragged never repeats a string, and caching one that never
        // repeats is a leak.
        // What the box *shows* is the tempo in force at the playhead — a
        // tempo lane bends it — while what a drag edits stays the document's
        // own. See `tempo_showing`.
        let showing = self.tempo_showing();
        if self.shaped_tempo != showing {
            let tempo = format_tempo(showing);
            self.tempo_text = self.text.layout(&tempo, &font, None);
            self.shaped_tempo = showing;
        }
        let beats = self.beats_per_bar();
        if self.shaped_beats != beats {
            let signature = format_signature(beats);
            self.signature_text = self.text.layout(&signature, &font, None);
            self.shaped_beats = beats;
        }

        // The mixer's names and read-outs, bounded by the strips on screen.
        if self.tab == EditorTab::Mixer {
            want(&mut self.labels, &mut self.text, TAB_MIXER);
            let visible: Vec<usize> = self
                .mixer
                .strips
                .iter()
                .chain(self.mixer.master.iter())
                .map(|s| s.index)
                .collect();
            for index in visible {
                let Some(strip) = self.mixer_strips.get(index).cloned() else {
                    continue;
                };
                self.labels.ensure(&strip.name, &font, &mut self.text);
                self.labels
                    .ensure(&format_gain_db(strip.gain_db), &font, &mut self.text);
                if self.drag == Drag::Pan(index) {
                    self.labels
                        .ensure(&format_pan(strip.pan), &font, &mut self.text);
                }
            }
            // The `+` column and the track-options column.
            want(&mut self.labels, &mut self.text, crate::render::ADD_TRACK);
            want(
                &mut self.labels,
                &mut self.text,
                crate::render::EFFECTS_HEADING,
            );
            want(&mut self.labels, &mut self.text, crate::render::ADD_EFFECT);
            want(
                &mut self.labels,
                &mut self.text,
                crate::render::SENDS_HEADING,
            );
            want(&mut self.labels, &mut self.text, crate::render::ADD_SEND);
            want(&mut self.labels, &mut self.text, crate::render::SEND_PRE);
            want(&mut self.labels, &mut self.text, crate::render::SEND_POST);
            want(&mut self.labels, &mut self.text, crate::render::GRIP);
            want(&mut self.labels, &mut self.text, crate::render::REMOVE);
            let output = self.output_caption();
            want(&mut self.labels, &mut self.text, &output);
            let input = self.input_caption();
            want(&mut self.labels, &mut self.text, &input);
            if let Some(options) = self.mixer.options.clone()
                && let Some(strip) = self.mixer_strips.get(options.track).cloned()
            {
                // The selected strip may be scrolled off the row of strips,
                // and its name is the column's title.
                self.labels.ensure(&strip.name, &font, &mut self.text);
                for insert in &strip.inserts {
                    self.labels.ensure(&insert.label, &font, &mut self.text);
                    let mix = crate::canvas::format_mix(insert.mix);
                    self.labels.ensure(&mix, &font, &mut self.text);
                }
                for send in &strip.sends {
                    self.labels.ensure(&send.target_name, &font, &mut self.text);
                    let level = crate::canvas::format_send_db(send.level_db);
                    self.labels.ensure(&level, &font, &mut self.text);
                }
            }
            // And whichever menu is open — the output row's or a send's.
            if let Some(menu) = self
                .output_menu
                .clone()
                .or_else(|| self.send_menu.as_ref().map(|(_, menu)| menu.clone()))
            {
                for (choice, _) in &menu.items {
                    let caption = menu.label(*choice, &self.route_names);
                    self.labels.ensure(&caption, &font, &mut self.text);
                }
            }
        }

        let heading = self.browser_title.clone();
        want(&mut self.labels, &mut self.text, &heading);
        let ghost_caption = self.roll.ghosts.label();
        want(&mut self.labels, &mut self.text, &ghost_caption);
        let lane_caption = crate::canvas::lane_caption(self.roll.lane_property);
        want(&mut self.labels, &mut self.text, &lane_caption);
        let snap_caption = crate::canvas::snap_caption(self.roll.view.snap);
        want(&mut self.labels, &mut self.text, &snap_caption);
        let tools_caption = crate::canvas::tools_caption();
        want(&mut self.labels, &mut self.text, &tools_caption);
        // Both, not only the one that is on, for the reason the two key
        // styles are both shaped: the other is what the button draws next.
        for kind in fontelle_types::FolderKind::ALL {
            want(&mut self.labels, &mut self.text, kind.tab_label());
        }
        // The record menu's three answers, so the menu it opens is never a
        // frame of empty rows — the fault a fourth browser tab shipped with.
        for mode in crate::transport::RecordMode::ALL {
            want(&mut self.labels, &mut self.text, mode.label());
        }
        // Both, not only the one that is on: the chip is pressed and the other
        // caption is what it draws next, and shaping it on the frame after
        // would draw an empty chip for one frame.
        for style in [
            crate::canvas::KeyStyle::Piano,
            crate::canvas::KeyStyle::Names,
        ] {
            want(&mut self.labels, &mut self.text, style.label());
        }
        for (control, _) in &self.roll_bar.items {
            let caption = match control {
                RollControl::Snap => &snap_caption,
                RollControl::Lane => &lane_caption,
                RollControl::Ghost => &ghost_caption,
                RollControl::Tools => &tools_caption,
                other => other.label(),
            };
            self.labels.ensure(caption, &font, &mut self.text);
        }
        if !self.query.is_empty() {
            let query = self.query.clone();
            want(&mut self.labels, &mut self.text, &query);
        }
        if !self.status.is_empty() {
            let status = self.status.clone();
            want(&mut self.labels, &mut self.text, &status);
        }
        // The hover tip. Shaped while it is *pending* rather than when it
        // falls due, so the frame that first shows it already has its width —
        // otherwise the box would be laid out around a string nothing had
        // measured and appear one frame late at the wrong size.
        if let Some(tip) = self.hover_tip.clone() {
            want(&mut self.labels, &mut self.text, &tip);
        }

        // The panel's tab captions and the prefab list's, whichever tab is
        // showing: the strip is drawn either way.
        for tab in crate::document::RackTab::ALL {
            want(&mut self.labels, &mut self.text, tab.label());
        }
        want(&mut self.labels, &mut self.text, crate::render::ADD_PREFAB);
        for row in &self.prefab_panel.rows {
            if let Some(prefab) = self.prefabs.get(row.index) {
                self.labels.ensure(&prefab.name, &font, &mut self.text);
                let caption = crate::render::prefab_uses_label(prefab.uses);
                self.labels.ensure(&caption, &font, &mut self.text);
            }
        }
        for row in &self.rack.rows {
            if let Some(channel) = self.channels.get(row.index) {
                self.labels.ensure(&channel.name, &font, &mut self.text);
                // The route chip's number. Two characters, and there are only
                // as many distinct ones as there are strips, so the cache does
                // not grow with the session.
                let caption = route_label(channel.route, self.route_names.len());
                self.labels.ensure(&caption, &font, &mut self.text);
            }
        }
        // And the route menu's rows, while it is open.
        if let Some((_, menu)) = &self.route_menu {
            let captions: Vec<String> = menu
                .items
                .iter()
                .map(|(choice, _)| menu.label(*choice, &self.route_names))
                .collect();
            for caption in captions {
                self.labels.ensure(&caption, &font, &mut self.text);
            }
        }
        // The arrangement's own text: a name on every visible lane and on every
        // visible clip. Bounded by what is on screen, like everything else here.
        if !self.layout.timeline.frame.is_empty() {
            let lanes = crate::canvas::visible_lanes(
                &self.timeline.view,
                self.timeline_layout.grid,
                self.lanes.len(),
            );
            for lane in lanes.clone() {
                if let Some(info) = self.lanes.get(lane) {
                    self.labels.ensure(&info.name, &font, &mut self.text);
                }
            }
            let ticks = crate::canvas::timeline_visible_ticks(
                &self.timeline.view,
                self.timeline_layout.grid,
            );
            for clip in &self.clips {
                if lanes.contains(&clip.lane)
                    && clip.start <= ticks.end
                    && clip.start + clip.length >= ticks.start
                {
                    self.labels.ensure(&clip.name, &font, &mut self.text);
                }
            }
            let bar = fontelle_types::PPQN * i64::from(self.beats_per_bar().max(1));
            if let Some(stride) = label_stride(bar as f32 * self.timeline.view.pixels_per_tick) {
                let mut tick = ticks.start - ticks.start.rem_euclid(bar);
                while tick < ticks.end {
                    let number = tick / bar + 1;
                    if labelled_bar(number, stride) {
                        self.labels
                            .ensure(&number.to_string(), &font, &mut self.text);
                    }
                    tick += bar;
                }
            }
        }
        let rows: Vec<usize> = self.browser.file_rows.iter().map(|(i, _)| *i).collect();
        for index in rows {
            let entry = match self.browser_mode {
                BrowserMode::Sounds | BrowserMode::Presets => self.files.get(index).cloned(),
                BrowserMode::Projects => self.projects.get(index).cloned(),
                BrowserMode::Import => self.imports.get(index).cloned(),
                BrowserMode::Settings => self.settings.get(index).cloned(),
            };
            if let Some(entry) = entry {
                self.labels.ensure(&entry.name, &font, &mut self.text);
                self.labels.ensure(&entry.detail, &font, &mut self.text);
            }
        }
        for (index, _) in &self.browser.preset_rows {
            if let Some(entry) = self.presets.get(*index) {
                self.labels.ensure(&entry.name, &font, &mut self.text);
                self.labels.ensure(&entry.detail, &font, &mut self.text);
            }
        }

        // The instrument editor's captions and read-outs, which change as a
        // knob turns — bounded by what is on the panel, like everything else.
        // Shaped by the *main* window even though a floating one draws them:
        // `Labels` is the font cache and there is one of it, so an editor
        // window looks its captions up in the same place.
        // The two editor windows that draw a caption of their own inside the
        // panel. Neither was ever shaped, so both rows drew as nothing — a
        // reserved band of empty pixels where the name of the thing you were
        // editing was meant to be. Found by opening one.
        if self.is_editor_open(EditorKind::Effect) {
            let title = self.effect_title();
            self.labels.ensure(&title, &font, &mut self.text);
            // The editor's own captions: the eight band numbers, the axis, and
            // the read-out on every control. Bounded by what is on the panel,
            // like everything else here.
            let config = self.eq.unwrap_or_default();
            for band in 0..fontelle_types::BANDS {
                self.labels
                    .ensure(&(band + 1).to_string(), &font, &mut self.text);
            }
            for field in crate::canvas::EqField::ALL {
                let caption = crate::canvas::eq_field_caption(field, &config, self.eq_band);
                self.labels.ensure(&caption, &font, &mut self.text);
            }
            for caption in crate::render::EQ_AXIS_CAPTIONS {
                self.labels.ensure(caption, &font, &mut self.text);
            }
        }
        // The shapes a point's menu offers, and the tempo's menu.
        for shape in crate::canvas::CURVE_SHAPES {
            self.labels
                .ensure(crate::canvas::curve_label(shape), &font, &mut self.text);
        }

        // Both panels of knobs: the instrument's, and the one an effect that is
        // not an EQ opens. Same shape, same captions to shape.
        for view in [
            self.is_editor_open(EditorKind::Instrument)
                .then(|| self.instrument.clone())
                .flatten(),
            self.is_editor_open(EditorKind::Effect)
                .then(|| self.insert_view.clone())
                .flatten(),
        ]
        .into_iter()
        .flatten()
        {
            // The preset row. Shaped like everything else that is drawn: a
            // caption nobody shaped draws as nothing, which is a row of empty
            // chips — a defect this file has already recorded once, on the
            // editor titles.
            for name in view.keys.iter() {
                self.labels.ensure(name, &font, &mut self.text);
            }
            for group in &view.groups {
                self.labels.ensure(&group.name, &font, &mut self.text);
                for param in &group.params {
                    self.labels.ensure(&param.label, &font, &mut self.text);
                    self.labels.ensure(&param.display, &font, &mut self.text);
                }
            }
        }

        // The arrangement's toolbar. Its snap chip says its division, which
        // changes, so it is shaped from the live value rather than once — and
        // with a caret on it, because it drops a list.
        let timeline_snap = crate::canvas::snap_caption(self.timeline.view.snap);
        for (control, _) in &self.timeline_bar.items {
            let caption = match control {
                crate::canvas::TimelineControl::Snap => &timeline_snap,
                other => other.label(),
            };
            self.labels.ensure(caption, &font, &mut self.text);
        }

        // The roll's own text: a name beside every C, and a number on every
        // bar line the ruler has room to number.
        if self.options.document.is_some() {
            for key in crate::canvas::visible_keys(&self.roll.view, self.roll_layout.grid) {
                // The name of the thing on the key, when the instrument has
                // one, and the octave name otherwise — the same precedence
                // `draw_keyboard` uses, because a caption it draws and this
                // never shaped draws as nothing at all.
                if let Some(name) = self.key_map.name(key.clamp(0, 127) as u8) {
                    let name = name.to_string();
                    self.labels.ensure(&name, &font, &mut self.text);
                } else if key % 12 == 0 || self.key_style == crate::canvas::KeyStyle::Names {
                    // Every row is named in the list view, falling back to the
                    // note where the instrument has nothing of its own to say —
                    // that is what makes it a list rather than a keyboard with
                    // the black keys painted out.
                    self.labels.ensure(&key_name(key), &font, &mut self.text);
                }
            }
            let beats = self
                .options
                .document
                .as_ref()
                .map_or(BEATS_PER_BAR, |doc| doc.beats_per_bar());
            let bar = fontelle_types::PPQN * i64::from(beats.max(1));
            if let Some(stride) = label_stride(bar as f32 * self.roll.view.pixels_per_tick) {
                let ticks = crate::canvas::visible_ticks(&self.roll.view, self.roll_layout.grid);
                let mut tick = ticks.start - ticks.start.rem_euclid(bar);
                while tick < ticks.end {
                    let number = tick / bar + 1;
                    if labelled_bar(number, stride) {
                        self.labels
                            .ensure(&number.to_string(), &font, &mut self.text);
                    }
                    tick += bar;
                }
            }
        }
    }

    // ------------------------------------------------------------- mouse ---

    /// One press, routed to whichever panel it landed in.
    fn press(&mut self, button: winit::event::MouseButton, x: f32, y: f32) {
        self.drag = Drag::None;
        self.end_edge_scroll();
        // A press outside the soundfont panel hands the arrow keys back to the
        // notes and clips. `press_browser` puts the focus back on straight
        // after, so clicking a preset row keeps it.
        if !self.layout.browser.frame.contains(x, y) && self.preset_focus.take().is_some() {
            self.tree.invalidate(BROWSER);
        }

        // Whatever the pointer was explaining, it is explaining it about a
        // window that is about to change: a menu opened by a press would be
        // drawn *under* the tip of the button that opened it. The dwell
        // starts again, which is what a person expects after clicking.
        self.dismiss_tip();

        // An open menu is above everything, including the transport bar, and a
        // click anywhere shuts it — which is what every menu on every desktop
        // does. Before the bar, or clicking Play with the menu up would both
        // start the song and leave the menu hanging over the roll.
        if self.menu.is_some() {
            // A press that chose an entry, or that landed on the menu without
            // choosing one, is spent — the panel underneath must not also act
            // on it. A press that missed the menu closes it and then carries
            // on to whatever it hit, which is what every desktop does.
            if self.press_menu(x, y) {
                return;
            }
        }
        if self.tools_panel.is_some() {
            self.press_tools_panel(x, y);
            return;
        }
        if self.lane_menu.is_some() {
            self.press_lane_menu(x, y);
            return;
        }
        if self.route_menu.is_some() {
            self.press_route_menu(x, y);
            return;
        }
        if self.output_menu.is_some() {
            self.press_output_menu(x, y);
            return;
        }
        if self.send_menu.is_some() {
            self.press_send_menu(x, y);
            return;
        }

        // A press anywhere ends a rename, for the reason the search box gives
        // its keyboard back further down: a window whose keyboard is stuck in
        // a text field nobody can see is a window that has stopped responding.
        // After the menus, because the press that *starts* a rename is a press
        // on one of them.
        if self.renaming.is_some() {
            self.renaming = None;
            self.status.clear();
            if let Some(doc) = &mut self.options.document {
                doc.end_gesture();
            }
            // Every panel a name is typed in, the mixer included — a press in
            // the rack that ends a rename in the mixer has to take the caret
            // off the strip it was on.
            self.invalidate_names();
        }

        // The transport bar first: it is the only thing above the panels.
        if let Some(what) = hit(&self.bar, &self.view, x, y) {
            // §12.4's rule on the tempo box: right-click any control to
            // automate it, and the tempo is a control (§12.3). *"even the
            // tempo section for example which i currently cannot turn into
            // an automation clip."*
            if button == winit::event::MouseButton::Right && what == TransportHit::Tempo {
                let bounds = self.layout.window;
                self.open_menu(MenuTarget::Tempo, x, y, bounds);
                self.tree.invalidate(TRANSPORT);
                return;
            }
            if button == winit::event::MouseButton::Left {
                // The boxes that write to the *document* rather than to the
                // engine. `transport` would do nothing with any of them —
                // `action` returns `None` — so they are dealt with here,
                // where the document is reachable.
                match what {
                    TransportHit::Tempo => {
                        self.value_drag = Some((self.tempo, y));
                        self.drag = Drag::Tempo;
                        return;
                    }
                    TransportHit::Signature => {
                        self.set_beats_per_bar(cycle_beats_per_bar(self.beats_per_bar()));
                        return;
                    }
                    TransportHit::Mode => {
                        self.toggle_play_mode();
                        return;
                    }
                    _ => {}
                }
                self.transport(what);
                // A press on the ruler is the start of a scrub, not one click.
                // Without this the bar could only be *tapped*, which is what
                // made getting the playhead to the very start of the song a
                // matter of hitting one pixel.
                if matches!(what, TransportHit::Scrub(_)) {
                    self.drag = Drag::BarRuler;
                }
            }
            return;
        }

        // Clicking anywhere but the search box gives the keyboard back to the
        // roll — otherwise typing a note-tool shortcut types it into the
        // search field instead.
        let in_search = self.browser.search.contains(x, y);
        if self.searching != in_search {
            self.searching = in_search;
            self.tree.invalidate(BROWSER);
        }

        if self.layout.rack.frame.contains(x, y) {
            // Converted here rather than at the top of `press`, because the
            // transport bar above still compares against winit's own enum.
            self.press_rack(
                match button {
                    winit::event::MouseButton::Right => MouseButton::Right,
                    _ => MouseButton::Left,
                },
                x,
                y,
            );
            return;
        }
        if self.layout.browser.frame.contains(x, y) {
            self.press_browser(x, y);
            return;
        }
        if self.layout.sidebar_seam.contains(x, y) {
            self.drag = Drag::SidebarSeam;
            return;
        }
        if self.layout.sidebar_split.contains(x, y) {
            self.drag = Drag::SidebarSplit;
            return;
        }
        if self.layout.divider.contains(x, y) {
            self.drag = Drag::Divider;
            return;
        }

        let Some(button) = (match button {
            winit::event::MouseButton::Left => Some(MouseButton::Left),
            winit::event::MouseButton::Right => Some(MouseButton::Right),
            _ => None,
        }) else {
            return;
        };

        if self.layout.timeline.frame.contains(x, y) {
            self.press_timeline(button, x, y);
            return;
        }
        // The editor column's tabs live in its header, above everything else
        // in the panel.
        if let Some(tab) = editor_tab_at(&self.tabs, x, y)
            && button == MouseButton::Left
        {
            self.show_tab(tab);
            return;
        }
        // Everything below is the editor, so the keyboard is now its.
        if self.focus != Focus::Roll {
            self.focus = Focus::Roll;
            self.tree.invalidate(TIMELINE);
            self.tree.invalidate(PANEL);
        }

        if self.tab == EditorTab::Mixer {
            match button {
                MouseButton::Left => self.press_mixer(x, y),
                // §12.4: right-click any control to automate it. The fader and
                // the pan are controls like any other, and they get the same
                // gesture the effects do because they share the addressing
                // scheme (§8.2) rather than because it was wired twice.
                MouseButton::Right => match mixer_hit(&self.mixer, x, y) {
                    MixerHit::Fader(strip) => self.automate_track(strip, true),
                    MixerHit::Pan(strip) => self.automate_track(strip, false),
                    // A wet/dry is a control like any other, and sweeping one
                    // is how an effect is brought in over a bar.
                    MixerHit::Options(crate::canvas::OptionsHit::InsertMix(slot)) => {
                        self.automate_insert(
                            self.selected_track,
                            slot,
                            fontelle_types::MIX,
                            "wet/dry",
                        );
                    }
                    _ => {}
                },
            }
            return;
        }
        if self.roll_layout.toolbar.contains(x, y) {
            if button == MouseButton::Left
                && let Some(control) = toolbar_hit(&self.roll_bar, x, y)
            {
                self.activate(control);
            }
            return;
        }
        // The roll's chrome — the lane, its seam, the keyboard, the ruler —
        // answers to the left button only. The right button is the eraser
        // (see `canvas::piano_roll`'s `Gesture::Erasing`), and none of these
        // hold anything erasable, so a right press on them has to do
        // *nothing*: sounding a key, moving the playhead or grabbing the lane
        // seam in the middle of a sweep across the grid is the window
        // answering a question nobody asked.
        if button == MouseButton::Left {
            // Before the grid, because the grip is the last few pixels of it.
            if !self.roll_layout.velocity.is_empty() && self.roll_layout.lane_grip.contains(x, y) {
                self.roll.press_lane_grip();
                self.drag = Drag::LaneGrip;
                return;
            }
            if self.roll_layout.velocity.contains(x, y) {
                self.drag = Drag::Lane;
                self.press_lane(x, y);
                return;
            }
            if self.roll_layout.keys.contains(x, y) {
                // The on-screen keyboard: clicking a key plays it (TDD §14.1's
                // live path). Nothing is written down — it is an audition, and
                // sliding down the keyboard sounds each key as it is reached.
                let key = y_to_key(&self.roll.view, self.roll_layout.grid, y);
                self.drag = Drag::Keys;
                // No note behind it, so no length: the keyboard gets the floor.
                self.start_audition(key, 0);
                return;
            }
            if self.roll_layout.ruler.contains(x, y) {
                self.drag = Drag::RollRuler;
                self.mark_at_roll(x);
                return;
            }
        } else if self.roll_layout.ruler.contains(x, y) {
            // The right button on the roll's ruler drags a time selection
            // out, the same as on the arrangement's — in this clip's ticks,
            // stored as the song's.
            let grid = self.roll_layout.grid;
            let (x, _) = clamp_to_grid(grid, x, grid.y);
            let clip_tick = x_to_tick(&self.roll.view, grid, x);
            if let Some(doc) = &self.options.document {
                self.select_anchor = doc.song_tick_of_clip_tick(clip_tick);
                self.select_preview = None;
                self.drag = Drag::RollSelect;
            }
            return;
        } else if self.roll_layout.velocity.contains(x, y) || self.roll_layout.keys.contains(x, y) {
            return;
        }
        self.drag = Drag::Roll;
        self.press_roll(button, x, y);
    }

    /// How far the view should travel this step because the pointer is being
    /// held off the edge of `grid`.
    ///
    /// **Against the clock, not against the event.** The rate says pixels per
    /// second (`edge_scroll_rate`) and this charges it for the time since the
    /// last step, so a gaming mouse reporting a thousand times a second and a
    /// cheap one reporting a hundred carry the drag the same distance —
    /// *"when i drag things they often go wayyyy off into infinity ... with
    /// the slightest mouse movement"* was the mouse's report rate being the
    /// speed control.
    fn edge_scroll_step(
        &mut self,
        view: &crate::canvas::RollView,
        grid: crate::layout::Rect,
    ) -> (fontelle_types::Tick, i32) {
        let now = std::time::Instant::now();
        let dt = match self.edge_scroll_at.replace(now) {
            Some(then) => now.duration_since(then).as_secs_f32(),
            // The first move of a gesture has no time behind it, so it is
            // worth nothing — the scroll begins on the second.
            None => return (0, 0),
        };
        let (x, y) = self.cursor;
        let rate = edge_scroll_rate(view, grid, x, y);
        self.edge_scroll.step(rate, dt)
    }

    /// Ends an edge scroll: the clock and the part-tick both go.
    fn end_edge_scroll(&mut self) {
        self.edge_scroll.reset();
        self.edge_scroll_at = None;
    }

    /// One pointer move, sent to whatever the press decided this drag is.
    fn drag_pointer(&mut self) {
        let (x, y) = self.cursor;
        match self.drag {
            Drag::None => {}
            Drag::MenuScroll => self.drag_menu_scroll(y),
            Drag::Roll => self.drag_roll(),
            Drag::Lane => self.drag_lane(),
            Drag::LaneGrip => self.drag_lane_grip(y),
            Drag::RollRuler => self.mark_at_roll(x),
            Drag::BarRuler => self.mark_on_bar(x),
            Drag::Keys => self.audition_at(y),
            Drag::Timeline => self.drag_timeline(),
            Drag::TimelineRuler => self.mark_on_timeline(x),
            Drag::Divider => self.drag_divider(y),
            Drag::SidebarSeam => self.drag_sidebar_seam(x),
            Drag::SidebarSplit => self.drag_sidebar_split(y),
            Drag::BrowserSplit => self.drag_browser_split(y),
            // Carried, not acted on: what it means is decided by where it is
            // let go.
            Drag::BrowserRow(_) => {}
            Drag::Knob => self.drag_knob(y),
            Drag::FlopKnob => self.drag_flop_knob(y),
            Drag::FlopWave(card) => self.drag_flop_wave(card, x),
            Drag::FlopResponse(card) => self.drag_flop_response(card, x, y),
            Drag::FlopEnvNode => self.drag_flop_env_node(x, y),
            Drag::FlopRing => self.drag_flop_ring(y),
            Drag::FlopMatrix(index) => self.drag_flop_matrix(index, x),
            // The badge follows the pointer, and the knobs light up behind it.
            Drag::FlopAssign => {
                if let Some((_, at)) = &mut self.flop_assign {
                    *at = (x, y);
                }
                self.tree.invalidate(PANEL);
                self.redraw_editors();
            }
            Drag::AudioRow(field) => self.drag_audio_row(field, x),
            Drag::InsertKnob => self.drag_insert_knob(y),
            Drag::Fader(strip) => self.drag_fader(strip, y),
            Drag::EqHandle(band) => self.drag_eq(band, x, y),
            Drag::TimelineSelect => self.drag_select_timeline(x),
            Drag::RollSelect => self.drag_select_roll(x),
            Drag::InsertRow => self.drag_insert_row(x, y),
            Drag::SendLevel(index) => self.drag_send_level(index, x),
            Drag::InsertMix(slot) => self.drag_insert_mix(slot, y),
            Drag::EqField(field) => self.drag_eq_field(field, y),
            Drag::Pan(strip) => self.drag_pan(strip, x),
            Drag::Tempo => self.drag_tempo(y),
        }
    }

    // ------------------------------------------------------------ the mixer ---

    /// A press on the mixer panel.
    ///
    /// A fader and a pan start an **absolute** drag — the value jumps to where
    /// the press landed and then follows — because that is how a fader works
    /// everywhere and because the alternative makes "pull it right down" a
    /// long haul rather than one click. The two switches step on the press;
    /// there is nothing to drag.
    fn press_mixer(&mut self, x: f32, y: f32) {
        match mixer_hit(&self.mixer, x, y) {
            MixerHit::Fader(strip) => {
                self.drag = Drag::Fader(strip);
                self.drag_fader(strip, y);
            }
            MixerHit::Pan(strip) => {
                self.drag = Drag::Pan(strip);
                self.drag_pan(strip, x);
            }
            MixerHit::Mute(strip) => {
                if let Some(doc) = &mut self.options.document {
                    doc.toggle_track_mute(strip);
                }
                self.refresh_title();
            }
            MixerHit::Solo(strip) => {
                if let Some(doc) = &mut self.options.document {
                    doc.toggle_track_solo(strip);
                }
                self.refresh_title();
            }
            // The rack. One kind of effect ships so far, so the add row adds
            // it rather than dropping a menu with one row in it; the menu is
            // what the second effect brings.
            MixerHit::AddInsert(strip) => {
                let anchor = self
                    .strip_layout(strip)
                    .map_or(crate::layout::Rect::ZERO, |s| s.add);
                self.open_effect_menu(strip, anchor);
            }
            MixerHit::BypassInsert(strip, slot) => {
                if let Some(doc) = &mut self.options.document {
                    doc.toggle_insert_bypass(strip, slot);
                }
                self.refresh_studio();
                self.refresh_title();
            }
            MixerHit::Insert(strip, slot) => {
                self.open_insert(strip, slot);
            }
            // Selecting a strip is what points the track-options column at
            // it, which is the only place its chain and its routing can be
            // reached. The name row and the strip's own body both do it: a
            // strip you have to aim at a 22-pixel caption to choose is one
            // nobody realises they can choose at all.
            // *"i want to be able to click on their name to type in that
            // field."* The first click chooses the strip and a click on the
            // name of the one already chosen starts typing — see
            // `canvas::name_press`, which is where that rule lives.
            MixerHit::Name(strip) => match crate::canvas::name_press(strip, self.selected_track) {
                crate::canvas::NamePress::Rename => {
                    self.start_rename(MenuTarget::MixerTrack(strip))
                }
                crate::canvas::NamePress::Select => self.select_track(strip),
            },
            MixerHit::Strip(strip) => self.select_track(strip),
            // *"a plus where you can add a new track there"* — and the new
            // track is selected, because it is the one you are about to put
            // something on.
            MixerHit::AddTrack => {
                if let Some(doc) = &mut self.options.document {
                    doc.add_mixer_track();
                }
                self.refresh_studio();
                self.refresh_title();
            }
            MixerHit::Options(what) => self.press_options(what),
            MixerHit::Nothing => {}
        }
        self.tree.invalidate(PANEL);
    }

    /// What the options column's output row says.
    ///
    /// The master has no name of its own in `route_names` beyond being last in
    /// it, which is the same convention the rack's route chip reads.
    fn output_caption(&self) -> String {
        // A track that goes nowhere says so rather than naming where it would
        // go if it were connected — which would be a row that reads exactly
        // like an audible track and a mix you cannot explain.
        if !self.track_output_on {
            return crate::render::output_label(crate::canvas::NO_OUTPUT);
        }
        let name = match self.track_output {
            Some(index) => self.route_names.get(index).cloned(),
            None => self.route_names.last().cloned(),
        };
        crate::render::output_label(&name.unwrap_or_else(|| "Master".to_string()))
    }

    /// What the options column's input row says (TDD §15.4).
    ///
    /// The device's name, or that the track records nothing — which is what
    /// every track says until somebody chooses one, and is a state rather than
    /// a blank row.
    fn input_caption(&self) -> String {
        match &self.track_input {
            Some(name) => format!("In: {name}"),
            None => "In: none".to_string(),
        }
    }

    /// The selected strip's mute or solo, from the keyboard.
    ///
    /// The same two document calls the switches on the strip make, so a
    /// keypress and a click are one edit rather than two paths that can drift
    /// — see `press_mixer`.
    fn toggle_selected_track(&mut self, key: crate::canvas::MixerKey) {
        let strip = self.selected_track;
        if let Some(doc) = &mut self.options.document {
            match key {
                crate::canvas::MixerKey::Mute => doc.toggle_track_mute(strip),
                crate::canvas::MixerKey::Solo => doc.toggle_track_solo(strip),
            }
        }
        self.refresh_title();
        self.tree.invalidate(PANEL);
    }

    /// Points the mixer — and the track-options column with it — at `strip`.
    fn select_track(&mut self, strip: usize) {
        if let Some(doc) = &mut self.options.document {
            doc.select_mixer_track(strip);
        }
        self.refresh_studio();
        self.tree.invalidate(PANEL);
    }

    /// A press in the track-options column.
    fn press_options(&mut self, what: crate::canvas::OptionsHit) {
        use crate::canvas::OptionsHit;
        let strip = self.selected_track;
        match what {
            // Where the track goes (§13.2). The same menu the rack's route
            // chip drops, over the same list, minus this track itself — the
            // shortest possible feedback loop and the easiest to click by
            // accident.
            OptionsHit::Output => self.open_output_menu(),
            // *"i click a input button that lets my select my mic input to
            // feed to that mixer track."* A menu of what the machine has,
            // because the answer cannot be guessed.
            OptionsHit::Input => {
                let anchor = self
                    .mixer
                    .options
                    .as_ref()
                    .map_or(crate::layout::Rect::ZERO, |o| o.input);
                let bounds = self.layout.window;
                self.open_menu(
                    MenuTarget::TrackInput(strip),
                    anchor.x,
                    anchor.bottom(),
                    bounds,
                );
            }
            // Which effect is a question the row cannot ask, so it drops the
            // menu rather than guessing. It used to add an EQ every time,
            // which made the compressor unreachable from the window at all.
            OptionsHit::AddInsert => {
                let anchor = self
                    .mixer
                    .options
                    .as_ref()
                    .map_or(crate::layout::Rect::ZERO, |o| o.add_insert);
                self.open_effect_menu(strip, anchor);
            }
            OptionsHit::Insert(slot) => self.open_insert(strip, slot),
            OptionsHit::Bypass(slot) => {
                if let Some(doc) = &mut self.options.document {
                    doc.toggle_insert_bypass(strip, slot);
                }
                self.refresh_studio();
                self.refresh_title();
            }
            OptionsHit::Remove(slot) => {
                if let Some(doc) = &mut self.options.document {
                    doc.remove_insert(strip, slot);
                }
                // The editor may have been showing the effect that just went.
                if self.open_insert == Some((strip, slot)) {
                    self.open_insert = None;
                }
                self.refresh_studio();
                self.refresh_title();
            }
            // Picked up, not applied: the move happens where it is let go.
            OptionsHit::Grip(slot) => {
                self.drag = Drag::InsertRow;
                self.insert_drag = Some((slot, slot));
            }
            // The column is already about this track, so there is nothing
            // for a press on its title to choose: it types. The one place in
            // the mixer where renaming is a single click from anywhere.
            OptionsHit::Rename => self.start_rename(MenuTarget::MixerTrack(strip)),

            // --- the sends (§13.2) ---
            OptionsHit::AddSend => self.open_send_menu(None),
            OptionsHit::Send(index) => self.open_send_menu(Some(index)),
            OptionsHit::SendTap(index) => {
                if let Some(doc) = &mut self.options.document {
                    doc.toggle_send_pre_fader(strip, index);
                }
                self.refresh_studio();
                self.refresh_title();
            }
            OptionsHit::SendRemove(index) => {
                if let Some(doc) = &mut self.options.document {
                    doc.remove_send(strip, index);
                }
                self.refresh_studio();
                self.refresh_title();
            }
            // Absolute, like a fader: a press on the groove takes the level to
            // where it landed and then follows.
            OptionsHit::SendLevel(index) => {
                self.drag = Drag::SendLevel(index);
                self.drag_send_level(index, self.cursor.0);
            }
            // A knob, so it is *turned* rather than slid: the drag starts from
            // where the value is and moves with the pointer, which is the
            // gesture every other knob in this window has.
            OptionsHit::InsertMix(slot) => {
                let from = self
                    .mixer_strips
                    .get(strip)
                    .and_then(|track| track.inserts.get(slot))
                    .map_or(1.0, |insert| insert.mix);
                self.mix_drag = Some((from, self.cursor.1));
                self.drag = Drag::InsertMix(slot);
            }
        }
    }

    /// Drops the menu of effects that can go on a track, from the row that
    /// asked for one.
    ///
    /// A right-click menu like every other now, rather than a menu of its own:
    /// its rows come from [`crate::canvas::effect_menu_rows`], so the
    /// favourites go first and a starred plugin effect is one press away.
    fn open_effect_menu(&mut self, strip: usize, anchor: crate::layout::Rect) {
        if anchor.is_empty() {
            return;
        }
        self.scan_for_starred_plugins();
        let bounds = self.layout.window;
        self.open_menu(
            MenuTarget::AddEffect(strip),
            anchor.x,
            anchor.bottom(),
            bounds,
        );
    }

    /// Looks for plugins once, if any of the favourites is one.
    ///
    /// A starred plugin is offered straight from the *+ fx* and *New
    /// instrument* menus, and it can only be offered if the machine has been
    /// searched. The picker searches when it opens; these menus search when
    /// they have a reason to, so a person who never starred a plugin never
    /// waits for a scan they did not ask for.
    fn scan_for_starred_plugins(&mut self) {
        let Some(doc) = &mut self.options.document else {
            return;
        };
        if doc
            .favorites()
            .iter()
            .any(|favorite| matches!(favorite, fontelle_types::Favorite::Plugin(_)))
        {
            doc.scan_plugins_once();
        }
    }

    /// Drops the menu of everywhere a send could go.
    ///
    /// `index` is the send being re-pointed, or `None` to make a new one — one
    /// menu for both, because "where does this go" is the same question either
    /// way and a second list of the same tracks is a second thing to keep in
    /// step.
    fn open_send_menu(&mut self, index: Option<usize>) {
        let Some(options) = self.mixer.options.clone() else {
            return;
        };
        let chip = match index {
            Some(index) => options
                .sends
                .iter()
                .find(|row| row.index == index)
                .map_or(options.add_send, |row| row.target),
            None => options.add_send,
        };
        self.send_menu = Some((
            index,
            crate::canvas::route_menu_layout_excluding(
                chip,
                self.layout.panel.body,
                &self.options.theme.metrics,
                &self.route_names,
                // A track cannot send to itself: the shortest possible
                // feedback loop, and the easiest to click by accident.
                Some(self.selected_track),
            ),
        ));
        self.tree.invalidate(PANEL);
    }

    /// A press while the send menu is open. Anywhere but a row shuts it.
    fn press_send_menu(&mut self, x: f32, y: f32) {
        let Some((index, menu)) = self.send_menu.take() else {
            return;
        };
        self.tree.invalidate(PANEL);
        let Some(choice) = route_menu_hit(&menu, x, y) else {
            return;
        };
        let strip = self.selected_track;
        // The master is the last of `route_names`, and it is a destination
        // like any other for a send — unlike an *output*, where "the master"
        // is spelled `None`.
        let master = self.route_names.len().saturating_sub(1);
        let Some(doc) = &mut self.options.document else {
            return;
        };
        let target = match choice {
            RouteChoice::Master => master,
            RouteChoice::Track(track) => track,
            RouteChoice::New => {
                // "I need a reverb bus and this goes to it", in one gesture.
                let made = self.route_names.len().saturating_sub(1);
                doc.add_mixer_track();
                doc.select_mixer_track(strip);
                made
            }
            // `route_menu_layout_excluding` never offers it — a send with
            // nowhere to go is a send to delete.
            RouteChoice::Off => return,
        };
        // Re-pointing an existing send is a delete and a make: `Send::target`
        // has no command of its own, and one that only changed the target
        // would still be a graph rebuild and a history entry — which is what
        // these two are.
        if let Some(index) = index {
            doc.remove_send(strip, index);
        }
        doc.add_send(strip, target);
        self.refresh_studio();
        self.refresh_title();
    }

    /// Follows a send's level while it is being dragged.
    /// Blends one insert with the signal that went into it.
    ///
    /// **Turned, not slid**: relative to where the value was when the drag
    /// started, up for more, with Shift for a fine turn — `knob_value` is the
    /// same arithmetic the instrument editor's knobs use, so every dial in the
    /// window behaves alike. One undo entry for the whole drag: the command
    /// merges, and `end_gesture` on the mouse-up closes it.
    fn drag_insert_mix(&mut self, slot: usize, y: f32) {
        let Some((from, from_y)) = self.mix_drag else {
            return;
        };
        let mix = knob_value(from, y - from_y, self.modifiers.shift_key());
        let strip = self.selected_track;
        if self
            .mixer_strips
            .get(strip)
            .and_then(|track| track.inserts.get(slot))
            .is_some_and(|insert| (insert.mix - mix).abs() < 0.0005)
        {
            // Nothing to say when the knob has not moved a step: every write
            // is a command, a publish to the audio thread and a redraw.
            return;
        }
        if let Some(doc) = &mut self.options.document {
            doc.set_insert_mix(strip, slot, mix);
        }
        self.refresh_studio();
        self.refresh_title();
        self.tree.invalidate(PANEL);
    }

    fn drag_send_level(&mut self, index: usize, x: f32) {
        let Some(options) = &self.mixer.options else {
            return;
        };
        let Some(row) = options.sends.iter().find(|row| row.index == index) else {
            return;
        };
        let db = crate::canvas::send_level_at(row.level, x);
        let strip = self.selected_track;
        let current = self
            .mixer_strips
            .get(strip)
            .and_then(|s| s.sends.get(index))
            .map_or(0.0, |send| send.level_db);
        // Nothing to say when the value has not moved — the same guard
        // `drag_fader` has, and for the same reason.
        if (db - current).abs() < 0.001 {
            return;
        }
        if let Some(doc) = &mut self.options.document {
            doc.set_send_level(strip, index, db);
        }
        self.refresh_studio();
        self.refresh_title();
        self.tree.invalidate(PANEL);
    }

    /// Drops the output row's menu, listing everywhere this track could go.
    fn open_output_menu(&mut self) {
        let Some(options) = self.mixer.options.clone() else {
            return;
        };
        self.output_menu = Some(crate::canvas::output_menu_layout(
            options.output,
            self.layout.panel.body,
            &self.options.theme.metrics,
            &self.route_names,
            Some(self.selected_track),
        ));
        self.tree.invalidate(PANEL);
    }

    /// A press while the output menu is open. Anywhere but a row shuts it.
    fn press_output_menu(&mut self, x: f32, y: f32) {
        let Some(menu) = self.output_menu.take() else {
            return;
        };
        self.tree.invalidate(PANEL);
        let Some(choice) = route_menu_hit(&menu, x, y) else {
            return; // a click off the menu is how a menu is dismissed
        };
        let strip = self.selected_track;
        let Some(doc) = &mut self.options.document else {
            return;
        };
        match choice {
            // Choosing a destination **connects** it as well as naming it, or
            // picking "Master" on a track that was switched off would look
            // like a menu that did nothing. See `MixerTrack::output_on`.
            RouteChoice::Master => {
                doc.set_track_output(strip, None);
                doc.set_track_output_on(strip, true);
            }
            RouteChoice::Track(index) => {
                doc.set_track_output(strip, Some(index));
                doc.set_track_output_on(strip, true);
            }
            // *"if i chose to not route it to master."* The destination is
            // kept, so switching it back on puts the track where it was.
            RouteChoice::Off => doc.set_track_output_on(strip, false),
            RouteChoice::New => {
                // "I need a drum bus and this goes into it", in one gesture.
                // The new track lands before the master, so its index is the
                // number of strips there were minus the master's own.
                let index = self.route_names.len().saturating_sub(1);
                doc.add_mixer_track();
                // `add_mixer_track` selects what it made; the routing is about
                // the strip that was selected when the menu was opened.
                doc.select_mixer_track(strip);
                doc.set_track_output(strip, Some(index));
                doc.set_track_output_on(strip, true);
            }
        }
        self.refresh_studio();
        self.refresh_title();
    }

    /// Follows an insert being dragged up or down its chain.
    fn drag_insert_row(&mut self, x: f32, y: f32) {
        let Some((from, _)) = self.insert_drag else {
            return;
        };
        let Some(options) = &self.mixer.options else {
            return;
        };
        let over = options
            .inserts
            .iter()
            .find(|row| row.frame.contains(x, y))
            .map(|row| row.slot);
        if let Some(over) = over
            && self.insert_drag != Some((from, over))
        {
            self.insert_drag = Some((from, over));
            self.tree.invalidate(PANEL);
        }
    }

    /// And drops it. The move is one edit, made where the mouse came up.
    fn drop_insert_row(&mut self) {
        let Some((from, to)) = self.insert_drag.take() else {
            return;
        };
        if from == to {
            return;
        }
        let strip = self.selected_track;
        if let Some(doc) = &mut self.options.document {
            doc.move_insert(strip, from, to);
        }
        // The editor addresses an insert by its slot, and the slot it was
        // showing has just moved.
        if let Some((open_strip, slot)) = self.open_insert
            && open_strip == strip
        {
            self.open_insert = Some((strip, reslot(slot, from, to)));
        }
        self.refresh_studio();
        self.refresh_title();
        self.tree.invalidate(PANEL);
    }

    fn drag_fader(&mut self, strip: usize, y: f32) {
        let Some(layout) = self.strip_layout(strip) else {
            return;
        };
        let db = fader_db_at(layout.fader, y);
        let current = self.mixer_strips.get(strip).map_or(0.0, |s| s.gain_db);
        // Nothing to say when the value has not moved: every one of these is a
        // command on the history and a set of atomic stores, and one per pixel
        // of a stationary mouse is waste.
        if (db - current).abs() < 0.001 {
            return;
        }
        if let Some(doc) = &mut self.options.document {
            doc.set_track_gain_db(strip, db);
        }
        self.refresh_title();
        self.tree.invalidate(PANEL);
    }

    fn drag_pan(&mut self, strip: usize, x: f32) {
        let Some(layout) = self.strip_layout(strip) else {
            return;
        };
        let pan = pan_at(layout.pan, x);
        let current = self.mixer_strips.get(strip).map_or(0.0, |s| s.pan);
        if (pan - current).abs() < 0.001 {
            return;
        }
        if let Some(doc) = &mut self.options.document {
            doc.set_track_pan(strip, pan);
        }
        self.refresh_title();
        self.tree.invalidate(PANEL);
    }

    /// Makes an automation lane for a track's fader or its pan.
    fn automate_track(&mut self, strip: usize, gain: bool) {
        let name = self
            .mixer_strips
            .get(strip)
            .map(|track| track.name.clone())
            .unwrap_or_default();
        let label = format!("{name} \u{2014} {}", if gain { "gain" } else { "pan" });
        let at = self.view.position_sample;
        let Some(doc) = &mut self.options.document else {
            return;
        };
        let Some(track) = doc.mixer_track_id(strip) else {
            return;
        };
        let address = if gain {
            fontelle_types::ParamTarget::TrackGain(track)
        } else {
            fontelle_types::ParamTarget::TrackPan(track)
        }
        .address();
        let at = doc.playhead_song_tick(at);
        doc.create_automation(&address, &label, at);
        self.lane_made();
    }

    /// The arrangement after a lane was made: it is drawn there, selected,
    /// and that is where it is edited — *"then it creates a new automation
    /// clip in my arrangement"*. Nothing opens.
    fn lane_made(&mut self) {
        self.refresh_studio();
        self.refresh_title();
        self.tree.invalidate(TIMELINE);
    }

    /// A press in the effect tab. Right-click makes an automation lane for
    /// whatever is under the pointer (§12.4).
    fn press_effect_editor(&mut self, button: MouseButton, x: f32, y: f32) {
        match button {
            MouseButton::Left => self.press_effect(x, y),
            MouseButton::Right => match crate::canvas::eq_hit(&self.eq_layout, x, y) {
                // A band's *gain* is what a right-click on its handle means:
                // it is the axis the handle moves vertically, and the one a
                // sweep is nearly always drawn on.
                crate::canvas::EqHit::Handle(band) | crate::canvas::EqHit::Band(band) => {
                    self.automate_insert_param(&format!("band{}.gain", band + 1));
                }
                // §12.4 taken at its word: every control in the row is a
                // parameter, so right-clicking any of them makes its lane —
                // the frequency, the Q, the type, the wet/dry.
                crate::canvas::EqHit::Field(field) => {
                    if let Some(param) = field.param(self.eq_band) {
                        self.automate_insert_param(&param);
                    }
                }
                crate::canvas::EqHit::Curve | crate::canvas::EqHit::Nothing => {}
            },
        }
    }

    /// Makes an automation lane for one parameter of the open insert.
    fn automate_insert_param(&mut self, param: &str) {
        self.automate_insert_named(param, param);
    }

    /// The same for the insert the effect window has open, which is the only
    /// one its own controls can belong to.
    fn automate_insert_named(&mut self, param: &str, caption: &str) {
        let Some((strip, slot)) = self.open_insert else {
            return;
        };
        self.automate_insert(strip, slot, param, caption);
    }

    /// The same, for an insert named directly — the mixer's rows can automate
    /// a control without the effect's window being open, which is §12.4's
    /// "right-click any control" taken at its word.
    /// `caption` is what the control is **called**; `param` is the effect's own
    /// id for it.
    ///
    /// Both, because they are two different things and the lane needs each: a
    /// lane named `mixer:4294967296/insert[0]/param/threshold` is one nobody
    /// can find on an arrangement, and a lane *addressed* by "Threshold" is one
    /// that reaches nothing. The effect's panel has both — it read the word out
    /// of the parameter's own spec — so it hands both over.
    fn automate_insert(&mut self, strip: usize, slot: usize, param: &str, caption: &str) {
        // Both worked out before the document is borrowed mutably.
        let name = self
            .mixer_strips
            .get(strip)
            .map_or_else(|| "Track".to_string(), |track| track.name.clone());
        let effect = self
            .mixer_strips
            .get(strip)
            .and_then(|track| track.inserts.get(slot))
            .map_or_else(String::new, |insert| insert.label.clone());
        let label = format!("{name} \u{2014} {effect} {caption}");
        let at = self.view.position_sample;
        let Some(doc) = &mut self.options.document else {
            return;
        };
        let Some(track) = doc.mixer_track_id(strip) else {
            return;
        };
        let address = fontelle_types::ParamTarget::Insert {
            track,
            slot,
            param: param.to_string(),
        }
        .address();
        let at = doc.playhead_song_tick(at);
        doc.create_automation(&address, &label, at);
        self.lane_made();
    }

    /// The half of a mouse-up that belongs to a *control* — a knob, a fader,
    /// an EQ band, an automation point.
    ///
    /// Its own function because an editor window's mouse-up needs exactly
    /// this and none of the roll's or the arrangement's: those are the main
    /// window's canvases, and releasing a marquee against a floating window's
    /// coordinates would catch whatever notes happened to be under the same
    /// numbers.
    fn release_controls(&mut self) {
        // A badge let go over a knob is where a route is made (§8.4) — the
        // *release* and not the press, because a drag that has not been let go
        // is a drag somebody can still call off by moving away.
        if matches!(self.drag, Drag::FlopAssign) {
            let (x, y) = self.cursor;
            self.drop_flop_assign(x, y);
        }
        self.flop_assign = None;
        self.flop_node = None;
        self.flop_ring = None;
        // A band or point drag coalesces into one history entry while it
        // runs; this is what tells the document it has stopped, the same
        // handshake a note drag has.
        // A band, a point or an audio clip's track: each coalesces into one
        // history entry while it runs, and this is what tells the document it
        // has stopped — the same handshake a note drag has. Without it the
        // next unrelated change to the clip would merge into the drag and
        // one Ctrl+Z would take both back.
        if matches!(self.drag, Drag::EqHandle(_) | Drag::AudioRow(_))
            && let Some(doc) = &mut self.options.document
        {
            doc.end_gesture();
        }
        self.drag = Drag::None;
        self.end_edge_scroll();
        self.dismissed = None;
        self.knob = None;
        self.insert_knob = None;
        self.value_drag = None;
        self.eq_drag = None;
        self.mix_drag = None;
        self.pointer_anchor = None;
        if let Some(doc) = &mut self.options.document {
            doc.end_gesture();
        }
    }

    // ------------------------------------------------ the time selection ---

    /// One step of a right-drag along the arrangement's ruler.
    fn drag_select_timeline(&mut self, x: f32) {
        let grid = self.timeline_layout.grid;
        let (x, _) = clamp_to_grid(grid, x, grid.y);
        let tick = timeline_x_to_tick(&self.timeline.view, grid, x);
        let snap = if self.modifiers.alt_key() {
            SnapDivision::None
        } else {
            self.timeline.view.snap
        };
        self.preview_selection(tick, snap);
    }

    /// The same along the roll's ruler, in the clip's ticks turned into the
    /// song's — so a loop drawn on either ruler is one loop on both.
    fn drag_select_roll(&mut self, x: f32) {
        let grid = self.roll_layout.grid;
        let (x, _) = clamp_to_grid(grid, x, grid.y);
        let snap = if self.modifiers.alt_key() {
            SnapDivision::None
        } else {
            self.roll.view.snap
        };
        let clip_tick = x_to_tick(&self.roll.view, grid, x);
        let Some(tick) = self
            .options
            .document
            .as_ref()
            .map(|doc| doc.song_tick_of_clip_tick(clip_tick))
        else {
            return;
        };
        self.preview_selection(tick, snap);
    }

    fn preview_selection(&mut self, tick: fontelle_types::Tick, snap: SnapDivision) {
        let beats = self.beats_per_bar();
        let picked = crate::canvas::time_selection(self.select_anchor, tick, snap, beats);
        if picked == self.select_preview {
            return;
        }
        self.select_preview = picked;
        // Shown on both rulers while it is drawn, in place of the loop the
        // document holds — what you are choosing, not what you had.
        self.loop_range = picked;
        self.tree.invalidate(TIMELINE);
        self.tree.invalidate(PANEL);
    }

    /// The button came up: what was dragged out is the loop now, and a drag
    /// that was a click clears it. One command, saved with the song.
    fn commit_selection(&mut self) {
        let picked = self.select_preview.take();
        if let Some(doc) = &mut self.options.document {
            doc.set_loop_range(picked);
            doc.end_gesture();
        }
        self.refresh_studio();
        self.refresh_title();
        self.tree.invalidate(TIMELINE);
        self.tree.invalidate(PANEL);
        self.tree.invalidate(TRANSPORT);
    }

    /// The song/clip chip: the other mode, and the marker brought inside the
    /// clip so play starts on it rather than somewhere the loop will pull it
    /// into a bar later.
    fn toggle_play_mode(&mut self) {
        let next = self.play_mode.next();
        let Some(doc) = &mut self.options.document else {
            return;
        };
        doc.set_play_mode(next);
        let span = doc.focused_clip_span();
        self.refresh_studio();
        if next == crate::document::PlayMode::Clip
            && let Some((from, to)) = span
            && let Some(doc) = &self.options.document
        {
            let marker_tick = doc.playhead_song_tick(self.marker);
            if marker_tick < from || marker_tick >= to {
                let sample = doc.sample_of_song_tick(from);
                self.mark(sample);
            }
        }
        self.status = match next {
            crate::document::PlayMode::Song => "Song mode: play plays the arrangement".to_string(),
            crate::document::PlayMode::Clip => {
                "Clip mode: play loops the clip you are editing".to_string()
            }
        };
        self.tick();
        self.tree.invalidate(TRANSPORT);
        self.tree.invalidate(TIMELINE);
        self.tree.invalidate(PANEL);
        self.tree.invalidate(BROWSER);
    }

    /// A press in the EQ editor.
    ///
    /// A press on empty curve **switches the nearest unused band on** where it
    /// landed, which is the gesture every EQ has: you point at the frequency
    /// you want changed and drag. The alternative — eight handles sitting on a
    /// flat line waiting to be found — is eight things to knock by accident to
    /// save one click.
    fn press_effect(&mut self, x: f32, y: f32) {
        use crate::canvas::EqHit;
        match crate::canvas::eq_hit(&self.eq_layout, x, y) {
            EqHit::Handle(band) => {
                // Picking a handle up also selects it, so the row of numbers
                // under the curve is about the band in your hand.
                self.select_eq_band(band);
                self.drag = Drag::EqHandle(band);
            }
            EqHit::Curve => {
                let Some(config) = self.eq else { return };
                let Some(band) = config.bands.iter().position(|band| !band.enabled) else {
                    // Eight is all there is. Saying nothing is better than
                    // silently rewriting one somebody set.
                    return;
                };
                let mut value = config.bands[band];
                value.enabled = true;
                value.band_type = fontelle_types::BandType::Bell;
                value.freq_hz = crate::canvas::eq_freq_at(self.eq_layout.curve, x);
                value.gain_db = crate::canvas::eq_gain_at(self.eq_layout.curve, y);
                self.select_eq_band(band);
                self.write_eq_band(band, value);
                // And the drag continues from the handle it just made, so
                // placing a band and aiming it are one gesture — the same
                // handshake drawing a note and sizing it has.
                self.drag = Drag::EqHandle(band);
            }
            // A chip switches its band on where it belongs and selects it.
            // This is what a flat EQ needs to be usable at all: eight bands
            // that are all off draw no handles, and a curve nobody knows to
            // click is *"uninteractable"*.
            EqHit::Band(band) => self.press_eq_chip(band),
            EqHit::Field(field) => self.press_eq_field(field, y),
            EqHit::Nothing => {}
        }
    }

    /// Points the controls under the curve at one band.
    fn select_eq_band(&mut self, band: usize) {
        if self.eq_band == band {
            return;
        }
        self.eq_band = band;
        self.relayout_editors();
        self.redraw_editor(EditorKind::Effect);
    }

    /// Writes one band and lets everything that draws it know.
    fn write_eq_band(&mut self, band: usize, value: fontelle_types::EqBand) {
        let Some((strip, slot)) = self.open_insert else {
            return;
        };
        if let Some(doc) = &mut self.options.document {
            doc.set_eq_band(strip, slot, band, value);
        }
        // `refresh_studio` re-reads the config and relays the editor out —
        // without the revision the studio bumps for this, the sound would
        // change and the curve would not. See `fontelle-app/tests/eq_editor.rs`.
        self.refresh_studio();
        self.refresh_title();
        self.redraw_editor(EditorKind::Effect);
        self.tree.invalidate(PANEL);
    }

    /// A press on one of the eight numbered chips.
    ///
    /// Selects that band, and switches it on where it belongs if it is off —
    /// which is the discoverable way in. Ctrl switches it back off, so the
    /// chip is the one control that both adds and removes.
    fn press_eq_chip(&mut self, band: usize) {
        let Some(config) = self.eq else { return };
        let Some(mut value) = config.bands.get(band).copied() else {
            return;
        };
        self.select_eq_band(band);
        if self.modifiers.control_key() {
            if !value.enabled {
                return;
            }
            value.enabled = false;
            value.solo = false;
        } else if !value.enabled {
            value.enabled = true;
            // Where nothing has moved it yet: eight bands stacked at 1 kHz
            // would be eight handles on top of each other.
            if value.freq_hz == fontelle_types::EqBand::new().freq_hz {
                value.freq_hz = crate::canvas::band_home_hz(band);
            }
        } else {
            // Already on and already selected: nothing to change. The press
            // has done its job by pointing the controls at it.
            return;
        }
        self.write_eq_band(band, value);
    }

    /// Switches a band off, which is what deleting one means: the eight bands
    /// are a fixed set and a band that is off has no handle, no curve and no
    /// effect on the sound.
    ///
    /// One function rather than three copies of the same two lines, because
    /// there are three ways in — the delete chip, Ctrl-clicking the band's
    /// number, and the Delete key inside the window.
    fn remove_eq_band(&mut self, band: usize) {
        let Some(config) = self.eq else { return };
        let Some(mut value) = config.bands.get(band).copied() else {
            return;
        };
        if !value.enabled {
            return;
        }
        value.enabled = false;
        value.solo = false;
        self.write_eq_band(band, value);
    }

    /// A press on one of the selected band's controls.
    fn press_eq_field(&mut self, field: crate::canvas::EqField, y: f32) {
        use crate::canvas::EqField;
        let Some(config) = self.eq else { return };
        let band = self.eq_band;
        let Some(mut value) = config.bands.get(band).copied() else {
            return;
        };
        // Ctrl steps a chooser backwards, the same modifier the browser's
        // settings rows and the roll's lane chip give it.
        let forward = !self.modifiers.control_key();
        match field {
            EqField::Type => {
                value.band_type = crate::canvas::next_band_type(value.band_type, forward);
                // Changing what a band *is* is a statement that you want it,
                // so it comes on rather than being changed invisibly.
                value.enabled = true;
                self.write_eq_band(band, value);
            }
            EqField::Channel => {
                value.channel = crate::canvas::next_band_channel(value.channel, forward);
                self.write_eq_band(band, value);
            }
            EqField::Solo => {
                value.solo = !value.solo;
                if value.solo {
                    value.enabled = true;
                }
                self.write_eq_band(band, value);
            }
            EqField::Delete => self.remove_eq_band(band),
            // The numbers are dragged, from where they are.
            EqField::Freq | EqField::Gain | EqField::Q | EqField::Mix => {
                let from = match field {
                    EqField::Freq => value.freq_hz,
                    EqField::Gain => value.gain_db,
                    EqField::Q => value.q,
                    _ => config.mix,
                };
                self.eq_drag = Some((from, y));
                self.drag = Drag::EqField(field);
            }
        }
    }

    /// One of the EQ's numbers, being dragged.
    ///
    /// Relative and from where the drag started, so a hand that wanders back
    /// to where it began puts the value back — the same rule the tempo box
    /// follows. Shift is the fine drag, as everywhere else in this window.
    fn drag_eq_field(&mut self, field: crate::canvas::EqField, y: f32) {
        use crate::canvas::EqField;
        let (Some((from, from_y)), Some(config)) = (self.eq_drag, self.eq) else {
            return;
        };
        let band = self.eq_band;
        let Some(mut value) = config.bands.get(band).copied() else {
            return;
        };
        // Up is more, which is the one direction nobody argues about.
        let steps = (from_y - y) / if self.modifiers.shift_key() { 8.0 } else { 2.0 };
        match field {
            EqField::Freq => value.freq_hz = crate::canvas::eq_nudge_freq(from, steps),
            EqField::Gain => value.gain_db = crate::canvas::eq_nudge_gain(from, steps),
            EqField::Q => value.q = crate::canvas::eq_nudge_q(from, steps),
            EqField::Mix => {
                let mix = crate::canvas::eq_nudge_mix(from, steps);
                if (mix - config.mix).abs() > 1e-4 {
                    self.set_open_insert_mix(mix);
                }
                return;
            }
            _ => return,
        }
        if config.bands.get(band) != Some(&value) {
            self.write_eq_band(band, value);
        }
    }

    /// Moves the open insert's wet/dry from inside its own window.
    fn set_open_insert_mix(&mut self, mix: f32) {
        let Some((strip, slot)) = self.open_insert else {
            return;
        };
        if let Some(doc) = &mut self.options.document {
            doc.set_insert_mix(strip, slot, mix);
        }
        self.refresh_studio();
        self.refresh_title();
        self.redraw_editor(EditorKind::Effect);
        self.tree.invalidate(PANEL);
    }

    /// The wheel over the EQ: the Q of the band under the pointer, or the
    /// control under it.
    ///
    /// Over a handle it is the Q, which is the one thing a handle cannot say
    /// by being dragged — it has two axes and three numbers.
    fn wheel_effect(&mut self, x: f32, y: f32, steps: f32) {
        use crate::canvas::{EqField, EqHit};
        let Some(config) = self.eq else { return };
        match crate::canvas::eq_hit(&self.eq_layout, x, y) {
            EqHit::Handle(band) | EqHit::Band(band) => {
                let Some(mut value) = config.bands.get(band).copied() else {
                    return;
                };
                value.q = crate::canvas::eq_nudge_q(value.q, steps);
                self.select_eq_band(band);
                self.write_eq_band(band, value);
            }
            EqHit::Field(field) => {
                let band = self.eq_band;
                let Some(mut value) = config.bands.get(band).copied() else {
                    return;
                };
                match field {
                    EqField::Freq => {
                        value.freq_hz = crate::canvas::eq_nudge_freq(value.freq_hz, steps)
                    }
                    EqField::Gain => {
                        value.gain_db = crate::canvas::eq_nudge_gain(value.gain_db, steps)
                    }
                    EqField::Q => value.q = crate::canvas::eq_nudge_q(value.q, steps),
                    EqField::Type => {
                        value.band_type =
                            crate::canvas::next_band_type(value.band_type, steps > 0.0);
                    }
                    EqField::Channel => {
                        value.channel =
                            crate::canvas::next_band_channel(value.channel, steps > 0.0);
                    }
                    EqField::Mix => {
                        let mix = crate::canvas::eq_nudge_mix(config.mix, steps * 2.0);
                        self.set_open_insert_mix(mix);
                        return;
                    }
                    EqField::Solo | EqField::Delete => return,
                }
                self.write_eq_band(band, value);
            }
            EqHit::Curve | EqHit::Nothing => {}
        }
    }

    /// Opens one insert in the effect tab.
    ///
    /// Switching the tab as well as selecting the slot, because clicking a
    /// three-letter row in a mixer strip means "show me this" and a click that
    /// selected something out of sight would be a click that did nothing.
    fn open_insert(&mut self, strip: usize, slot: usize) {
        let (config, view) = match &self.options.document {
            // An EQ draws its curve; everything else draws the grid of knobs
            // its own parameter list describes. Exactly one of the two is
            // `Some` for any insert that is there.
            Some(doc) => (doc.eq_config(strip, slot), doc.insert_view(strip, slot)),
            None => (None, None),
        };
        if config.is_none() && view.is_none() {
            // The slot is empty — the chain changed under a click. Nothing to
            // open, and nothing worth saying about it.
            return;
        }
        self.open_insert = Some((strip, slot));
        self.eq = config;
        self.insert_view = view;
        self.open_editor(EditorKind::Effect);
        self.relayout_editors();
    }

    /// What the effect panel calls what it is showing: the strip's name and
    /// the effect's, because "EQ" on its own does not say which track.
    fn effect_title(&self) -> String {
        // The panel names itself when it is one of the generic ones — it was
        // built from the effect's own list and knows which effect that was.
        if let Some(view) = &self.insert_view {
            return view.title.clone();
        }
        match self.open_insert {
            Some((strip, _)) => match self.mixer_strips.get(strip) {
                Some(track) => format!("{} \u{2014} EQ", track.name),
                None => "EQ".to_string(),
            },
            None => "EQ".to_string(),
        }
    }

    fn open_insert_bypassed(&self) -> bool {
        self.open_insert
            .and_then(|(strip, slot)| {
                self.mixer_strips
                    .get(strip)
                    .and_then(|track| track.inserts.get(slot))
            })
            .is_some_and(|insert| insert.bypassed)
    }

    /// Moves one band's handle to where the pointer is.
    ///
    /// Absolute rather than relative, unlike the tempo box and like the fader:
    /// the handle *is* the frequency and the gain, so it goes where it is put.
    fn drag_eq(&mut self, band: usize, x: f32, y: f32) {
        let (Some((strip, slot)), Some(config)) = (self.open_insert, self.eq) else {
            return;
        };
        let Some(mut value) = config.bands.get(band).copied() else {
            return;
        };
        value.freq_hz = crate::canvas::eq_freq_at(self.eq_layout.curve, x);
        if value.band_type.uses_gain() {
            value.gain_db = crate::canvas::eq_gain_at(self.eq_layout.curve, y);
        }
        if let Some(doc) = &mut self.options.document {
            doc.set_eq_band(strip, slot, band, value);
        }
        self.refresh_studio();
        self.refresh_title();
        self.relayout_panels();
        self.tree.invalidate(PANEL);
    }

    /// Where a strip is, whether it is one of the scrolling ones or the master.
    fn strip_layout(&self, strip: usize) -> Option<crate::canvas::MixerStripLayout> {
        self.mixer
            .strips
            .iter()
            .chain(self.mixer.master.iter())
            .find(|s| s.index == strip)
            .cloned()
    }

    // ---------------------------------------------- the tempo and the metre ---

    fn drag_tempo(&mut self, y: f32) {
        let Some((from_value, from_y)) = self.value_drag else {
            return;
        };
        let bpm = tempo_at(from_value, y - from_y, self.modifiers.shift_key());
        if (bpm - self.tempo).abs() < 1e-9 {
            return;
        }
        self.set_tempo(bpm);
    }

    fn set_tempo(&mut self, bpm: f64) {
        if let Some(doc) = &mut self.options.document {
            doc.set_tempo(bpm);
        }
        self.refresh_title();
        // Every tick is a different sample now, so the playhead and both
        // rulers have moved even though nothing was played.
        self.tree.invalidate(TRANSPORT);
        self.tree.invalidate(TIMELINE);
        self.tree.invalidate(PANEL);
    }

    fn set_beats_per_bar(&mut self, beats: u32) {
        if let Some(doc) = &mut self.options.document {
            doc.set_beats_per_bar(beats);
            doc.end_gesture();
        }
        self.refresh_title();
        self.tree.invalidate(TRANSPORT);
        self.tree.invalidate(TIMELINE);
        self.tree.invalidate(PANEL);
    }

    // ------------------------------------------------- the instrument editor ---

    // ---------------------------------------------------- the preset bar ---

    /// Which device the editor window of `kind` is showing, when it is showing
    /// one a preset can be loaded onto.
    ///
    /// The audio clip editor is `None` and always will be: its subject is a
    /// *clip*, and a clip is not a device — it has no state a preset could be.
    fn preset_device(&self, kind: EditorKind) -> Option<crate::canvas::PresetDevice> {
        match kind {
            EditorKind::Instrument => Some(crate::canvas::PresetDevice::Instrument),
            EditorKind::Effect => self
                .open_insert
                .map(|(strip, slot)| crate::canvas::PresetDevice::Insert { strip, slot }),
            EditorKind::AudioClip => None,
        }
    }

    /// Where this window keeps its bar's view: the instrument window's is 0,
    /// the effect window's is 1.
    fn preset_slot(kind: EditorKind) -> Option<usize> {
        match kind {
            EditorKind::Instrument => Some(0),
            EditorKind::Effect => Some(1),
            EditorKind::AudioClip => None,
        }
    }

    /// Where the bar's controls are in this window's header.
    ///
    /// Recomputed rather than kept, because it depends on how wide the
    /// window's *title* came out — and that changes whenever the channel is
    /// renamed, which is exactly what loading a preset does.
    fn preset_bar_geometry(&self, kind: EditorKind) -> Option<crate::canvas::PresetBarLayout> {
        let slot = Self::preset_slot(kind)?;
        let editor = self.editors.iter().find(|e| e.kind == kind)?;
        Some(crate::canvas::preset_bar_layout(
            editor.panel.header,
            // The title, plus the padding either side of it, plus a gap: the
            // bar starts where the name has finished rather than on top of it.
            self.options.theme.metrics.panel_padding * 2.0 + editor.title.width + 12.0,
            &self.preset_view[slot],
            &self.options.theme.metrics,
        ))
    }

    /// The preset drop-down's rows for one window, and what each one means.
    ///
    /// A pair, like every other menu here (`favorites.rs`): the window builds
    /// the menu from one half and answers a press from the other, so the two
    /// cannot come to disagree about which row was the pad.
    fn preset_menu_rows(
        &self,
        kind: EditorKind,
    ) -> (
        Vec<crate::canvas::MenuEntry>,
        Vec<crate::canvas::PresetMenuRow>,
    ) {
        let choices = match (self.preset_device(kind), self.options.document.as_ref()) {
            (Some(device), Some(doc)) => doc.preset_choices(device),
            _ => Vec::new(),
        };
        crate::canvas::preset_menu(&choices, &self.menu_filter)
    }

    fn preset_categories_for(&self, kind: EditorKind) -> Vec<String> {
        match (self.preset_device(kind), self.options.document.as_ref()) {
            (Some(device), Some(doc)) => doc.preset_categories(device),
            _ => Vec::new(),
        }
    }

    /// Finishes a "Save as…": the name and the category are both in hand.
    fn finish_save_as(&mut self, kind: EditorKind, category: &str) {
        let Some((_, name)) = self.pending_save_as.take() else {
            return;
        };
        if let (Some(device), Some(doc)) =
            (self.preset_device(kind), self.options.document.as_mut())
        {
            doc.save_preset_as(device, &name, category);
        }
        self.after_preset_change();
    }

    /// A press on the bar. `true` when it was one.
    fn press_preset_bar(&mut self, kind: EditorKind, x: f32, y: f32) -> bool {
        use crate::canvas::PresetBarHit;
        let (Some(slot), Some(layout), Some(device)) = (
            Self::preset_slot(kind),
            self.preset_bar_geometry(kind),
            self.preset_device(kind),
        ) else {
            return false;
        };
        let Some(hit) = crate::canvas::preset_bar_hit(&layout, &self.preset_view[slot], x, y)
        else {
            return false;
        };
        match hit {
            PresetBarHit::Previous | PresetBarHit::Next => {
                let delta = match hit {
                    PresetBarHit::Previous => -1,
                    _ => 1,
                };
                if let Some(doc) = &mut self.options.document {
                    doc.step_preset(device, delta);
                }
                self.after_preset_change();
            }
            // Both open the same list: "show me the others like this" and
            // "show me the list" are the same list.
            PresetBarHit::Name | PresetBarHit::Category => {
                let bounds = self
                    .editors
                    .iter()
                    .find(|e| e.kind == kind)
                    .map(|e| e.panel.frame)
                    .unwrap_or(self.layout.window);
                self.open_menu(MenuTarget::PresetMenu(kind), x, y, bounds);
            }
            PresetBarHit::Star => {
                if let Some(doc) = &mut self.options.document {
                    doc.toggle_preset_favorite(device);
                }
                self.after_preset_change();
            }
            PresetBarHit::Save => {
                if let Some(doc) = &mut self.options.document {
                    doc.save_preset(device);
                }
                self.after_preset_change();
            }
            PresetBarHit::SaveAs => {
                let seed = self.preset_view[slot].name.clone().unwrap_or_default();
                self.ask_in_editor(MenuTarget::PresetSaveName(kind), kind, seed);
            }
        }
        true
    }

    /// Opens a prompt or a list **in an editor window**, in the middle of it.
    ///
    /// `ask_for_a_name`'s reasoning, one window along: the two gestures that
    /// reach this are a button in a header and a key, and a prompt that
    /// appeared under the pointer for one and in the middle for the other
    /// would be two different features.
    fn ask_in_editor(&mut self, target: MenuTarget, kind: EditorKind, seed: String) {
        self.dismissed = None;
        let bounds = self
            .editors
            .iter()
            .find(|e| e.kind == kind)
            .map(|e| e.panel.frame)
            .unwrap_or(self.layout.window);
        let (x, y) = (
            bounds.x + bounds.width / 2.0,
            bounds.y + bounds.height / 3.0,
        );
        self.open_menu(target, x, y, bounds);
        if self.menu.is_some() && !seed.is_empty() {
            self.menu_filter = seed;
            self.relayout_menu();
        }
    }

    /// After anything that loads, saves or stars a preset: the document moved,
    /// the bar has to be read again, and both windows redraw.
    fn after_preset_change(&mut self) {
        self.studio_revision = u64::MAX;
        self.refresh_studio();
        self.refresh_title();
        self.tree.invalidate(RACK);
        self.tree.invalidate(PANEL);
        self.redraw_editors();
    }

    /// Every editor window redraws. Used where a change is one window's
    /// gesture and another window's subject — a preset loaded on a channel
    /// moves the rack, the panel and the synth's own window at once.
    fn redraw_editors(&mut self) {
        for editor in &self.editors {
            editor.window.request_redraw();
        }
    }

    /// A press on the instrument panel.
    ///
    /// A knob starts a drag from where it was grabbed; a switch or a choice
    /// steps on the press, because there is nothing to drag.
    fn press_instrument(&mut self, x: f32, y: f32) {
        let Some(which) = instrument_hit(&self.instrument_layout, x, y) else {
            return;
        };
        let Some(param) = self
            .instrument
            .as_ref()
            .and_then(|view| view.param(which.0, which.1))
            .cloned()
        else {
            return;
        };
        match param.kind {
            ParamKind::Knob => {
                self.knob = Some((which, y, param.value));
                self.drag = Drag::Knob;
                self.tree.invalidate(PANEL);
            }
            // One of a list is a **drop-down**, not a step: with six options
            // in it, reaching the one you want was five presses through five
            // sounds you did not ask for.
            ParamKind::Choice(_) => self.open_param_choice(EditorKind::Instrument, which),
            ref kind => {
                let value = next_value(kind, param.value);
                self.set_param(&param.address, value);
                if let Some(doc) = &mut self.options.document {
                    doc.end_gesture();
                }
            }
        }
    }

    /// The control at `which` on Flopsynth's window, if it is still there.
    ///
    /// Looked up each time rather than held: the view is rebuilt whenever the
    /// document moves, and a control cached across that is a control that has
    /// been replaced.
    fn flop_param(&self, which: (usize, usize)) -> Option<&crate::canvas::InstrumentParam> {
        self.flopsynth
            .as_ref()?
            .cards
            .get(which.0)?
            .group
            .params
            .get(which.1)
    }

    /// A press on Flopsynth's window.
    ///
    /// The instrument panel's rules exactly — a knob is dragged from where it
    /// was grabbed, a switch flips, a chooser opens a list — because a knob
    /// has to mean the same thing in both windows. What is different is only
    /// which layout answers "what is under the pointer".
    fn press_flopsynth(&mut self, x: f32, y: f32) {
        // The page tabs, the badges and the matrix are chrome around the cards
        // and are checked first — each is somewhere no card is, so the order
        // is about reading the code rather than about resolving a conflict.
        if let Some(page) = crate::canvas::flopsynth_tab_at(&self.flopsynth_layout, x, y) {
            self.show_flop_page(page);
            return;
        }
        if let Some(hit) = crate::canvas::presets_hit(&self.flopsynth_layout, x, y) {
            self.press_flop_presets(hit);
            return;
        }
        if let Some(source) = crate::canvas::badge_at(&self.flopsynth_layout, x, y) {
            self.flop_assign = Some((source, (x, y)));
            self.drag = Drag::FlopAssign;
            self.tree.invalidate(PANEL);
            return;
        }
        if let Some(hit) = crate::canvas::matrix_hit(&self.flopsynth_layout, x, y) {
            self.press_flop_matrix(hit, x);
            return;
        }
        let hit = crate::canvas::flopsynth_hit(&self.flopsynth_layout, x, y);
        // A picture is dragged, not turned (§8.7). Checked before the control
        // branch below because a picture is never *also* a control — the hit
        // test has already decided which of the two is under the pointer.
        if let Some(crate::canvas::FlopsynthHit::Picture { card }) = hit {
            self.press_flop_picture(card, x, y);
            return;
        }
        // The chain is edited from its page (§8.5): the list drops under the
        // button, and a card's ✕ takes its slot off.
        if let Some(crate::canvas::FlopsynthHit::AddEffect) = hit {
            let button = self.flopsynth_layout.add_effect;
            let bounds = self
                .editors
                .iter()
                .find(|e| e.kind == EditorKind::Instrument)
                .map(|e| e.panel.frame)
                .unwrap_or(self.layout.window);
            self.open_menu(
                MenuTarget::AddPatchEffect,
                button.x,
                button.bottom(),
                bounds,
            );
            return;
        }
        if let Some(crate::canvas::FlopsynthHit::Remove { card }) = hit {
            if let (Some(slot), Some(doc)) =
                (self.flop_fx_slot(card), self.options.document.as_mut())
            {
                doc.remove_patch_effect(slot);
            }
            self.after_flop_structure();
            return;
        }
        let Some(crate::canvas::FlopsynthHit::Control { card, param }) = hit else {
            return;
        };
        let which = (card, param);
        // The modulation ring sits **outside** the knob's groove, so a press
        // that lands on it is a depth and not a value — see `ring_hit`, which
        // is where that band is defined.
        if let Some(cell) = self.flop_cell(which)
            && crate::canvas::ring_hit(cell, x, y)
            && let Some((depth, _)) = self.route_depth(which)
        {
            self.flop_ring = Some((which, y, depth));
            self.drag = Drag::FlopRing;
            self.tree.invalidate(PANEL);
            return;
        }
        let Some(control) = self.flop_param(which).cloned() else {
            return;
        };
        match control.kind {
            ParamKind::Knob => {
                self.flop_knob = Some((which, y, control.value));
                self.drag = Drag::FlopKnob;
                self.tree.invalidate(PANEL);
            }
            ParamKind::Choice(_) => self.open_param_choice(EditorKind::Instrument, which),
            ref kind => {
                let value = next_value(kind, control.value);
                self.set_param(&control.address, value);
                if let Some(doc) = &mut self.options.document {
                    doc.end_gesture();
                }
            }
        }
    }

    /// A press on the Presets page (§8.6).
    ///
    /// A row is `ApplyPreset` — the same command the bar's drop-down sends —
    /// and a star is the favourite the bar's star is: the page invents no
    /// mechanism, it is a second view over the bank.
    fn press_flop_presets(&mut self, hit: crate::canvas::PresetsHit) {
        use crate::canvas::{PresetDevice, PresetsHit};
        match hit {
            PresetsHit::Shelf(index) => {
                let shelf = self
                    .flopsynth
                    .as_ref()
                    .map(|view| crate::canvas::preset_shelves(&view.bank))
                    .and_then(|shelves| shelves.get(index).cloned());
                if let Some(shelf) = shelf {
                    let mut browse = self.flop_browse.clone();
                    browse.shelf = shelf;
                    browse.scroll = 0.0;
                    self.set_flop_browse(browse);
                }
            }
            // Typing goes to the search whenever the page is showing; the box
            // is there to say so, and a press on it has nothing to add.
            PresetsHit::Search => {}
            PresetsHit::Row(which) => {
                if let Some(doc) = self.options.document.as_mut() {
                    doc.apply_preset(PresetDevice::Instrument, which);
                }
                self.after_preset_change();
            }
            PresetsHit::Star(which) => {
                if let Some(doc) = self.options.document.as_mut() {
                    doc.toggle_preset_star(PresetDevice::Instrument, which);
                }
                self.after_preset_change();
            }
        }
    }

    /// Changes how the Presets page is looked at, and lays it out again.
    ///
    /// The scroll is read back after the layout, which is the only thing that
    /// knows how long the list came out — so a shelf with three rows in it
    /// cannot be left scrolled to where a shelf of a hundred was.
    fn set_flop_browse(&mut self, browse: crate::canvas::PresetBrowse) {
        self.flop_browse = browse.clone();
        if let Some(view) = &mut self.flopsynth {
            view.browse = browse;
        }
        self.relayout_editors();
        let max = self.flopsynth_layout.presets.max_scroll();
        if self.flop_browse.scroll > max {
            self.flop_browse.scroll = max;
            if let Some(view) = &mut self.flopsynth {
                view.browse.scroll = max;
            }
            self.relayout_editors();
        }
        self.redraw_editors();
    }

    /// The About column's lines: the loaded preset, described.
    fn flop_about(&self) -> Vec<String> {
        let Some(view) = &self.flopsynth else {
            return Vec::new();
        };
        crate::canvas::preset_about(
            &self.preset_view[0],
            self.flopsynth_layout.presets.rows.len(),
            view.bank.len(),
        )
    }

    /// A key while the Presets page is showing: it types into the search.
    ///
    /// The search box's own rule, and the plugin picker's: while a box that
    /// takes text is showing, a typed "d" is a letter and not a shortcut.
    /// Escape clears what was typed before it closes anything.
    fn flop_search_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        use winit::keyboard::{Key, NamedKey};
        let mut browse = self.flop_browse.clone();
        match &event.logical_key {
            Key::Named(NamedKey::Escape) if !browse.query.is_empty() => browse.query.clear(),
            Key::Named(NamedKey::Backspace) => {
                browse.query.pop();
            }
            Key::Named(NamedKey::Space) => browse.query.push(' '),
            Key::Character(text) if !self.modifiers.control_key() => {
                let typed: String = text.chars().filter(|c| !c.is_control()).collect();
                if typed.is_empty() {
                    return false;
                }
                browse.query.push_str(&typed);
            }
            _ => return false,
        }
        browse.scroll = 0.0;
        self.set_flop_browse(browse);
        true
    }

    /// The wheel over Flopsynth's window: the Presets list scrolls, and a
    /// knob nudges by a fiftieth of its travel — a three-hundredth with Shift
    /// (§8.7).
    fn wheel_flopsynth(&mut self, x: f32, y: f32, steps: f32) {
        if self.flopsynth.is_none() {
            return;
        }
        let list = self.flopsynth_layout.presets.list;
        if !list.is_empty() && list.contains(x, y) {
            let mut browse = self.flop_browse.clone();
            browse.scroll =
                (browse.scroll - steps * crate::canvas::PRESET_ROW * MENU_WHEEL_ROWS).max(0.0);
            self.set_flop_browse(browse);
            return;
        }
        if let Some(crate::canvas::FlopsynthHit::Control { card, param }) =
            crate::canvas::flopsynth_hit(&self.flopsynth_layout, x, y)
            && let Some(control) = self.flop_param((card, param)).cloned()
            && control.kind == ParamKind::Knob
        {
            let step = if self.modifiers.shift_key() {
                1.0 / 300.0
            } else {
                0.02
            };
            let value = (control.value + steps * step).clamp(0.0, 1.0);
            self.set_param(&control.address, value);
            if let Some(doc) = &mut self.options.document {
                doc.end_gesture();
            }
        }
    }

    /// Which slot of the chain the card at `card` is, read off its first
    /// control's address (`patch/fx[n]/…`) rather than off its heading, because
    /// the address is the stable name (INVARIANT 7) and the heading is prose.
    fn flop_fx_slot(&self, card: usize) -> Option<usize> {
        let address = self
            .flopsynth
            .as_ref()?
            .cards
            .get(card)?
            .group
            .params
            .first()?
            .address
            .as_str()
            .to_string();
        address
            .strip_prefix("patch/fx[")?
            .split(']')
            .next()?
            .parse()
            .ok()
    }

    /// The kinds the `+ effect` list offers.
    fn patch_effect_kinds(&self) -> Vec<fontelle_types::EffectKind> {
        self.options
            .document
            .as_ref()
            .map(|doc| doc.patch_effect_kinds())
            .unwrap_or_default()
    }

    /// Where one of Flopsynth's controls is drawn.
    fn flop_cell(&self, which: (usize, usize)) -> Option<crate::layout::Rect> {
        self.flopsynth_layout
            .cards
            .get(which.0)?
            .cells
            .iter()
            .find(|(param, _)| *param == which.1)
            .map(|(_, cell)| *cell)
    }

    /// The depth of the newest route to this control, if anything modulates
    /// it.
    ///
    /// **The newest**, which is §8.4's rule for the ring: with two or more
    /// routes the ring edits the last one added and the tooltip says to use
    /// the matrix. A control with none has no ring to press.
    fn route_depth(&self, which: (usize, usize)) -> Option<(f32, fontelle_types::ParamAddress)> {
        let address = self.flop_param(which)?.address.clone();
        let doc = self.options.document.as_ref()?;
        doc.routes_to(&address)
            .last()
            .map(|route| (route.depth, route.depth_address.clone()))
    }

    /// A press on one of the pictures.
    fn press_flop_picture(&mut self, card: usize, x: f32, y: f32) {
        use crate::canvas::FlopsynthPicture;
        let Some(picture) = self
            .flopsynth_layout
            .cards
            .get(card)
            .map(|placed| placed.picture)
        else {
            return;
        };
        let Some(kind) = self
            .flopsynth
            .as_ref()
            .and_then(|view| view.cards.get(card))
            .map(|card| card.picture.clone())
        else {
            return;
        };
        match kind {
            FlopsynthPicture::Wave { .. } => {
                self.drag = Drag::FlopWave(card);
                self.drag_flop_wave(card, x);
            }
            FlopsynthPicture::Response { .. } => {
                self.drag = Drag::FlopResponse(card);
                self.drag_flop_response(card, x, y);
            }
            FlopsynthPicture::Envelope {
                attack,
                decay,
                sustain,
                release,
            } => {
                let Some(node) =
                    crate::canvas::env_node_at(picture, attack, decay, sustain, release, x, y)
                else {
                    return;
                };
                let from = match node {
                    crate::canvas::EnvNode::Attack => attack,
                    crate::canvas::EnvNode::Decay => decay,
                    crate::canvas::EnvNode::Sustain => sustain,
                    crate::canvas::EnvNode::Release => release,
                };
                self.flop_node = Some((card, node, (x, y), from));
                self.drag = Drag::FlopEnvNode;
            }
            // An LFO's picture is a read-out: its shape is a chooser and its
            // rate is a knob, and there is nothing in the drawing to aim at.
            FlopsynthPicture::Lfo { .. } | FlopsynthPicture::None => {}
        }
        self.tree.invalidate(PANEL);
    }

    /// Writes the control a picture gesture moves, by the tail of its address.
    fn set_flop_picture_param(&mut self, card: usize, tail: &str, value: f32) {
        let Some(param) = self
            .flopsynth
            .as_ref()
            .and_then(|view| view.cards.get(card))
            .and_then(|card| crate::canvas::picture_control(card, tail))
        else {
            return;
        };
        let Some(address) = self.flop_param((card, param)).map(|p| p.address.clone()) else {
            return;
        };
        self.set_param(&address, value);
    }

    fn drag_flop_wave(&mut self, card: usize, x: f32) {
        let Some(picture) = self.flopsynth_layout.cards.get(card).map(|c| c.picture) else {
            return;
        };
        let position = crate::canvas::wave_position_at(picture, x);
        self.set_flop_picture_param(card, "position", position);
    }

    fn drag_flop_response(&mut self, card: usize, x: f32, y: f32) {
        let Some(picture) = self.flopsynth_layout.cards.get(card).map(|c| c.picture) else {
            return;
        };
        let (cutoff, resonance) = crate::canvas::filter_xy_at(picture, x, y);
        self.set_flop_picture_param(card, "cutoff", cutoff);
        self.set_flop_picture_param(card, "resonance", resonance);
    }

    fn drag_flop_env_node(&mut self, x: f32, y: f32) {
        let Some((card, node, from, value)) = self.flop_node else {
            return;
        };
        let Some(picture) = self.flopsynth_layout.cards.get(card).map(|c| c.picture) else {
            return;
        };
        let moved = crate::canvas::env_node_drag(picture, node, value, x - from.0, y - from.1);
        self.set_flop_picture_param(card, node.stage(), moved);
    }

    fn drag_flop_ring(&mut self, y: f32) {
        let Some((which, from_y, from)) = self.flop_ring else {
            return;
        };
        let Some((_, address)) = self.route_depth(which) else {
            return;
        };
        let depth = crate::canvas::ring_depth(from, y - from_y);
        // A depth is bipolar and every parameter on the wire is 0..=1, so the
        // ring hands over the same number the matrix row's own slider would —
        // and the write goes down the ordinary live wire, because a modulation
        // depth is a knob like any other.
        self.set_param(&address, (depth + 1.0) * 0.5);
    }

    /// Shows one of Flopsynth's pages.
    fn show_flop_page(&mut self, page: crate::canvas::FlopsynthPage) {
        if self.flop_page == page {
            return;
        }
        self.flop_page = page;
        // The view is per page — the cards are filtered to it by the host — so
        // the studio's lists have to be read again rather than redrawn.
        self.studio_revision = u64::MAX;
        self.refresh_studio();
        self.redraw_editors();
    }

    /// A press on one of the matrix's rows.
    fn press_flop_matrix(&mut self, hit: crate::canvas::MatrixHit, x: f32) {
        use crate::canvas::MatrixHit;
        match hit {
            MatrixHit::Remove(index) => {
                let address = self.route_address(index);
                if let (Some(address), Some(doc)) = (address, self.options.document.as_mut()) {
                    doc.remove_route(&address, 0);
                }
                self.after_flop_structure();
            }
            MatrixHit::Depth(index) => {
                self.drag = Drag::FlopMatrix(index);
                self.drag_flop_matrix(index, x);
            }
        }
    }

    /// The destination address of the matrix row at `index`.
    ///
    /// The rows are the *whole* matrix in order, and `remove_route` counts the
    /// routes to one destination — so this hands over the destination and the
    /// position of this route among the routes to it.
    fn route_address(&self, index: usize) -> Option<fontelle_types::ParamAddress> {
        let route = self.flopsynth.as_ref()?.routes.get(index)?;
        // The row carries the destination's *label*, and the depth knob the
        // panel already draws carries its address — matched here rather than
        // stored twice.
        let doc = self.options.document.as_ref()?;
        let _ = route;
        let _ = doc;
        self.flop_route_depth_address(index)
    }

    /// The address of the depth control of the matrix row at `index`.
    fn flop_route_depth_address(&self, index: usize) -> Option<fontelle_types::ParamAddress> {
        Some(fontelle_types::ParamAddress::new(format!(
            "patch/mod[{index}]/depth"
        )))
    }

    fn drag_flop_matrix(&mut self, index: usize, x: f32) {
        let Some(row) = self.flopsynth_layout.routes.get(index).copied() else {
            return;
        };
        let Some(address) = self.flop_route_depth_address(index) else {
            return;
        };
        let depth = crate::canvas::matrix_depth_at(row.depth, x);
        self.set_param(&address, (depth + 1.0) * 0.5);
    }

    /// Finishes a drag-to-assign: the badge was let go over `(x, y)`.
    fn drop_flop_assign(&mut self, x: f32, y: f32) {
        let Some((source, _)) = self.flop_assign.take() else {
            return;
        };
        let landed = match crate::canvas::flopsynth_hit(&self.flopsynth_layout, x, y) {
            Some(crate::canvas::FlopsynthHit::Control { card, param }) => Some((card, param)),
            _ => None,
        };
        let address = landed.and_then(|which| self.flop_param(which).map(|p| p.address.clone()));
        // Released anywhere that is not a destination adds nothing — which is
        // how a drag is called off, and why the knobs light up during one.
        if let (Some(address), Some(doc)) = (address, self.options.document.as_mut())
            && doc.is_mod_destination(&address)
        {
            doc.add_route(source, &address);
            self.after_flop_structure();
            return;
        }
        self.tree.invalidate(PANEL);
        self.redraw_editors();
    }

    /// After a route is added or removed: the matrix changed shape, so the
    /// view has to be built again rather than redrawn.
    fn after_flop_structure(&mut self) {
        self.studio_revision = u64::MAX;
        self.refresh_studio();
        self.tree.invalidate(PANEL);
        self.redraw_editors();
    }

    /// A right-click on one of Flopsynth's knobs.
    ///
    /// *"i want to be able to right click on a knob and select create
    /// automation clip."* §12.4's rule, on this window too — and through the
    /// **same** menu target, because it is the same request about the same
    /// kind of thing and a second one would be a second thing to keep in step.
    fn press_flopsynth_menu(&mut self, x: f32, y: f32) {
        let Some(crate::canvas::FlopsynthHit::Control { card, param }) =
            crate::canvas::flopsynth_hit(&self.flopsynth_layout, x, y)
        else {
            return;
        };
        let Some(control) = self.flop_param((card, param)).cloned() else {
            return;
        };
        let target = MenuTarget::InstrumentParam {
            address: control.address.clone(),
            name: control.label.clone(),
        };
        let bounds = self
            .editors
            .iter()
            .find(|e| e.kind == EditorKind::Instrument)
            .map(|editor| editor.panel.body)
            .unwrap_or(self.flopsynth_layout.body);
        self.open_menu(target, x, y, bounds);
        self.redraw_editor(EditorKind::Instrument);
    }

    fn drag_flop_knob(&mut self, y: f32) {
        let Some((which, from_y, from_value)) = self.flop_knob else {
            return;
        };
        // Shift is a fine drag, the same modifier every other knob here has.
        let value = knob_value(from_value, y - from_y, self.modifiers.shift_key());
        let Some(control) = self.flop_param(which) else {
            return;
        };
        // Nothing to say when the value has not moved a step: a stationary
        // mouse must not write to the document once a pixel.
        if (value - control.value).abs() < 0.001 {
            return;
        }
        let address = control.address.clone();
        self.set_param(&address, value);
    }

    /// A press on the grid of knobs an effect that is not an EQ opens.
    ///
    /// The instrument panel's rules exactly — a knob is dragged, a switch and a
    /// chooser are clicked, and the right button offers an automation lane —
    /// because it is the same panel with a different list in it.
    fn press_insert_panel(&mut self, button: MouseButton, x: f32, y: f32) {
        // The key row, above the controls and not among them: a click that
        // lands on a chip is a choice rather than a knob. The first chip is
        // "no key", which is why
        // the index is shifted by one on the way out.
        if let Some(chip) = crate::canvas::instrument_key_hit(&self.insert_layout, x, y) {
            if button == MouseButton::Left {
                self.choose_insert_key(chip.checked_sub(1));
            }
            return;
        }
        let Some(which) = instrument_hit(&self.insert_layout, x, y) else {
            return;
        };
        let Some(param) = self
            .insert_view
            .as_ref()
            .and_then(|view| view.param(which.0, which.1))
            .cloned()
        else {
            return;
        };
        if button == MouseButton::Right {
            // §12.4 taken at its word on the panel that did not exist: every
            // control in it is a parameter, so every one of them can become a
            // lane. `automate_insert` is the same path the mixer's rows take.
            let bounds = self
                .editors
                .iter()
                .find(|e| e.kind == EditorKind::Effect)
                .map(|editor| editor.panel.body)
                .unwrap_or(self.insert_layout.body);
            self.open_menu(
                MenuTarget::InsertParam {
                    param: insert_param_id(param.address.as_str()),
                    name: param.label.clone(),
                },
                x,
                y,
                bounds,
            );
            self.redraw_editor(EditorKind::Effect);
            return;
        }
        match param.kind {
            ParamKind::Knob => {
                self.insert_knob = Some((which, y, param.value));
                self.drag = Drag::InsertKnob;
            }
            ParamKind::Choice(_) => self.open_param_choice(EditorKind::Effect, which),
            ref kind => {
                let value = next_value(kind, param.value);
                self.write_insert_param(param.address.as_str(), value);
                if let Some(doc) = &mut self.options.document {
                    doc.end_gesture();
                }
            }
        }
        self.redraw_editor(EditorKind::Effect);
    }

    fn drag_insert_knob(&mut self, y: f32) {
        let Some((which, from_y, from_value)) = self.insert_knob else {
            return;
        };
        // Shift is a fine drag, the same modifier every other knob gives it.
        let value = knob_value(from_value, y - from_y, self.modifiers.shift_key());
        let Some(param) = self
            .insert_view
            .as_ref()
            .and_then(|view| view.param(which.0, which.1))
            .cloned()
        else {
            return;
        };
        // Nothing to say when the value has not moved a step: every one of
        // these is a command on the history and a publish to the audio thread.
        if (value - param.value).abs() < 0.001 {
            return;
        }
        self.write_insert_param(param.address.as_str(), value);
    }

    // `choose_instrument_preset` and `choose_insert_preset` were here: the
    // two chip rows' presses. A preset is a file now and the bar in the
    // window's header chooses one for every device (§P.9), so both are one
    // function — `press_preset_bar`.

    /// Points this insert's detector at a strip, or at nothing.
    ///
    /// A **routing** change, so it goes through the document — which refuses a
    /// key that would make the graph feed itself, and a track keying an insert
    /// on itself — and then rebuilds the graph, because what changed is the
    /// order the schedule runs in.
    fn choose_insert_key(&mut self, key: Option<usize>) {
        let Some((strip, slot)) = self.open_insert else {
            return;
        };
        if let Some(doc) = &mut self.options.document {
            doc.set_insert_key(strip, slot, key);
            doc.end_gesture();
        }
        self.refresh_studio();
        self.refresh_title();
        self.redraw_editor(EditorKind::Effect);
    }

    fn write_insert_param(&mut self, param: &str, value: f32) {
        let Some((strip, slot)) = self.open_insert else {
            return;
        };
        let param = param.to_string();
        if let Some(doc) = &mut self.options.document {
            doc.set_insert_param(strip, slot, &param, value);
        }
        // The same reason the EQ refreshes here: the studio bumps its revision
        // for this, and without re-reading it the sound would change and the
        // panel would not.
        self.refresh_studio();
        self.refresh_title();
        self.redraw_editor(EditorKind::Effect);
    }

    /// A right-click on the instrument panel: a menu about the control under
    /// the pointer.
    ///
    /// Two of its controls are addressable — the channel's own level and its
    /// placement, which is what [`fontelle_types::ParamTarget`] knows how to
    /// name — and the patch's own knobs are not yet (TDD §8.2). Both open a
    /// menu, because a right-click that does nothing cannot be told from one
    /// that missed; the second one's entry is greyed and says why.
    fn press_instrument_menu(&mut self, x: f32, y: f32) {
        let Some(which) = instrument_hit(&self.instrument_layout, x, y) else {
            return;
        };
        let Some(param) = self
            .instrument
            .as_ref()
            .and_then(|view| view.param(which.0, which.1))
            .cloned()
        else {
            return;
        };
        let target = MenuTarget::InstrumentParam {
            address: param.address.clone(),
            name: param.label.clone(),
        };
        let bounds = self
            .editors
            .iter()
            .find(|e| e.kind == EditorKind::Instrument)
            .map(|editor| editor.panel.body)
            .unwrap_or(self.instrument_layout.body);
        self.open_menu(target, x, y, bounds);
        self.redraw_editor(EditorKind::Instrument);
    }

    fn drag_knob(&mut self, y: f32) {
        let Some((which, from_y, from_value)) = self.knob else {
            return;
        };
        // Shift is a fine drag, the same modifier the roll gives it.
        let value = knob_value(from_value, y - from_y, self.modifiers.shift_key());
        let Some(address) = self
            .instrument
            .as_ref()
            .and_then(|view| view.param(which.0, which.1))
            .map(|param| param.address.clone())
        else {
            return;
        };
        // Nothing to say when the value has not moved a step: a patch rewrite
        // is a graph rebuild, and one per pixel of a stationary mouse is waste.
        let current = self
            .instrument
            .as_ref()
            .and_then(|view| view.param(which.0, which.1))
            .map_or(0.0, |param| param.value);
        if (value - current).abs() < 0.001 {
            return;
        }
        self.set_param(&address, value);
    }

    /// Which of the two panels of knobs `editor` is looking at.
    ///
    /// One question in one place, because the instrument panel and an effect's
    /// own are the same shape over two different views and everything that
    /// works on one has to work on the other — that is what
    /// `press_insert_panel`'s doc means by *"the same panel with a different
    /// list in it"*.
    /// One control of whichever panel `editor` is currently showing.
    ///
    /// The instrument window draws **two** different panels — the knob grid
    /// and Flopsynth's cards — and `group` means a group in one and a card in
    /// the other. Every place that resolves a `MenuTarget::ParamChoice` asks
    /// here rather than choosing for itself, because a chooser that opened one
    /// panel's list and wrote the other panel's control is a bug that only
    /// shows up on one instrument.
    fn choice_param(
        &self,
        editor: EditorKind,
        group: usize,
        param: usize,
    ) -> Option<&crate::canvas::InstrumentParam> {
        if editor == EditorKind::Instrument && self.flopsynth.is_some() {
            return self.flop_param((group, param));
        }
        self.param_view(editor)?.param(group, param)
    }

    fn param_view(&self, editor: EditorKind) -> Option<&crate::canvas::InstrumentView> {
        match editor {
            EditorKind::Instrument => self.instrument.as_ref(),
            EditorKind::Effect => self.insert_view.as_ref(),
            EditorKind::AudioClip => None,
        }
    }

    /// Drops a [`ParamKind::Choice`]'s list under the control it belongs to.
    ///
    /// > *"a lot of options that could be knobs or sliders or dropdowns for
    /// > some reason are instead shown as buttons you click to toggle through
    /// > a list of options in order iteratively."*
    ///
    /// A row of pips that stepped, before: it said where in the list you were
    /// and gave no way to get anywhere else in one press. The pips stay —
    /// they are how the control reads at a glance — and the list is what a
    /// press now opens.
    fn open_param_choice(&mut self, editor: EditorKind, which: (usize, usize)) {
        let bounds = self
            .editors
            .iter()
            .find(|e| e.kind == editor)
            .map(|e| e.panel.body);
        let (Some(bounds), Some(control)) = (
            bounds,
            match editor {
                EditorKind::Instrument if self.flopsynth.is_some() => {
                    self.flopsynth_layout.cards.get(which.0).and_then(|card| {
                        card.cells
                            .iter()
                            .find(|(index, _)| *index == which.1)
                            .map(|(_, cell)| *cell)
                    })
                }
                EditorKind::Instrument => self.instrument_layout.control(which.0, which.1),
                _ => self.insert_layout.control(which.0, which.1),
            },
        ) else {
            return;
        };
        self.open_menu(
            MenuTarget::ParamChoice {
                editor,
                group: which.0,
                param: which.1,
            },
            control.x,
            control.bottom(),
            bounds,
        );
        self.redraw_editor(editor);
    }

    fn set_param(&mut self, address: &fontelle_types::ParamAddress, value: f32) {
        if let Some(doc) = &mut self.options.document {
            doc.set_instrument_param(address, value);
        }
        self.tree.invalidate(PANEL);
        self.tree.invalidate(RACK);
        self.refresh_title();
    }

    // ------------------------------------------------------- arrangement ---

    /// A press that landed on the arrangement.
    fn press_timeline(&mut self, button: MouseButton, x: f32, y: f32) {
        self.focus = Focus::Timeline;
        // The toolbar first: it is the top strip of the panel, above the
        // ruler, and a press on it is a button press rather than a scrub.
        if self.timeline_layout.toolbar.contains(x, y) {
            if button == MouseButton::Left
                && let Some(control) = timeline_toolbar_hit(&self.timeline_bar, x, y)
            {
                self.activate_timeline(control);
            }
            return;
        }
        // The ruler and the lane headers are the left button's, for the reason
        // the roll's chrome is: the right button erases, and neither of them
        // holds a clip.
        if self.timeline_layout.ruler.contains(x, y) {
            match button {
                MouseButton::Left => {
                    self.drag = Drag::TimelineRuler;
                    self.mark_on_timeline(x);
                }
                // The right button drags a time selection out — *"right
                // click and drag on the time bar to loop a time section"*.
                // Anchored where it went down, unsnapped; the grid is applied
                // to both ends as the selection is drawn.
                MouseButton::Right => {
                    let grid = self.timeline_layout.grid;
                    let (x, _) = clamp_to_grid(grid, x, grid.y);
                    self.select_anchor = timeline_x_to_tick(&self.timeline.view, grid, x);
                    self.select_preview = None;
                    self.drag = Drag::TimelineSelect;
                }
            }
            return;
        }
        if self.timeline_layout.headers.contains(x, y)
            && let TimelineHit::Lane(lane) = timeline_hit(
                &self.timeline.view,
                &self.timeline_layout,
                &self.clips,
                x,
                y,
            )
        {
            match button {
                // *"i made one i dont want but i cant right click and delete
                // it."* Adding, renaming, muting and deleting a row all live
                // in one menu, because that is where somebody looks for them.
                MouseButton::Right => {
                    let bounds = self.layout.window;
                    self.open_menu(MenuTarget::Lane(lane), x, y, bounds);
                }
                MouseButton::Left => {
                    if let Some(doc) = &mut self.options.document {
                        doc.toggle_lane_mute(lane);
                    }
                }
            }
            self.tree.invalidate(TIMELINE);
            return;
        }
        // The second press of a double-click is a different gesture from a
        // press: on empty grid it asks for a **blank** clip where the first
        // press stamped a copy — see `Timeline::double_press` — and on an
        // audio clip it opens the editor, below. Decided once, here, so
        // the two readings cannot both happen.
        let doubled =
            button == MouseButton::Left && self.double_click.press(x, y, std::time::Instant::now());
        let edits = if doubled {
            self.timeline.double_press(
                button,
                x,
                y,
                &self.timeline_layout,
                &self.clips,
                self.beats_per_bar(),
            )
        } else {
            self.timeline.press(
                button,
                x,
                y,
                &self.timeline_layout,
                &self.clips,
                self.beats_per_bar(),
            )
        };
        self.drag = Drag::Timeline;
        self.apply_arrange_edits(edits);
        // A right-click on a point of an automation block asks for the
        // point's menu: its shape, or its removal.
        if let Some((clip, id)) = self.timeline.take_point_menu() {
            let bounds = self.layout.window;
            self.open_menu(MenuTarget::Point { clip, id }, x, y, bounds);
        }
        // Clicking a clip opens it in the roll — the two panels are two views
        // of one piece, and having to find the channel in the rack to edit the
        // clip you just pointed at is two panels rather than one workflow.
        // *"double clicking on an audio clip should open a menu that lets me
        // make changes to that audio."* Before the open below, because an
        // audio clip has no roll to be opened in — and on the **second** press
        // rather than the first, because clicking one is how you move it and a
        // window that appeared every time you nudged a take along the bar
        // would be in the way of the thing you were doing.
        if doubled
            && let Some(clip) = self
                .clips
                .iter()
                .find(|info| {
                    if info.kind != crate::document::ClipKind::Audio {
                        return false;
                    }
                    let block = crate::canvas::clip_rect(
                        &self.timeline.view,
                        self.timeline_layout.grid,
                        info,
                    )
                    .intersection(&self.timeline_layout.grid);
                    !block.is_empty() && block.contains(x, y)
                })
                .map(|info| info.id)
        {
            self.open_audio_editor(clip);
        }
        if let Some(clip) = self.timeline.take_open() {
            // Which editor a block opens into is the block's own business —
            // a note clip opens the roll; an automation clip is edited where
            // it sits, so the document is told which one is in hand and the
            // panel stays where it was. Read before the document is borrowed
            // mutably.
            let kind = self
                .clips
                .iter()
                .find(|info| info.id == clip)
                .map(|info| info.kind);
            if let Some(doc) = &mut self.options.document {
                doc.open_clip(clip);
                self.roll.clear_selection();
            }
            if kind == Some(crate::document::ClipKind::Notes) {
                self.tab = EditorTab::Roll;
            }
            self.refresh_studio();
            self.relayout_panels();
        }
        self.tree.invalidate(TIMELINE);
        self.tree.invalidate(PANEL);
        self.tree.invalidate(RACK);
    }

    fn drag_timeline(&mut self) {
        let (x, y) = self.cursor;
        let grid = self.timeline_layout.grid;
        // The same edge-scroll the roll has, for the same reason: a clip has to
        // be draggable to bar 1 from a screen away.
        let (ticks, rows) = self.edge_scroll_step(
            &crate::canvas::RollView {
                scroll_tick: self.timeline.view.scroll_tick,
                top_key: 0,
                pixels_per_tick: self.timeline.view.pixels_per_tick,
                key_height: self.timeline.view.lane_height,
                snap: self.timeline.view.snap,
            },
            grid,
        );
        if ticks != 0 || rows != 0 {
            let v = &mut self.timeline.view;
            v.scroll_tick = (v.scroll_tick + ticks).max(0);
            // Rows count *up* the keyboard in the roll and *down* the lane list
            // here, so the sign flips.
            v.top_lane = (v.top_lane as i64 - rows as i64).max(0) as usize;
            self.tree.invalidate(TIMELINE);
        }
        let edits = self.timeline.drag(
            x,
            y,
            &self.timeline_layout,
            &self.clips,
            self.beats_per_bar(),
        );
        // A marquee **or a cut**: both paint something the document knows
        // nothing about, so neither dirties the panel by producing an edit.
        // See `Timeline::draws_overlay` — this used to name the marquee alone,
        // which is why the cut tool's line was invisible until something else
        // happened to repaint.
        let overlay = self.timeline.draws_overlay();
        self.apply_arrange_edits(edits);
        if overlay {
            self.tree.invalidate(TIMELINE);
        }
    }

    /// The arrangement's ruler: the time marker, in song ticks.
    fn mark_on_timeline(&mut self, x: f32) {
        let grid = self.timeline_layout.grid;
        let (x, _) = clamp_to_grid(grid, x, grid.y);
        let tick = timeline_x_to_tick(&self.timeline.view, grid, x);
        let tick = if self.modifiers.alt_key() {
            tick
        } else {
            timeline_snap(&self.timeline.view, tick, self.beats_per_bar())
        };
        let Some(sample) = self
            .options
            .document
            .as_ref()
            .map(|doc| doc.sample_of_song_tick(tick))
        else {
            return;
        };
        self.mark(sample);
    }

    /// The panel sizes as they stand, for the layout.
    fn docks(&self) -> Docks {
        Docks {
            timeline_height: self.timeline_height,
            sidebar_width: self.sidebar_width,
            rack_share: self.rack_share,
        }
    }

    /// Dragging the seam down the right of the sidebar.
    fn drag_sidebar_seam(&mut self, x: f32) {
        let wanted = sidebar_width_at(&self.layout, x);
        if self.sidebar_width.is_some_and(|w| (w - wanted).abs() < 0.5) {
            return;
        }
        self.sidebar_width = Some(wanted);
        self.resize_from_window();
    }

    /// And the one across it, between the rack and the browser.
    /// Divides the soundfont panel's two lists — *"give it a knob i can drag
    /// in the middle to make whichever one i want bigger whenever i want."*
    fn drag_browser_split(&mut self, y: f32) {
        let wanted = browser_file_share_at(&self.browser, y);
        if self
            .browser_file_share
            .is_some_and(|s| (s - wanted).abs() < 0.001)
        {
            return;
        }
        self.browser_file_share = Some(wanted);
        self.relayout_panels();
        self.tree.invalidate(BROWSER);
    }

    fn drag_sidebar_split(&mut self, y: f32) {
        let wanted = rack_share_at(&self.layout, y);
        if self.rack_share.is_some_and(|s| (s - wanted).abs() < 0.001) {
            return;
        }
        self.rack_share = Some(wanted);
        self.resize_from_window();
    }

    fn drag_divider(&mut self, y: f32) {
        let wanted = timeline_height_at(&self.layout, y);
        if (wanted - self.timeline_height).abs() < 0.5 {
            return;
        }
        self.timeline_height = wanted;
        if wanted > 0.0 {
            self.timeline_height_shown = wanted;
        }
        self.resize_from_window();
    }

    /// Shows and hides the arrangement, remembering how tall it was.
    fn toggle_timeline(&mut self) {
        self.timeline_height = if self.timeline_height > 0.0 {
            0.0
        } else {
            self.timeline_height_shown
        };
        self.resize_from_window();
    }

    /// Recomputes the whole window from the live surface size — what a change
    /// to the arrangement's height needs, because it moves every panel below
    /// it.
    fn resize_from_window(&mut self) {
        let Some(live) = &self.live else {
            return;
        };
        let size = live.window.inner_size();
        let scale = live.window.scale_factor();
        self.resize(size.width.max(1), size.height.max(1), scale);
        if let Some(live) = &self.live {
            live.window.request_redraw();
        }
    }

    /// The one place the arrangement's wishes become document changes — the
    /// counterpart to [`apply_roll_edits`](Self::apply_roll_edits), and the
    /// same rule: there is no path from the canvas to a `&mut Project`.
    fn apply_arrange_edits(&mut self, edits: Vec<crate::canvas::ArrangeEdit>) {
        if edits.is_empty() {
            return;
        }
        // **Drawing with a prefab selected draws a place for it.**
        //
        // > *"clips that you can basically draw into your arrangement."*
        //
        // The draw tool's `Add` is the only edit this changes, and only while
        // the panel is on the prefab tab with one picked. Everything else —
        // moving, resizing, splitting, muting — is the same edit whichever
        // kind of block it lands on, because a place *is* a clip.
        let prefab = (self.rack_tab == crate::document::RackTab::Prefabs)
            .then(|| {
                self.options
                    .document
                    .as_ref()
                    .and_then(|doc| doc.selected_prefab())
            })
            .flatten();

        let mut created = crate::document::Created::default();
        if let Some(doc) = &mut self.options.document {
            for edit in edits {
                let made = match (prefab, &edit) {
                    (Some(index), crate::canvas::ArrangeEdit::Add { lane, start }) => {
                        let (lane, start) = (*lane, *start);
                        let mut made = crate::document::Created::default();
                        made.clips.extend(doc.draw_prefab(index, lane, start));
                        made
                    }
                    _ => doc.arrange(edit),
                };
                created.clips.extend(made.clips);
                created.points.extend(made.points);
            }
        }
        // The copy becomes the selection, so repeating walks along the
        // arrangement instead of stacking clips in one place — see
        // `Timeline::clips_inserted`. And a point a press just made is the
        // one its drag carries — `Timeline::points_inserted`.
        self.timeline.clips_inserted(created.clips);
        if let Some(clip) = self.timeline.point_clip() {
            self.timeline.points_inserted(clip, created.points);
        }
        // The blocks are re-read now rather than at the next frame: a point
        // drag reads the curve it is on, and one frame stale is one step of
        // the drag applied to the wrong list.
        self.refresh_studio();
        self.tree.invalidate(TIMELINE);
        self.tree.invalidate(PANEL);
        self.refresh_title();
    }

    /// One transport press, marker and all.
    ///
    /// The decision is [`crate::transport::action`]'s and the writes are
    /// [`crate::transport::apply`]'s; what is left here is holding the marker,
    /// which is window state because it is not something the engine has an
    /// opinion about.
    fn transport(&mut self, what: TransportHit) {
        // A stop while the tape is running keeps what was played. Read before
        // the action, because after it the transport is no longer recording
        // and the position is back at the mark.
        let was_recording = self.view.recording;
        let stopping = matches!(
            what,
            TransportHit::Stop | TransportHit::Play if self.view.playing
        );
        let end = self.view.position_sample;

        if let Some(host) = &mut self.options.host {
            // Commands down (TDD §2.2): one press, a couple of relaxed stores
            // on the other side. Nothing here waits for the audio thread to
            // acknowledge it — the next `tick` reads back what happened.
            let view = host.view();
            // `None` is the tempo and the signature: they are the document's,
            // not the engine's, and `press_transport` has already dealt with
            // them by starting a drag.
            if let Some(decision) = action(what, &view) {
                // *"when i click record it prompts me what i would like to
                // record."* Arming is the question, not the answer: the button
                // opens the menu and the menu arms. Disarming needs no
                // question, so it goes straight through.
                if decision == TransportAction::SetArmed(true) {
                    let bounds = self.layout.window;
                    let at = self.bar.record;
                    self.open_menu(MenuTarget::RecordMode, at.x, at.bottom(), bounds);
                    return;
                }
                if decision == TransportAction::SetArmed(false) {
                    self.arm_recording(false);
                    return;
                }
                // Play, while armed to record audio: roll a **bar early** over
                // the click, and start the tape when the playhead reaches the
                // marker. *"after the 4 tap metronome count in it starts
                // recording."*
                if decision == TransportAction::Play
                    && view.armed
                    && self
                        .options
                        .document
                        .as_ref()
                        .map(|doc| doc.record_mode() == crate::transport::RecordMode::Audio)
                        .unwrap_or(false)
                {
                    let beat = self
                        .options
                        .document
                        .as_ref()
                        .map_or(0, |doc| doc.samples_per_beat());
                    let beats = self
                        .options
                        .document
                        .as_ref()
                        .map_or(4, |doc| doc.beats_per_bar())
                        .min(crate::transport::COUNT_IN_BEATS);
                    let count = crate::transport::count_in_samples(beat, beats);
                    let target = self.marker;
                    if count > 0 {
                        self.count_in_until = Some(target);
                        self.take_from = None;
                        // The click, for the bar it is counting: a count-in
                        // nobody can hear is a bar of silence.
                        apply(
                            host.as_mut(),
                            TransportAction::SetMetronome(true),
                            self.marker,
                        );
                        self.marker = apply(
                            host.as_mut(),
                            TransportAction::Mark((target - count).max(0)),
                            self.marker,
                        );
                    } else {
                        self.count_in_until = None;
                        self.take_from = Some(target);
                        // The tape starts *here*, so whatever monitoring had
                        // left in the ring is not part of it. The count-in
                        // branch above discards at the moment it hands over
                        // for the same reason; a project with no tempo takes
                        // this one and must not be the one that records the
                        // minute you spent setting up.
                        if let Some(doc) = &mut self.options.document {
                            doc.discard_audio_take();
                        }
                    }
                }
                self.marker = apply(host.as_mut(), decision, self.marker);
            }
        }
        if was_recording && stopping {
            self.keep_take(end);
        }
        self.tick();
        // The marker is drawn on both rulers, and it may have moved without
        // the transport's position changing at all.
        self.tree.invalidate(TRANSPORT);
        self.tree.invalidate(PANEL);
    }

    /// Arms or disarms recording, opening or closing the capture stream that
    /// [`RecordMode::Audio`](crate::transport::RecordMode::Audio) needs.
    ///
    /// The stream is opened **on arming** rather than on play, so a microphone
    /// that is not there is reported while you are still setting up rather than
    /// in the middle of a take.
    fn arm_recording(&mut self, armed: bool) {
        if let Some(host) = &mut self.options.host {
            self.marker = apply(host.as_mut(), TransportAction::SetArmed(armed), self.marker);
        }
        // Arming throws the last take away, so a new one does not begin with
        // the end of the one before it still in the ring.
        if let Some(doc) = &mut self.options.document {
            doc.discard_take();
        }
        self.count_in_until = None;
        self.take_from = None;
        let wants_audio = armed
            && self
                .options
                .document
                .as_ref()
                .map(|doc| doc.record_mode() == crate::transport::RecordMode::Audio)
                .unwrap_or(false);
        if !wants_audio {
            if let Some(doc) = &mut self.options.document {
                doc.close_audio_input();
            }
            self.tick();
            self.tree.invalidate(TRANSPORT);
            return;
        }
        self.status = match self
            .options
            .document
            .as_mut()
            .map(|doc| doc.open_audio_input())
        {
            Some(Ok(name)) => format!("Recording from \u{201c}{name}\u{201d}"),
            Some(Err(said)) => said,
            None => String::new(),
        };
        self.tick();
        self.tree.invalidate(TRANSPORT);
        self.tree.invalidate(BROWSER);
    }

    /// Bounces the project to a WAV, and says where it went.
    ///
    /// The status line either way: a bounce that clipped, or one that had
    /// nowhere to go, is exactly the thing you want told rather than left to
    /// discover in a file manager.
    fn export(&mut self) {
        let Some(doc) = &mut self.options.document else {
            return;
        };
        self.status = match doc.export_wav() {
            Ok(said) | Err(said) => said,
        };
        self.tree.invalidate(BROWSER);
    }

    /// Takes a backup if one is due and there is anything to back up.
    ///
    /// Called from the event loop rather than from a timer of its own, so a
    /// window nobody is touching takes no backups — which is right, because
    /// there is nothing new in it to lose. See `fontelle_app::autosave_due`.
    fn maybe_autosave(&mut self) {
        if !autosave_due(self.last_autosave.elapsed(), AUTOSAVE_EVERY) {
            return;
        }
        // The clock restarts whether or not anything was written, so a clean
        // document is asked once a minute rather than on every pass.
        self.last_autosave = std::time::Instant::now();
        if let Some(doc) = &mut self.options.document
            && doc.autosave()
        {
            self.status = "backed up".to_string();
            self.tree.invalidate(BROWSER);
        }
    }

    /// Turns what was just played into notes on the open clip.
    ///
    /// Says how many, because "nothing was recorded" is the commonest thing
    /// that happens to a record button and a window that says nothing leaves
    /// you wondering whether it worked.
    fn keep_take(&mut self, end_sample: fontelle_types::Sample) {
        // An **audio** take is a different thing to keep, and it is kept
        // wherever the tape actually started rather than wherever the playhead
        // was when the transport rolled — see `count_in_until`.
        let from = self.take_from.take();
        self.count_in_until = None;
        let Some(doc) = &mut self.options.document else {
            return;
        };
        if doc.record_mode() == crate::transport::RecordMode::Audio {
            let at = from.unwrap_or(0);
            // The device's own rate, not a constant: a take counted in
            // 48 kHz seconds on a 44.1 kHz interface reads nine per cent
            // short, which is exactly wrong enough to be believed.
            let rate = if self.view.sample_rate > 0.0 {
                self.view.sample_rate
            } else {
                48_000.0
            };
            // The reason, when there is one: a take refused because there
            // was nowhere to write it used to be reported as *"nothing
            // arrived on the input"*, which was untrue and sent the person
            // checking cables.
            self.status = match doc.keep_audio_take(at, end_sample) {
                Ok(0) => "nothing arrived on the input, so nothing was recorded".to_string(),
                Ok(frames) => format!("recorded {:.1} seconds", frames as f64 / rate),
                Err(said) => said,
            };
            self.studio_revision = u64::MAX;
            self.tree.invalidate(PANEL);
            self.tree.invalidate(TIMELINE);
            self.tree.invalidate(BROWSER);
            self.refresh_title();
            return;
        }
        self.status = match doc.keep_take(end_sample) {
            0 => "nothing was played, so nothing was recorded".to_string(),
            1 => "recorded 1 note".to_string(),
            n => format!("recorded {n} notes"),
        };
        self.studio_revision = u64::MAX;
        self.tree.invalidate(PANEL);
        self.tree.invalidate(BROWSER);
        self.refresh_title();
    }

    /// A press that landed on the roll's grid.
    fn press_roll(&mut self, button: MouseButton, x: f32, y: f32) {
        let Some(doc) = &self.options.document else {
            return;
        };
        let beats_per_bar = doc.beats_per_bar();
        let edits = self.roll.press(
            button,
            x,
            y,
            self.roll_layout.grid,
            doc.notes(),
            beats_per_bar,
        );
        self.apply_roll_edits(edits);
        // A press asks for no sound of its own any more — it only *offers*
        // one, and `release_over` decides. See `PianoRoll::take_audition`: a
        // bare click on a note sounds it, and editing is silent.
        // A press always changes the selection or the gesture, both of which
        // are visible.
        self.tree.invalidate(PANEL);
    }

    fn press_lane(&mut self, x: f32, y: f32) {
        let Some(doc) = &self.options.document else {
            return;
        };
        let edits = self.roll.press_lane(
            x,
            y,
            self.roll_layout.velocity,
            self.roll_layout.grid,
            doc.notes(),
        );
        self.apply_roll_edits(edits);
        self.tree.invalidate(PANEL);
    }

    /// Acts on a row carried out of the soundfont panel, by where it landed.
    ///
    /// > *"i want to be able to click and drag them into the sampler or into
    /// > the channel rack to make it have a sampler with that clip sampled."*
    ///
    /// Three landings, three meanings, and they are the ones a rack already
    /// implies:
    ///
    /// - **A channel on the rack** takes the sound: that row becomes a sampler
    ///   playing the file, or a player of that preset. Dropping onto the row
    ///   you are working on is how a kit gets built without eight channels
    ///   appearing beside it.
    /// - **The empty space under the rows** makes a *new* channel of it.
    /// - **Anywhere else** is not a drop at all, and the press falls back to
    ///   what a click on that row has always meant — for an audio file, an
    ///   import onto the arrangement. The click waits for the release
    ///   precisely so that one gesture does one thing; see `press_browser`.
    fn drop_browser_row(&mut self, row: BrowserRow, x: f32, y: f32) {
        let on_rack = self.layout.rack.frame.contains(x, y);
        // Which channel it landed on, if it landed on one. Any part of the row
        // counts, the way the right-click menu already does: aiming at a
        // caption is not a thing anybody should have to do.
        let onto = match (on_rack, rack_hit(&self.rack, x, y)) {
            (
                true,
                RackHit::Row(index)
                | RackHit::Mute(index)
                | RackHit::Solo(index)
                | RackHit::Edit(index)
                | RackHit::Route(index),
            ) => Some(index),
            // The "+ Add instrument" button and the empty space below the rows
            // both mean the rack rather than a channel on it.
            _ => None,
        };
        // The instrument window's name field, when the release reached it —
        // *"i should be able to drag soundfonts into it from the soundfonts
        // window to also assign a soundfont."* It stands for the channel that
        // window has open, which is the selected one.
        let on_name = self.pointer_window == Some(EditorKind::Instrument)
            && self.instrument_layout.name.contains(x, y);
        // **The channel it landed on**, from either target. `None` and on the
        // rack means the rack itself; `None` and off it means it landed
        // nowhere.
        let onto = onto.or(on_name.then_some(self.selected_channel));
        let made = onto.is_none() && on_rack;
        let Some(doc) = &mut self.options.document else {
            return;
        };
        let result = match (row, onto, made) {
            // Onto a channel: it plays this instead of what it was playing.
            (BrowserRow::Preset(index), Some(channel), _) => {
                doc.set_channel_instrument_on(channel, index)
            }
            (BrowserRow::File(index), Some(channel), _) => {
                doc.set_sampler_from_import(channel, index)
            }
            // Onto the rack itself: a channel of its own.
            (BrowserRow::Preset(index), None, true) => doc.add_channel_with(index),
            (BrowserRow::File(index), None, true) => doc.add_sampler_from_import(index),
            // Let go over nothing. A file falls back to the click the press
            // did not do; a preset's click is a listen, and the press already
            // did that one.
            (BrowserRow::File(index), None, false) => doc.open_import(index),
            (BrowserRow::Preset(_), None, false) => return,
        };
        match result {
            Ok(()) => {
                // A new row is at the bottom of a list that may be scrolled
                // away from it.
                if made {
                    self.rack_scroll = usize::MAX;
                }
            }
            Err(e) => self.status = e,
        }
        self.refresh_studio();
        self.refresh_title();
        self.tree.invalidate(RACK);
        self.tree.invalidate(PANEL);
        self.tree.invalidate(BROWSER);
        self.tree.invalidate(TIMELINE);
    }

    fn press_rack(&mut self, button: MouseButton, x: f32, y: f32) {
        // One panel, two lists — see `canvas::tab_strip`. The strip is laid
        // out identically for both, so it is answered before either list gets
        // a look at the press.
        if self.rack_tab == crate::document::RackTab::Prefabs {
            self.press_prefabs(button, x, y);
            return;
        }
        let hit = rack_hit(&self.rack, x, y);
        // The right button opens a menu about whichever row it landed on —
        // *"stuff like being able to right click and duplicate too, for
        // instruments in the channel rack for example."* Any part of the row,
        // not only its name: aiming at a caption is not a thing anybody should
        // have to do to reach a menu.
        if button == MouseButton::Right {
            if let RackHit::Row(index)
            | RackHit::Mute(index)
            | RackHit::Solo(index)
            | RackHit::Edit(index)
            | RackHit::Route(index) = hit
            {
                let bounds = self.layout.window;
                self.open_menu(MenuTarget::Channel(index), x, y, bounds);
            }
            return;
        }
        match hit {
            RackHit::Row(index) => {
                if let Some(doc) = &mut self.options.document {
                    doc.select_channel(index);
                }
                self.roll.clear_selection();
                // If the instrument window is open it is now showing *this*
                // channel — see `refresh_editors` — so it comes to the front
                // rather than staying wherever it was behind the studio. It
                // is not *opened* by this: a click on a row means "work on
                // this part", and a window appearing every time you changed
                // parts would be its own complaint.
                self.raise_editor(EditorKind::Instrument);
            }
            RackHit::Mute(index) => {
                if let Some(doc) = &mut self.options.document {
                    doc.toggle_mute(index);
                }
            }
            RackHit::Solo(index) => {
                if let Some(doc) = &mut self.options.document {
                    doc.toggle_solo(index);
                }
            }
            RackHit::Edit(index) => {
                // Select the channel *and* open its instrument: this is the
                // switch that answers "why can't I open up the VST options".
                if let Some(doc) = &mut self.options.document {
                    doc.select_channel(index);
                }
                self.roll.clear_selection();
                self.open_editor(EditorKind::Instrument);
                self.tree.invalidate(PANEL);
            }
            RackHit::Route(index) => {
                // A press on an open chip shuts it, which is what every
                // drop-down does and what stops the menu needing a second
                // click somewhere neutral to dismiss.
                self.route_menu = match self.route_menu.take() {
                    Some((open, _)) if open == index => None,
                    _ => self
                        .rack
                        .rows
                        .iter()
                        .find(|row| row.index == index)
                        .map(|row| {
                            (
                                index,
                                route_menu_layout(
                                    row.route,
                                    self.layout.rack.body,
                                    &self.options.theme.metrics,
                                    &self.route_names,
                                ),
                            )
                        }),
                };
            }
            // *"when you select new instrument it lets you select one of
            // those"* — the button asks which rather than always making a synth.
            RackHit::Add => {
                let bounds = self.layout.window;
                self.scan_for_starred_plugins();
                self.open_menu(MenuTarget::NewInstrument, x, y, bounds);
            }
            RackHit::Tab(tab) => {
                if let Some(doc) = &mut self.options.document {
                    doc.set_rack_tab(tab);
                }
            }
            RackHit::Nothing => {}
        }
        self.tree.invalidate(RACK);
    }

    /// A press in the prefab list (TDD §10.5).
    ///
    /// > *"a plus icon to make a new one and name it and stuff"*, and
    /// > *"selecting the prefab in the prefab menu and then selecting the
    /// > instrument you want to edit."*
    fn press_prefabs(&mut self, button: MouseButton, x: f32, y: f32) {
        use crate::canvas::{PrefabHit, prefab_hit};
        let hit = prefab_hit(&self.prefab_panel, x, y);
        // The right button opens a menu about the row it landed on — rename,
        // delete — the same gesture the channel rack's rows answer.
        if button == MouseButton::Right {
            if let PrefabHit::Row(index) = hit {
                let bounds = self.layout.window;
                self.open_menu(MenuTarget::Prefab(index), x, y, bounds);
            }
            return;
        }
        match hit {
            PrefabHit::Row(index) => {
                // A press on the open one shuts it, which puts the roll back
                // on the arrangement — the same "press it again" every toggle
                // in this window has.
                let open = self.prefabs.get(index).is_some_and(|row| row.open);
                if let Some(doc) = &mut self.options.document {
                    doc.select_prefab((!open).then_some(index));
                }
                self.roll.clear_selection();
            }
            PrefabHit::Add => {
                if let Some(doc) = &mut self.options.document {
                    doc.add_prefab();
                }
                // The new one is at the end, and it is the one you are about
                // to name.
                self.prefab_scroll = usize::MAX;
            }
            PrefabHit::Tab(tab) => {
                if let Some(doc) = &mut self.options.document {
                    doc.set_rack_tab(tab);
                }
            }
            PrefabHit::Nothing => {}
        }
        self.tree.invalidate(RACK);
        self.tree.invalidate(PANEL);
    }

    /// A press while the route menu is open. Anywhere but a row shuts it.
    fn press_route_menu(&mut self, x: f32, y: f32) {
        let Some((channel, menu)) = self.route_menu.take() else {
            return;
        };
        self.tree.invalidate(RACK);
        let Some(choice) = route_menu_hit(&menu, x, y) else {
            return; // a click off the menu is how a menu is dismissed
        };
        let Some(doc) = &mut self.options.document else {
            return;
        };
        match choice {
            RouteChoice::Master => doc.set_channel_route(channel, None),
            RouteChoice::Track(index) => doc.set_channel_route(channel, Some(index)),
            RouteChoice::New => {
                // One gesture for "I need a drum bus and this goes on it".
                // The new track lands before the master, so its index is the
                // number of strips there were minus the master's row.
                let index = self.route_names.len().saturating_sub(1);
                doc.add_mixer_track();
                doc.set_channel_route(channel, Some(index));
            }
            // The rack's chip does not offer it: a channel plays somewhere or
            // it is muted, and "nowhere" is what the mute switch beside it
            // already means.
            RouteChoice::Off => {}
        }
        self.refresh_title();
    }

    /// Walks the preset list by `delta` rows, keeping the focused row on
    /// screen.
    fn step_preset_focus(&mut self, delta: i32) {
        let next = crate::canvas::browser_focus_step(&self.presets, self.preset_focus, delta);
        if next == self.preset_focus {
            return;
        }
        self.preset_focus = next;
        // Scrolled to, not just moved: a focus that walks off the bottom of
        // the list and keeps going is a focus nobody can follow.
        if let Some(index) = next {
            let rows = self.browser.preset_rows.len().max(1);
            if index < self.preset_scroll {
                self.preset_scroll = index;
            } else if index >= self.preset_scroll + rows {
                self.preset_scroll = index + 1 - rows;
            }
        }
        self.relayout_panels();
        self.tree.invalidate(BROWSER);
    }

    /// Puts the focused preset on the selected channel — what Enter and a
    /// double-click both mean.
    fn choose_focused_preset(&mut self) {
        let Some(index) = self.preset_focus else {
            return;
        };
        if self.presets.get(index).map(|entry| entry.kind)
            == Some(crate::document::LibraryKind::Group)
        {
            return;
        }
        if let Some(doc) = &mut self.options.document
            && let Err(e) = doc.set_channel_instrument(index)
        {
            self.status = e;
        }
        self.refresh_title();
        self.tree.invalidate(BROWSER);
        self.tree.invalidate(RACK);
    }

    fn press_browser(&mut self, x: f32, y: f32) {
        match browser_hit(&self.browser, x, y) {
            BrowserHit::Seam => {
                self.drag = Drag::BrowserSplit;
            }
            BrowserHit::Search(_) => {
                self.searching = true;
            }
            BrowserHit::File(index) => {
                // A row that **moves** the browser rather than opening
                // something: the list under the pointer is about to be a
                // different list, so the scroll offset it was read at means
                // nothing in it.
                //
                // **From the list the rows were drawn from.** This used to ask
                // `self.files` — the soundfont list — about a row that came out
                // of `self.imports`, so the answer was whatever the other panel
                // held at that index. See `canvas::browser_row_carries`.
                let rows = self.browser_list();
                let moving = matches!(
                    rows.get(index).map(|entry| entry.kind),
                    Some(crate::document::LibraryKind::Folder | crate::document::LibraryKind::Up)
                );
                // A **file** row in the Import tab can be carried out of the
                // panel and dropped on the rack to become a sampler. A folder
                // cannot: it is a place, not a sound.
                let carried = crate::canvas::browser_row_carries(rows, self.browser_mode, index);
                if carried {
                    self.drag = Drag::BrowserRow(BrowserRow::File(index));
                    // **And the click waits.** What this row means is decided
                    // by where the button comes up: on the rack it is a
                    // sampler, and anywhere else it is the click it always was
                    // — an import onto the arrangement. Doing the import here
                    // as well would leave a clip at bar one behind every drag.
                    // See `drop_browser_row`.
                    return;
                }
                // The same rows in both modes, and the same click: a soundfont
                // opens to show its presets, a folder opens to show what is
                // in it, and a project opens as a project.
                let modifiers = self.modifiers;
                let result = match (&mut self.options.document, self.browser_mode) {
                    // A device row opens to show its presets, exactly as a
                    // soundfont row does — the same list in the same place.
                    (Some(doc), BrowserMode::Sounds | BrowserMode::Presets) => doc.open_file(index),
                    (Some(doc), BrowserMode::Projects) => doc.open_project(index),
                    // A folder row walks in, a file row imports. Which of
                    // those it was is the host's to know — it holds the list.
                    (Some(doc), BrowserMode::Import) => doc.open_import(index),
                    (Some(doc), BrowserMode::Settings) => {
                        // A setting steps forward when you click it and back
                        // when you Ctrl+click, which is the same pair of
                        // gestures a preset row already uses for "this one"
                        // against "a new one". No text field is involved:
                        // there is not one in this window yet, and a value
                        // you can reach with one click is quicker anyway.
                        doc.nudge_setting(index, if modifiers.control_key() { -1 } else { 1 });
                        Ok(())
                    }
                    (None, _) => Ok(()),
                };
                if let Err(e) = result {
                    self.status = e;
                }
                if moving {
                    self.file_scroll = 0;
                }
                self.preset_scroll = 0;
                self.refresh_title();
            }
            BrowserHit::Preset(index) => {
                // A heading over a run of search hits is a label saying which
                // soundfont they came from. Clicking one does nothing —
                // quietly, because "that row is a heading" is a sentence
                // nobody needs to read.
                if self.presets.get(index).map(|entry| entry.kind)
                    == Some(crate::document::LibraryKind::Group)
                {
                    return;
                }
                // Clicking a row is what gives the list the keyboard, so the
                // arrows walk it from here — *"after it being clicked on to
                // focus it"*.
                self.preset_focus = Some(index);
                // And arms carrying it out of the panel; a press that never
                // moves is a click and does the click's thing instead.
                self.drag = Drag::BrowserRow(BrowserRow::Preset(index));
                // **A click is a listen; a double-click is a choice.**
                // *"if i click a soundfont in the soundfont menu it plays that
                // instrument at a c tone ... this is just so i can easily
                // click on instruments and hear how they sound"*, and *"the
                // way to change the sound ... should be to double click it or
                // press enter while its selected"*.
                let doubled = self.double_click.press(x, y, std::time::Instant::now());
                if !doubled {
                    self.preview_preset(index);
                    self.tree.invalidate(BROWSER);
                    return;
                }
                // A preset click puts it on the **selected** channel, which is
                // what "try this sound on this part" means. The add button —
                // and Ctrl+click — make a new one instead.
                let modifiers = self.modifiers;
                if let Some(doc) = &mut self.options.document {
                    let result = if modifiers.control_key() {
                        doc.add_channel_with(index)
                    } else {
                        doc.set_channel_instrument(index)
                    };
                    if let Err(e) = result {
                        self.status = e;
                    }
                }
            }
            BrowserHit::OpenFolder(mode) => {
                if let Some(doc) = &mut self.options.document {
                    match mode {
                        BrowserMode::Sounds => doc.reveal_library_dir(),
                        // The user's own bank, which is the half of the
                        // Presets tab there is anything to reveal *of*: the
                        // factory presets are in the binary.
                        BrowserMode::Presets => doc.reveal_preset_dir(),
                        BrowserMode::Projects => doc.reveal_projects_dir(),
                        BrowserMode::Import => doc.reveal_import_dir(),
                        BrowserMode::Settings => doc.reveal_config_dir(),
                    }
                }
            }
            BrowserHit::ChooseFolder(mode) => {
                // **The mode comes off the hit**, not off `self.browser_mode`.
                // The version that read it from the window branched for
                // "Open folder" and not for this one, so choosing a projects
                // folder replaced the soundfont bank and left the projects
                // folder unset — which is *"project management completely
                // impossible"*, since a new project then has nowhere to go.
                //
                // Ctrl adds a folder instead of replacing the list, the same
                // way Ctrl on a preset adds a channel instead of replacing its
                // instrument — and only in Sounds, because a bank is genuinely
                // several places and "where do my projects live" has one
                // answer. The button says "Change" because replacing is what
                // somebody clicking it means.
                let add = self.modifiers.control_key();
                if let Some(doc) = &mut self.options.document {
                    match mode {
                        BrowserMode::Sounds => doc.choose_library_dir(add),
                        BrowserMode::Presets => doc.choose_preset_dir(),
                        BrowserMode::Projects => doc.choose_projects_dir(),
                        // The kind the tab is showing, which the host holds:
                        // the mode alone does not say whether this is the
                        // MIDI folder or the score one.
                        BrowserMode::Import => doc.choose_import_dir(),
                        // Not drawn in the settings tab — there is no folder
                        // being browsed there — so this cannot be reached.
                        BrowserMode::Settings => {}
                    }
                }
                // The picker held the event loop while it was up, so the meters
                // and the playhead have a gap in them to catch up on.
                self.last_tick = std::time::Instant::now();
            }
            BrowserHit::Mode(mode) => {
                if self.browser_mode != mode {
                    self.browser_mode = mode;
                    if let Some(doc) = &mut self.options.document {
                        doc.set_browser_mode(mode);
                    }
                    // The search filters whichever list is showing, and a
                    // query typed against soundfonts means nothing against
                    // projects.
                    self.file_scroll = 0;
                    self.preset_scroll = 0;
                    // The status line says something different in each mode
                    // and the studio's revision has not moved, so ask for the
                    // lists again rather than waiting for something else to
                    // change them.
                    self.studio_revision = u64::MAX;
                    self.refresh_studio();
                    self.relayout_panels();
                }
            }
            BrowserHit::Kind(kind) => {
                if let Some(doc) = &mut self.options.document {
                    doc.set_import_kind(kind);
                }
                // A different folder is a different list, and the offset the
                // old one was read at means nothing in it.
                self.file_scroll = 0;
                self.studio_revision = u64::MAX;
                self.refresh_studio();
            }
            // **It asks first.** *"when i make a new project i need to be
            // prompted to name it"* — and a project whose name you chose is
            // one you can find again in a folder of them.
            BrowserHit::NewProject => self.ask_for_a_name(NameFor::NewProject, String::new()),
            BrowserHit::Export => self.export(),
            BrowserHit::Nothing => {}
        }
        self.tree.invalidate(BROWSER);
        self.tree.invalidate(RACK);
        // Importing a `.mid` of several parts asks a question rather than
        // doing anything; this is where it goes up.
        self.show_import_prompt();
    }

    /// How many rows the browser's main list has in the mode it is in.
    ///
    /// One function rather than the same three-armed match written out at
    /// every scroll clamp — which is how the projects list came to be bounded
    /// by the number of soundfonts.
    fn browser_rows(&self) -> usize {
        self.browser_list().len()
    }

    /// The rows themselves — which list the panel is drawing.
    ///
    /// The same four-armed match, in the one place it belongs. Written out by
    /// hand at each site, it went wrong the way it always does: the press that
    /// arms a drag asked the *soundfont* list about a row the *import* list had
    /// drawn, and no audio file could be dragged anywhere at all
    /// (`canvas::browser_row_carries`).
    fn browser_list(&self) -> &[crate::document::LibraryEntry] {
        match self.browser_mode {
            BrowserMode::Sounds | BrowserMode::Presets => &self.files,
            BrowserMode::Import => &self.imports,
            BrowserMode::Projects => &self.projects,
            BrowserMode::Settings => &self.settings,
        }
    }

    // ------------------------------------------------ the right-click menu ---

    /// What a menu about `target` offers.
    ///
    /// A function of the target and the studio's lists, so what a menu says is
    /// decided in one place rather than at each of the presses that opens one.
    /// Entries that cannot be chosen are **greyed rather than left out**: a
    /// menu that hides "Delete lane" on the last lane teaches nothing about why
    /// it is not there.
    /// Everything starred, or nothing when there is no document.
    fn favorites(&self) -> Vec<fontelle_types::Favorite> {
        self.options
            .document
            .as_ref()
            .map(|doc| doc.favorites())
            .unwrap_or_default()
    }

    /// The plugins a picker for `purpose` chooses from: effects for an insert
    /// slot, instruments for a channel.
    fn plugin_listings(&self, purpose: PluginPurpose) -> Vec<crate::document::PluginListing> {
        let Some(doc) = self.options.document.as_ref() else {
            return Vec::new();
        };
        match purpose {
            PluginPurpose::Insert(_) => doc.plugin_effects(),
            _ => doc.plugin_instruments(),
        }
    }

    /// The *+ fx* menu's rows, and what each means.
    fn effect_rows(&self) -> Vec<(crate::canvas::MenuEntry, crate::canvas::EffectRow)> {
        let plugins = self.plugin_listings(PluginPurpose::Insert(0));
        crate::canvas::effect_menu_rows(&self.favorites(), &plugins)
    }

    /// The *New instrument* / *Change instrument* menu's rows.
    fn instrument_rows(
        &self,
        current: Option<fontelle_types::InstrumentKind>,
    ) -> Vec<(crate::canvas::MenuEntry, crate::canvas::InstrumentRow)> {
        let plugins = self.plugin_listings(PluginPurpose::NewChannel);
        crate::canvas::instrument_menu_rows(current, &self.favorites(), &plugins)
    }

    /// The plugin picker's rows, narrowed by what has been typed.
    fn picker_rows(
        &self,
        purpose: PluginPurpose,
    ) -> Vec<(crate::canvas::MenuEntry, crate::canvas::PickerRow)> {
        let heading = match purpose {
            PluginPurpose::Insert(_) => "Plugin effects",
            _ => "Plugin instruments",
        };
        crate::canvas::plugin_picker_rows(
            heading,
            &self.menu_filter,
            &self.favorites(),
            &self.plugin_listings(purpose),
        )
    }

    /// What the star on row `index` of a menu about `target` stands for, if
    /// that row has one.
    fn favorite_at(&self, target: &MenuTarget, index: usize) -> Option<fontelle_types::Favorite> {
        use crate::canvas::{EffectRow, InstrumentRow, PickerRow};
        use fontelle_types::Favorite;
        let plugin_key = |purpose: PluginPurpose, which: usize| {
            self.plugin_listings(purpose)
                .get(which)
                .map(|listing| Favorite::Plugin(listing.key.clone()))
        };
        match target {
            MenuTarget::AddEffect(_) => match self.effect_rows().get(index)?.1 {
                EffectRow::Builtin(kind) => Some(Favorite::Effect(kind)),
                EffectRow::Plugin(which) => plugin_key(PluginPurpose::Insert(0), which),
                EffectRow::Heading | EffectRow::PluginPicker => None,
            },
            MenuTarget::NewInstrument | MenuTarget::ChangeInstrument(_) => {
                let current = match target {
                    MenuTarget::ChangeInstrument(channel) => self
                        .options
                        .document
                        .as_ref()
                        .and_then(|doc| doc.channel_kind(*channel)),
                    _ => None,
                };
                match self.instrument_rows(current).get(index)?.1 {
                    InstrumentRow::Kind(kind) if kind != fontelle_types::InstrumentKind::Plugin => {
                        Some(Favorite::Instrument(kind))
                    }
                    InstrumentRow::Plugin(which) => plugin_key(PluginPurpose::NewChannel, which),
                    _ => None,
                }
            }
            MenuTarget::PluginPicker(purpose) => match self.picker_rows(*purpose).get(index)?.1 {
                PickerRow::Plugin(which) => plugin_key(*purpose, which),
                _ => None,
            },
            _ => None,
        }
    }

    /// A press on a row's star. The thing is starred or un-starred, and the
    /// menu **stays open**, rebuilt around the new list where it was and
    /// scrolled where it was — a star that closed the menu would make
    /// starring three effects three trips.
    fn toggle_star(&mut self, target: &MenuTarget, index: usize) {
        let Some(favorite) = self.favorite_at(target, index) else {
            return;
        };
        if let Some(doc) = &mut self.options.document {
            doc.toggle_favorite(favorite);
        }
        let scroll = self.menu.as_ref().map_or(0.0, |(_, menu)| menu.scroll());
        self.relayout_menu();
        if let Some((_, menu)) = &mut self.menu {
            menu.scroll_by(scroll);
        }
        self.refresh_studio();
    }

    fn menu_entries(&self, target: &MenuTarget) -> Vec<crate::canvas::MenuEntry> {
        use crate::canvas::MenuEntry;
        match target {
            MenuTarget::AddEffect(_) => self
                .effect_rows()
                .into_iter()
                .map(|(entry, _)| entry)
                .collect(),
            MenuTarget::Prefab(index) => {
                // The count is on the row already; what the menu adds is the
                // two things you cannot do by clicking. "Delete" is safe here
                // in a way it usually is not — see `RemovePrefab`: it bakes
                // the content into every place rather than emptying them — so
                // it is not guarded behind a count.
                let uses = self.prefabs.get(*index).map_or(0, |row| row.uses);
                vec![
                    MenuEntry::new("Rename"),
                    MenuEntry::new(if uses == 1 {
                        "Delete (used in 1 place)".to_string()
                    } else {
                        format!("Delete (used in {uses} places)")
                    }),
                ]
            }
            MenuTarget::Channel(index) => {
                let plays = self
                    .channels
                    .get(*index)
                    .is_some_and(|channel| channel.has_instrument);
                vec![
                    MenuEntry::new("Open instrument"),
                    MenuEntry::new("Change instrument..."),
                    MenuEntry::new("Rename"),
                    MenuEntry::new("Duplicate"),
                    if plays {
                        MenuEntry::new("Clear instrument").after_rule()
                    } else {
                        MenuEntry::disabled("Clear instrument").after_rule()
                    },
                    if self.channels.len() > 1 {
                        MenuEntry::new("Delete instrument")
                    } else {
                        MenuEntry::disabled("Delete instrument")
                    },
                ]
            }
            MenuTarget::Lane(index) => {
                let muted = self.lanes.get(*index).is_some_and(|lane| lane.muted);
                let can_remove = self
                    .options
                    .document
                    .as_ref()
                    .is_some_and(|doc| doc.can_remove_lane(*index));
                vec![
                    // Where you are looking, not the end of the list —
                    // *"right now theyre all going to the end."*
                    MenuEntry::new("Add lane above"),
                    MenuEntry::new("Add lane below"),
                    MenuEntry::new("Render to audio"),
                    MenuEntry::new("Rename lane"),
                    MenuEntry::new(if muted { "Unmute lane" } else { "Mute lane" }),
                    // Greyed at the ends rather than left out, for the reason
                    // this function's own docs give.
                    if *index > 0 {
                        MenuEntry::new("Move up").after_rule()
                    } else {
                        MenuEntry::disabled("Move up").after_rule()
                    },
                    if *index + 1 < self.lanes.len() {
                        MenuEntry::new("Move down")
                    } else {
                        MenuEntry::disabled("Move down")
                    },
                    if can_remove {
                        MenuEntry::new("Delete lane").after_rule()
                    } else {
                        MenuEntry::disabled("Delete lane").after_rule()
                    },
                ]
            }
            // The control's own name first, greyed: a menu of one entry with
            // no heading is a menu you have to remember what you right-clicked
            // to read.
            MenuTarget::InstrumentParam { name, .. } | MenuTarget::InsertParam { name, .. } => {
                vec![
                    MenuEntry::disabled(name.clone()),
                    MenuEntry::new("Create automation clip").after_rule(),
                ]
            }
            MenuTarget::Tempo => vec![
                MenuEntry::disabled("Tempo".to_string()),
                MenuEntry::new("Create automation clip").after_rule(),
            ],
            // The three instruments, in one order, from one list — so the two
            // menus that offer them cannot come to disagree about what there
            // is. A heading, greyed, because a bare list of three words is a
            // menu you have to guess the subject of.
            MenuTarget::RenderChoice(_) => vec![
                MenuEntry::disabled("Render"),
                MenuEntry::new("Time selection"),
                MenuEntry::new("Whole track"),
            ],
            // Both instrument menus are one list — see
            // `canvas::instrument_menu_rows`. This one is not changing
            // anything, so nothing in it is the kind you already have.
            MenuTarget::NewInstrument => {
                let mut entries: Vec<MenuEntry> = self
                    .instrument_rows(None)
                    .into_iter()
                    .map(|(entry, _)| entry)
                    .collect();
                entries[0] = MenuEntry::disabled("New instrument");
                entries
            }
            MenuTarget::PluginPicker(purpose) => self
                .picker_rows(*purpose)
                .into_iter()
                .map(|(entry, _)| entry)
                .collect(),
            MenuTarget::NameProject(purpose) => {
                crate::canvas::name_prompt_entries(purpose.title(), &self.menu_filter)
            }
            MenuTarget::PresetMenu(kind) => self.preset_menu_rows(*kind).0,
            MenuTarget::AddPatchEffect => {
                let mut entries = vec![MenuEntry::disabled("Add effect")];
                for kind in self.patch_effect_kinds() {
                    entries.push(MenuEntry::new(kind.label()));
                }
                entries
            }
            MenuTarget::PresetSaveName(_) => {
                crate::canvas::name_prompt_entries("Preset name", &self.menu_filter)
            }
            MenuTarget::PresetNewCategory(_) => {
                crate::canvas::name_prompt_entries("New category", &self.menu_filter)
            }
            MenuTarget::PresetCategory(kind) => {
                let mut entries = vec![MenuEntry::disabled("Category")];
                for category in self.preset_categories_for(*kind) {
                    entries.push(MenuEntry::new(category));
                }
                entries.push(MenuEntry::new(crate::render::NEW_CATEGORY).after_rule());
                entries
            }
            MenuTarget::ChangeInstrument(index) => {
                let current = self
                    .options
                    .document
                    .as_ref()
                    .and_then(|doc| doc.channel_kind(*index));
                self.instrument_rows(current)
                    .into_iter()
                    .map(|(entry, _)| entry)
                    .collect()
            }
            // A rename target and nothing else — a mixer strip's name is typed
            // over in place rather than through a menu, which is what *"just
            // be able to rename super quickly"* asks for. An empty list opens
            // no menu, so this can never be a menu nobody can see.
            MenuTarget::MixerTrack(_) => Vec::new(),
            // Built by the host, which read the file. The window draws the
            // lines and says which one was pressed.
            MenuTarget::ImportChoice => match self
                .options
                .document
                .as_ref()
                .and_then(|doc| doc.import_prompt())
            {
                Some(prompt) => {
                    let mut entries = vec![MenuEntry::disabled(prompt.title)];
                    for (index, choice) in prompt.choices.into_iter().enumerate() {
                        let entry = MenuEntry::new(choice);
                        entries.push(if index == 0 {
                            entry.after_rule()
                        } else {
                            entry
                        });
                    }
                    entries
                }
                None => Vec::new(),
            },
            // *"when i click record it prompts me what i would like to
            // record."* The one that is on is marked — and still offered,
            // because choosing is how the button arms and a second take is
            // the same choice again. See `record_menu_entries_for`.
            MenuTarget::RecordMode => {
                let current = self
                    .options
                    .document
                    .as_ref()
                    .map(|doc| doc.record_mode())
                    .unwrap_or_default();
                crate::transport::record_menu_entries_for(current)
            }
            // Every input the machine has, and "none" above them — a track
            // that records nothing is the state every track starts in and the
            // one you need to be able to get back to.
            MenuTarget::TrackInput(strip) => {
                let strip = *strip;
                let Some(doc) = self.options.document.as_ref() else {
                    return Vec::new();
                };
                let current = doc.track_input(strip);
                let mut entries = vec![if current.is_none() {
                    MenuEntry::disabled("No input")
                } else {
                    MenuEntry::new("No input")
                }];
                let inputs = doc.audio_inputs();
                if inputs.is_empty() {
                    entries.push(MenuEntry::disabled("nothing to record from").after_rule());
                }
                for name in inputs {
                    let entry = if current.as_deref() == Some(name.as_str()) {
                        MenuEntry::disabled(name)
                    } else {
                        MenuEntry::new(name)
                    };
                    entries.push(entry);
                }
                entries
            }
            // The tools, each of which opens its own dialog — see
            // `canvas::tools`. A rule above the importers: bringing a file in
            // is not something you do to the selection.
            MenuTarget::RollTools => crate::canvas::TOOL_MENU
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let entry = MenuEntry::new(item.label());
                    if index == crate::canvas::ToolKind::ALL.len() {
                        entry.after_rule()
                    } else {
                        entry
                    }
                })
                .collect(),
            // A point's shapes, then its removal. The shape it has is greyed,
            // which is how the menu says which one that is.
            MenuTarget::Point { clip, id } => {
                let current = self
                    .clips
                    .iter()
                    .find(|info| info.id == *clip)
                    .and_then(|info| info.curve.iter().find(|p| p.id == *id))
                    .map(|p| p.curve);
                let mut entries: Vec<MenuEntry> = crate::canvas::CURVE_SHAPES
                    .iter()
                    .map(|shape| {
                        if current == Some(*shape) {
                            MenuEntry::disabled(crate::canvas::curve_label(*shape))
                        } else {
                            MenuEntry::new(crate::canvas::curve_label(*shape))
                        }
                    })
                    .collect();
                entries.push(MenuEntry::new("Delete point").after_rule());
                entries
            }
            // The grid, as a list rather than as six presses round a ring.
            MenuTarget::Snap { timeline } => {
                let current = if *timeline {
                    self.timeline.view.snap
                } else {
                    self.roll.view.snap
                };
                crate::canvas::SNAP_DIVISIONS
                    .iter()
                    .map(|snap| {
                        if *snap == current {
                            MenuEntry::disabled(snap.label())
                        } else {
                            MenuEntry::new(snap.label())
                        }
                    })
                    .collect()
            }
            // A parameter that is one of a list: its options, with the one it
            // is already on greyed. The same shape the audio editor's rows
            // get, because it is the same kind of value.
            MenuTarget::ParamChoice {
                editor,
                group,
                param,
            } => {
                let Some(param) = self.choice_param(*editor, *group, *param) else {
                    return Vec::new();
                };
                let ParamKind::Choice(options) = &param.kind else {
                    return Vec::new();
                };
                let at = crate::canvas::choice_index(&param.kind, param.value);
                options
                    .iter()
                    .enumerate()
                    .map(|(index, label)| {
                        if index == at {
                            MenuEntry::disabled(label)
                        } else {
                            MenuEntry::new(label)
                        }
                    })
                    .collect()
            }
            // A drop-down: the row's own list, with the one it is already on
            // greyed. Greyed rather than left out, so the list is the same
            // length and in the same order every time it opens — a menu whose
            // rows move about is a menu you have to read.
            MenuTarget::AudioRow(field) => {
                let (choices, at) = self.audio_row_menu(*field);
                choices
                    .into_iter()
                    .enumerate()
                    .map(|(index, label)| {
                        if Some(index) == at {
                            MenuEntry::disabled(&label)
                        } else {
                            MenuEntry::new(&label)
                        }
                    })
                    .collect()
            }
        }
    }

    /// What a drop-down row on the audio clip editor lists, and which entry it
    /// is on.
    ///
    /// The route is the one row whose entries are not the clip's to know —
    /// which mixer tracks exist is the document's — so it is answered from
    /// `clip_routes`, the same list every other route control in this window
    /// reads. Everything else comes off the field.
    fn audio_row_menu(&self, field: crate::canvas::AudioField) -> (Vec<String>, Option<usize>) {
        let Some(open) = &self.audio_clip else {
            return (Vec::new(), None);
        };
        if field == crate::canvas::AudioField::Route {
            let at = self
                .clip_routes
                .iter()
                .position(|(id, _)| *id == open.data.mixer_track);
            let names = self
                .clip_routes
                .iter()
                .map(|(_, name)| name.clone())
                .collect();
            return (names, at);
        }
        (
            crate::canvas::audio_row_choices(field)
                .into_iter()
                .map(str::to_string)
                .collect(),
            crate::canvas::audio_row_chosen(&open.data, field),
        )
    }

    /// Opens a menu about `target` at `(x, y)`, inside `bounds`.
    fn open_menu(&mut self, target: MenuTarget, x: f32, y: f32, bounds: crate::layout::Rect) {
        // The press that shut this very menu is not the press that reopens
        // it: a chip whose list drops down has to be a toggle, or it can
        // never be shut by clicking it again. See `dismissed`.
        if self.dismissed.as_ref() == Some(&target) {
            return;
        }
        // A fresh menu is unfiltered, and remembers where it was dropped so
        // that typing into it can re-lay it out in the same place.
        self.menu_filter.clear();
        self.menu_at = ((x, y), bounds);
        let entries = self.menu_entries(&target);
        let menu = crate::canvas::context_menu_layout(
            (x, y),
            bounds,
            &self.options.theme.metrics,
            self.options.theme.font.size,
            entries,
        );
        if menu.is_empty() {
            return;
        }
        self.menu = Some((target, menu));
        self.tree.invalidate_rect(bounds);
    }

    /// Builds the open menu again, in the place it already occupies.
    ///
    /// What a keystroke in the plugin picker does. Deliberately **not**
    /// `open_menu`: that one guards against the press that just dismissed a
    /// menu reopening it, which is a rule about clicking and nothing to do
    /// with typing.
    fn relayout_menu(&mut self) {
        let Some((target, _)) = self.menu.take() else {
            return;
        };
        let (at, bounds) = self.menu_at;
        let entries = self.menu_entries(&target);
        let menu = crate::canvas::context_menu_layout(
            at,
            bounds,
            &self.options.theme.metrics,
            self.options.theme.font.size,
            entries,
        );
        if !menu.is_empty() {
            self.menu = Some((target, menu));
        }
        self.tree.invalidate_rect(bounds);
    }

    /// A keystroke while a menu that types is open — the plugin picker, which
    /// filters, and the name prompt, which is a name box.
    ///
    /// `true` when it was theirs, which is every printable character,
    /// backspace and escape — so a "d" typed at a list of plugins is a letter
    /// in a name and not the delete tool. The same rule the search box
    /// follows, for the same reason.
    fn menu_filter_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        use winit::keyboard::{Key, NamedKey};
        let target = self.menu.as_ref().map(|(target, _)| target);
        let naming = matches!(
            target,
            Some(
                MenuTarget::NameProject(_)
                    | MenuTarget::PresetSaveName(_)
                    | MenuTarget::PresetNewCategory(_)
            )
        );
        if !naming
            && !matches!(
                target,
                Some(MenuTarget::PluginPicker(_) | MenuTarget::PresetMenu(_))
            )
        {
            return false;
        }
        match &event.logical_key {
            // **Enter is the press.** A name box you can only finish with the
            // mouse is a name box, and then a reach for the mouse.
            Key::Named(NamedKey::Enter) if naming => {
                let Some((target, _)) = self.menu.take() else {
                    return true;
                };
                let bounds = self.menu_at.1;
                self.tree.invalidate_rect(bounds);
                // Row 1: the heading is row 0 and cannot be chosen.
                self.choose_menu(&target, 1);
                true
            }
            Key::Named(NamedKey::Escape) => {
                let bounds = self.menu_at.1;
                self.menu = None;
                self.menu_filter.clear();
                self.tree.invalidate_rect(bounds);
                true
            }
            Key::Named(NamedKey::Backspace) => {
                self.menu_filter.pop();
                self.relayout_menu();
                true
            }
            Key::Character(text) => {
                let typed: String = text.chars().filter(|c| !c.is_control()).collect();
                if typed.is_empty() {
                    return false;
                }
                self.menu_filter.push_str(&typed);
                self.relayout_menu();
                true
            }
            Key::Named(NamedKey::Space) => {
                self.menu_filter.push(' ');
                self.relayout_menu();
                true
            }
            _ => false,
        }
    }

    /// A press while a menu is open. Every press closes it; one that landed on
    /// a row does what the row says first.
    ///
    /// Whether the press was **consumed** — a press that chose an entry, or one
    /// that landed on the menu's own frame without choosing, must not also
    /// reach whatever is drawn underneath it.
    fn press_menu(&mut self, x: f32, y: f32) -> bool {
        // The scrollbar takes the press before the rows do, and the menu
        // stays open under it: this press is the start of a drag, not a
        // choice. Reported from using it — *"i cant drag the scroll knob"* —
        // and it comes first because the bar is drawn over the right-hand end
        // of the rows it is scrolling.
        if let Some((_, menu)) = &self.menu
            && let Some(grab) = menu.thumb_grab(x, y)
        {
            self.menu_grab = grab;
            self.drag = Drag::MenuScroll;
            return true;
        }
        // A star is pressed without the menu closing — see `toggle_star`.
        if let Some((target, menu)) = &self.menu
            && let Some(index) = crate::canvas::context_menu_star_hit(menu, x, y)
        {
            let target = target.clone();
            self.toggle_star(&target, index);
            return true;
        }
        let Some((target, menu)) = self.menu.take() else {
            return false;
        };
        let inside = menu.frame.contains(x, y);
        let chosen = crate::canvas::context_menu_hit(&menu, x, y);
        // See `dismissed`: a press that shut this menu without choosing from
        // it must not be the press that opens it again.
        self.dismissed = (!inside).then(|| target.clone());
        self.tree.invalidate_rect(menu.frame);
        self.tree.invalidate(RACK);
        self.tree.invalidate(TIMELINE);
        self.tree.invalidate(PANEL);
        match chosen {
            Some(index) => self.choose_menu(&target, index),
            // A question dismissed is a question answered "no". Without this
            // the pending import would still be waiting, and the next click
            // anywhere would put the same menu back up.
            None => {
                if matches!(target, MenuTarget::ImportChoice)
                    && let Some(doc) = &mut self.options.document
                {
                    doc.cancel_import();
                }
            }
        }
        inside
    }

    /// Puts the plugin at `which` where `purpose` says — a new channel, an
    /// existing one, or the end of a strip's chain. One place, because the
    /// picker, the *+ fx* menu and the instrument menus all end here.
    fn choose_plugin(&mut self, purpose: PluginPurpose, which: usize, name: String) {
        if let Some(doc) = &mut self.options.document {
            match purpose {
                PluginPurpose::NewChannel => doc.add_plugin_channel(which),
                PluginPurpose::ChangeChannel(channel) => {
                    doc.set_channel_plugin(channel, which);
                    // The channel this was chosen for is the one whose window
                    // opens below, and the instrument window is always the
                    // selected channel's.
                    doc.select_channel(channel);
                }
                PluginPurpose::Insert(strip) => doc.add_plugin_insert(strip, which),
            }
        }
        self.status = name;
        if matches!(purpose, PluginPurpose::NewChannel) {
            self.rack_scroll = usize::MAX;
        }
        match purpose {
            PluginPurpose::NewChannel | PluginPurpose::ChangeChannel(_) => {
                self.open_chosen_instrument();
            }
            PluginPurpose::Insert(strip) => self.open_newest_insert(strip),
        }
    }

    /// Opens the selected channel's instrument window, straight after the
    /// instrument was chosen.
    ///
    /// *"make it so when an instrument is added or changed etc. it opens the
    /// window for that instrument."* The studio is refreshed first, because
    /// the window's shape is decided by what is on the channel
    /// (`create_editor` asks `self.flopsynth`), and that is read from the
    /// document by `refresh_studio`.
    fn open_chosen_instrument(&mut self) {
        self.refresh_studio();
        self.refresh_title();
        self.open_editor(EditorKind::Instrument);
    }

    /// Opens the window of the effect just added to the end of `strip`'s
    /// chain — the same request, for an effect.
    fn open_newest_insert(&mut self, strip: usize) {
        self.refresh_studio();
        self.refresh_title();
        let Some(slot) = self
            .mixer_strips
            .get(strip)
            .map(|s| s.inserts.len())
            .filter(|n| *n > 0)
            .map(|n| n - 1)
        else {
            return;
        };
        self.open_insert(strip, slot);
    }

    /// What entry `index` of a menu about `target` does. The other half of
    /// [`menu_entries`](Self::menu_entries), and the two are read together.
    fn choose_menu(&mut self, target: &MenuTarget, index: usize) {
        match (target, index) {
            (MenuTarget::Channel(channel), 0) => {
                if let Some(doc) = &mut self.options.document {
                    doc.select_channel(*channel);
                }
                self.open_editor(EditorKind::Instrument);
            }
            // A menu of its own rather than three more rows here: the kinds
            // are one list (`MenuTarget::ChangeInstrument`) and both places
            // that offer them read it, so they cannot come to disagree.
            (MenuTarget::Channel(channel), 1) => {
                let target = MenuTarget::ChangeInstrument(*channel);
                let (x, y) = self.cursor;
                let bounds = self.layout.window;
                self.scan_for_starred_plugins();
                self.open_menu(target, x, y, bounds);
            }
            (MenuTarget::Channel(channel), 2) => self.start_rename(MenuTarget::Channel(*channel)),
            (MenuTarget::Channel(channel), 3) => {
                if let Some(doc) = &mut self.options.document {
                    doc.duplicate_channel(*channel);
                }
                self.status = "Instrument duplicated".to_string();
                self.rack_scroll = usize::MAX;
            }
            (MenuTarget::Channel(channel), 4) => {
                if let Some(doc) = &mut self.options.document {
                    doc.clear_channel_instrument(*channel);
                }
            }
            (MenuTarget::Channel(channel), _) => {
                if let Some(doc) = &mut self.options.document {
                    doc.remove_channel(*channel);
                }
            }

            (MenuTarget::Prefab(index), 0) => self.start_rename(MenuTarget::Prefab(*index)),
            (MenuTarget::Prefab(index), _) => {
                let at = *index;
                if let Some(doc) = &mut self.options.document {
                    doc.remove_prefab(at);
                }
                self.status =
                    "Prefab deleted — the places it was drawn in keep what it held".to_string();
            }

            (MenuTarget::Lane(lane), 0) => {
                let at = *lane;
                if let Some(doc) = &mut self.options.document {
                    doc.add_lane_at(at);
                }
                self.lane_made();
            }
            (MenuTarget::Lane(lane), 1) => {
                let at = lane.saturating_add(1);
                if let Some(doc) = &mut self.options.document {
                    doc.add_lane_at(at);
                }
                self.lane_made();
            }
            // *"it will either do my time selection (if i have one) but it
            // will prompt first ... or if there was no time selection just
            // render the whole track."* The prompt only exists when there is
            // something to choose between.
            (MenuTarget::Lane(lane), 2) => {
                let lane = *lane;
                let selection = self
                    .options
                    .document
                    .as_ref()
                    .and_then(|doc| doc.loop_range());
                match selection {
                    Some(_) => {
                        let (x, y) = self.cursor;
                        let bounds = self.layout.window;
                        self.open_menu(MenuTarget::RenderChoice(lane), x, y, bounds);
                    }
                    None => self.render_lane(lane, None),
                }
            }
            (MenuTarget::Lane(lane), 3) => self.start_rename(MenuTarget::Lane(*lane)),
            (MenuTarget::Lane(lane), 4) => {
                if let Some(doc) = &mut self.options.document {
                    doc.toggle_lane_mute(*lane);
                }
            }
            (MenuTarget::Lane(lane), 5) => {
                if let Some(doc) = &mut self.options.document {
                    doc.move_lane(*lane, -1);
                }
                self.status = "Row moved up".to_string();
            }
            (MenuTarget::Lane(lane), 6) => {
                if let Some(doc) = &mut self.options.document {
                    doc.move_lane(*lane, 1);
                }
                self.status = "Row moved down".to_string();
            }
            (MenuTarget::Lane(lane), _) => {
                if let Some(doc) = &mut self.options.document {
                    doc.remove_lane(*lane);
                }
            }

            // Entry 1 is the selection, entry 2 the whole row.
            (MenuTarget::RenderChoice(lane), index) => {
                let lane = *lane;
                let span = match index {
                    1 => self
                        .options
                        .document
                        .as_ref()
                        .and_then(|doc| doc.loop_range()),
                    _ => None,
                };
                self.render_lane(lane, span);
            }

            // Entry 0 is the greyed heading, so the kinds start at 1 and line
            // up with `InstrumentKind::ALL`.
            //
            // **The button asks now.** It used to make a synth outright, which
            // was itself a fix: before *that* it put the browser's first
            // preset on a new channel and did nothing at all when no soundfont
            // had been opened, which is how a button ends up looking broken
            // and then looking haunted. Asking is the version that survives
            // there being three instruments to choose between.
            (MenuTarget::PluginPicker(purpose), index) => {
                use crate::canvas::PickerRow;
                let purpose = *purpose;
                match self.picker_rows(purpose).get(index).map(|(_, row)| *row) {
                    Some(PickerRow::Plugin(which)) => {
                        let name = self
                            .plugin_listings(purpose)
                            .get(which)
                            .map(|listing| listing.name.clone())
                            .unwrap_or_default();
                        self.choose_plugin(purpose, which, name);
                    }
                    Some(PickerRow::Rescan) => {
                        if let Some(doc) = &mut self.options.document {
                            doc.rescan_plugins();
                        }
                        self.status = "Scanning plugin folders".to_string();
                    }
                    _ => return,
                }
                self.refresh_studio();
                self.refresh_title();
            }
            // The mixer's *+ fx* row. A built-in goes straight on; a starred
            // plugin goes straight on too, which is what starring it bought;
            // and *Plugin…* opens the picker, dropped from where this was.
            (MenuTarget::AddEffect(strip), index) => {
                use crate::canvas::EffectRow;
                let strip = *strip;
                match self.effect_rows().get(index).map(|(_, row)| *row) {
                    Some(EffectRow::Builtin(kind)) => {
                        if let Some(doc) = &mut self.options.document {
                            doc.add_insert(strip, kind);
                        }
                        self.open_newest_insert(strip);
                    }
                    Some(EffectRow::Plugin(which)) => {
                        let name = self
                            .plugin_listings(PluginPurpose::Insert(strip))
                            .get(which)
                            .map(|listing| listing.name.clone())
                            .unwrap_or_default();
                        self.choose_plugin(PluginPurpose::Insert(strip), which, name);
                    }
                    Some(EffectRow::PluginPicker) => {
                        if let Some(doc) = &mut self.options.document {
                            doc.scan_plugins_once();
                        }
                        let (x, y) = self.cursor;
                        let bounds = self.layout.window;
                        self.open_menu(
                            MenuTarget::PluginPicker(PluginPurpose::Insert(strip)),
                            x,
                            y,
                            bounds,
                        );
                        return;
                    }
                    _ => return,
                }
                self.refresh_studio();
                self.refresh_title();
            }
            // One live row, and Enter presses it — see `menu_filter_key`.
            (MenuTarget::NameProject(purpose), _) => {
                let purpose = *purpose;
                let name = self.menu_filter.clone();
                self.menu_filter.clear();
                let done = self.options.document.as_mut().map(|doc| match purpose {
                    NameFor::NewProject => doc.new_project_named(&name),
                    NameFor::SaveAs => doc.save_as(&name),
                });
                self.status = match done {
                    Some(Ok(())) => match purpose {
                        NameFor::NewProject => "new project".to_string(),
                        NameFor::SaveAs => "saved".to_string(),
                    },
                    Some(Err(e)) => e,
                    None => String::new(),
                };
                self.studio_revision = u64::MAX;
                self.refresh_studio();
                self.refresh_title();
                self.tree.invalidate(BROWSER);
                self.tree.invalidate(RACK);
                self.tree.invalidate(TIMELINE);
                self.tree.invalidate(PANEL);
            }
            (MenuTarget::AddPatchEffect, index) => {
                // Row 0 is the heading, so the kinds start at 1.
                let kind = index
                    .checked_sub(1)
                    .and_then(|which| self.patch_effect_kinds().get(which).copied());
                if let (Some(kind), Some(doc)) = (kind, self.options.document.as_mut()) {
                    doc.add_patch_effect(kind);
                }
                self.after_flop_structure();
            }
            (MenuTarget::PresetMenu(kind), index) => {
                let kind = *kind;
                let row = self.preset_menu_rows(kind).1.get(index).copied();
                let Some(crate::canvas::PresetMenuRow::Preset(which)) = row else {
                    // A heading. Nothing happens, and the menu has already
                    // closed — the same as pressing any other greyed row.
                    return;
                };
                if let (Some(device), Some(doc)) =
                    (self.preset_device(kind), self.options.document.as_mut())
                {
                    doc.apply_preset(device, which);
                }
                self.after_preset_change();
            }
            // One live row, and Enter presses it — see `menu_filter_key`.
            (MenuTarget::PresetSaveName(kind), _) => {
                let kind = *kind;
                let name = self.menu_filter.trim().to_string();
                self.menu_filter.clear();
                if name.is_empty() {
                    return;
                }
                let Some(slot) = Self::preset_slot(kind) else {
                    return;
                };
                self.pending_save_as = Some((slot, name));
                // Straight on to the second question, in the same place, so
                // the two prompts read as one gesture.
                self.ask_in_editor(MenuTarget::PresetCategory(kind), kind, String::new());
            }
            (MenuTarget::PresetCategory(kind), index) => {
                let kind = *kind;
                let categories = self.preset_categories_for(kind);
                // Row 0 is the heading; the rows after the categories is the
                // one that makes a new one.
                match index.checked_sub(1).and_then(|at| categories.get(at)) {
                    Some(category) => {
                        let category = category.clone();
                        self.finish_save_as(kind, &category);
                    }
                    None => {
                        self.ask_in_editor(
                            MenuTarget::PresetNewCategory(kind),
                            kind,
                            String::new(),
                        );
                    }
                }
            }
            (MenuTarget::PresetNewCategory(kind), _) => {
                let kind = *kind;
                let category = self.menu_filter.trim().to_string();
                self.menu_filter.clear();
                if category.is_empty() {
                    return;
                }
                self.finish_save_as(kind, &category);
            }
            (MenuTarget::NewInstrument, index) => {
                use crate::canvas::InstrumentRow;
                let kind = match self.instrument_rows(None).get(index).map(|(_, row)| *row) {
                    Some(InstrumentRow::Kind(kind)) => kind,
                    // A starred plugin: what starring it was for.
                    Some(InstrumentRow::Plugin(which)) => {
                        let name = self
                            .plugin_listings(PluginPurpose::NewChannel)
                            .get(which)
                            .map(|listing| listing.name.clone())
                            .unwrap_or_default();
                        self.choose_plugin(PluginPurpose::NewChannel, which, name);
                        self.refresh_studio();
                        self.refresh_title();
                        return;
                    }
                    _ => return,
                };
                // "Plugin" is not an instrument yet — it is a promise to name
                // one, and the list of which is on the machine rather than in
                // this binary. So the row opens the browser instead of making
                // a channel that plays nothing.
                if kind == fontelle_types::InstrumentKind::Plugin {
                    if let Some(doc) = &mut self.options.document {
                        doc.scan_plugins_once();
                    }
                    let (x, y) = self.cursor;
                    let bounds = self.layout.window;
                    self.open_menu(
                        MenuTarget::PluginPicker(PluginPurpose::NewChannel),
                        x,
                        y,
                        bounds,
                    );
                    return;
                }
                let mut added = false;
                if let Some(doc) = &mut self.options.document {
                    match doc.add_channel_of(kind) {
                        Ok(()) => {
                            self.status = format!("New instrument: {}", kind.label());
                            self.rack_scroll = usize::MAX;
                            added = true;
                        }
                        Err(e) => self.status = e,
                    }
                }
                self.tree.invalidate(RACK);
                self.tree.invalidate(PANEL);
                // The new channel is the selected one (`Session::new_channel_of`),
                // so this opens *its* window.
                if added {
                    self.open_chosen_instrument();
                }
            }
            (MenuTarget::ChangeInstrument(channel), index) => {
                use crate::canvas::InstrumentRow;
                let channel = *channel;
                let current = self
                    .options
                    .document
                    .as_ref()
                    .and_then(|doc| doc.channel_kind(channel));
                let kind = match self
                    .instrument_rows(current)
                    .get(index)
                    .map(|(_, row)| *row)
                {
                    Some(InstrumentRow::Kind(kind)) => kind,
                    Some(InstrumentRow::Plugin(which)) => {
                        let name = self
                            .plugin_listings(PluginPurpose::ChangeChannel(channel))
                            .get(which)
                            .map(|listing| listing.name.clone())
                            .unwrap_or_default();
                        self.choose_plugin(PluginPurpose::ChangeChannel(channel), which, name);
                        self.refresh_studio();
                        self.refresh_title();
                        return;
                    }
                    _ => return,
                };
                // As above: which plugin is a second question, and this menu
                // cannot ask it.
                if kind == fontelle_types::InstrumentKind::Plugin {
                    if let Some(doc) = &mut self.options.document {
                        doc.scan_plugins_once();
                    }
                    let (x, y) = self.cursor;
                    let bounds = self.layout.window;
                    self.open_menu(
                        MenuTarget::PluginPicker(PluginPurpose::ChangeChannel(channel)),
                        x,
                        y,
                        bounds,
                    );
                    return;
                }
                if let Some(doc) = &mut self.options.document {
                    doc.select_channel(channel);
                    doc.set_channel_kind(channel, kind);
                }
                self.status = format!("Instrument is now {}", kind.label());
                self.tree.invalidate(RACK);
                self.tree.invalidate(PANEL);
                self.open_chosen_instrument();
            }

            (MenuTarget::InsertParam { param, name }, 1) => {
                let (param, name) = (param.clone(), name.clone());
                self.automate_insert_named(&param, &name);
            }
            (MenuTarget::InstrumentParam { address, .. }, 1) => {
                let at = self.view.position_sample;
                let address = address.clone();
                if let Some(doc) = &mut self.options.document {
                    doc.automate_instrument_param(&address, at);
                }
                // The lane is on the arrangement now, and that is where it is
                // drawn in — *"then it appears in my timeline and im able to
                // draw it"*.
                self.lane_made();
            }
            (MenuTarget::Tempo, 1) => {
                let at = self.view.position_sample;
                if let Some(doc) = &mut self.options.document {
                    let at = doc.playhead_song_tick(at);
                    doc.create_automation(
                        &fontelle_types::ParamTarget::Tempo.address(),
                        "Tempo",
                        at,
                    );
                }
                self.lane_made();
            }
            // Entry 0 is the greyed title, which cannot be chosen, so the
            // answers start at 1 — and the host counts them from zero.
            (MenuTarget::ImportChoice, index) => {
                if let Some(doc) = &mut self.options.document {
                    match index.checked_sub(1) {
                        Some(choice) => doc.answer_import(choice),
                        None => doc.cancel_import(),
                    }
                }
                // What arrived is a row on the arrangement and a clip in the
                // roll, so both have to be re-read and re-measured.
                self.lane_made();
                self.tree.invalidate(TIMELINE);
                self.tree.invalidate(RACK);
            }
            // A tool: the three that take settings open their dialog, and the
            // two importers open a file browser. `TOOL_MENU` is the order in
            // both places, so nothing here has to know which is which.
            (MenuTarget::RollTools, index) => match crate::canvas::TOOL_MENU.get(index) {
                Some(crate::canvas::ToolMenuItem::Open(kind)) => {
                    let kind = *kind;
                    self.open_tool_dialog(kind);
                }
                Some(crate::canvas::ToolMenuItem::Run(action)) => {
                    let action = *action;
                    self.run_tool(action);
                }
                None => {}
            },
            (MenuTarget::RecordMode, index) => {
                if let Some(mode) = crate::transport::RecordMode::ALL.get(index).copied() {
                    if let Some(doc) = &mut self.options.document {
                        doc.set_record_mode(mode);
                    }
                    // Choosing what to record *is* arming: the button was
                    // pressed to arm and the menu was the question it asked, so
                    // an answer that left it disarmed would need a second press
                    // of the same button.
                    self.arm_recording(true);
                }
            }
            (MenuTarget::TrackInput(strip), index) => {
                let strip = *strip;
                let chosen = match index.checked_sub(1) {
                    None => None,
                    Some(n) => self
                        .options
                        .document
                        .as_ref()
                        .and_then(|doc| doc.audio_inputs().get(n).cloned()),
                };
                if let Some(doc) = &mut self.options.document {
                    doc.set_track_input(strip, chosen);
                }
                self.refresh_studio();
                self.tree.invalidate(PANEL);
            }
            (MenuTarget::Point { clip, id }, index) => {
                let (clip, id) = (*clip, *id);
                let edit = match crate::canvas::CURVE_SHAPES.get(index) {
                    Some(shape) => crate::canvas::ArrangeEdit::SetPointCurve {
                        clip,
                        ids: vec![id],
                        curve: *shape,
                    },
                    None => {
                        self.timeline.delete_points();
                        crate::canvas::ArrangeEdit::RemovePoints {
                            clip,
                            ids: vec![id],
                        }
                    }
                };
                self.apply_arrange_edits(vec![edit]);
                if let Some(doc) = &mut self.options.document {
                    doc.end_gesture();
                }
            }
            (MenuTarget::Snap { timeline }, index) => {
                let Some(snap) = crate::canvas::SNAP_DIVISIONS.get(index).copied() else {
                    return;
                };
                if *timeline {
                    self.timeline.view.snap = snap;
                    self.tree.invalidate(TIMELINE);
                } else {
                    self.roll.view.snap = snap;
                    self.tree.invalidate(PANEL);
                }
            }
            // The same, for a parameter on either panel of knobs. The value
            // is a normalised position in the list (see
            // `canvas::choice_index`), so a choice and a knob stay one kind
            // of number and automation can address either.
            (
                MenuTarget::ParamChoice {
                    editor,
                    group,
                    param,
                },
                index,
            ) => {
                let (editor, group, param) = (*editor, *group, *param);
                let Some(control) = self.choice_param(editor, group, param).cloned() else {
                    return;
                };
                let ParamKind::Choice(options) = &control.kind else {
                    return;
                };
                if index >= options.len() {
                    return;
                }
                let value = if options.len() < 2 {
                    control.value
                } else {
                    index as f32 / (options.len() - 1) as f32
                };
                match editor {
                    EditorKind::Instrument => self.set_param(&control.address, value),
                    _ => self.write_insert_param(control.address.as_str(), value),
                }
                if let Some(doc) = &mut self.options.document {
                    doc.end_gesture();
                }
                self.redraw_editor(editor);
            }
            // A drop-down on the audio clip editor: the entry chosen, in one
            // press, whichever end of the list it is at.
            (MenuTarget::AudioRow(field), index) => {
                let field = *field;
                let tracks: Vec<Option<fontelle_types::MixerTrackId>> =
                    self.clip_routes.iter().map(|(id, _)| *id).collect();
                let Some(open) = &mut self.audio_clip else {
                    return;
                };
                if field == crate::canvas::AudioField::Route {
                    crate::canvas::choose_audio_route(&mut open.data, index, &tracks);
                } else {
                    crate::canvas::choose_audio_row(&mut open.data, field, index);
                }
                self.write_audio_clip();
            }
            _ => {}
        }
        self.refresh_studio();
        self.refresh_title();
    }

    // ------------------------------------------------------ typing a name ---

    /// Puts the name prompt up, seeded with `seed`.
    ///
    /// In the middle of the window rather than at the pointer: the two things
    /// that ask — the Projects tab's *New* and Ctrl+S — are a button in one
    /// corner and a key that has no position at all, and a prompt that
    /// appeared under the mouse for one of them and in the corner for the
    /// other would be two different features.
    fn ask_for_a_name(&mut self, purpose: NameFor, seed: String) {
        // Neither of the two things that ask is a chip you press twice, so
        // the toggle guard `open_menu` keeps has nothing to say here — and
        // leaving it set would swallow the Ctrl+S after a prompt somebody
        // dismissed by clicking away from it.
        self.dismissed = None;
        let bounds = self.layout.window;
        let (x, y) = (
            bounds.x + bounds.width / 2.0,
            bounds.y + bounds.height / 3.0,
        );
        self.open_menu(MenuTarget::NameProject(purpose), x, y, bounds);
        // After opening, because a fresh menu is unfiltered and this one is
        // seeded — with the name the project already goes by, so Ctrl+S on an
        // unsaved studio is Enter away from done.
        if self.menu.is_some() && !seed.is_empty() {
            self.menu_filter = seed;
            self.relayout_menu();
        }
    }

    /// Starts a rename. Every keystroke after this goes into the name until
    /// Enter or Escape ends it.
    fn start_rename(&mut self, target: MenuTarget) {
        self.renaming = Some(target);
        self.status = "Type a name \u{2014} Enter when you are done".to_string();
        self.invalidate_names();
    }

    /// Everywhere a name being typed is drawn. One list, because a rename that
    /// redraws three panels out of four is a caret you cannot see in the
    /// fourth — the mixer is in `PANEL`, and it was the one left out.
    fn invalidate_names(&mut self) {
        self.tree.invalidate(RACK);
        self.tree.invalidate(TIMELINE);
        self.tree.invalidate(BROWSER);
        self.tree.invalidate(PANEL);
    }

    /// The name being typed, as it stands — read from the document, because
    /// the document is the buffer. See `renaming`.
    fn renaming_text(&self) -> String {
        match &self.renaming {
            Some(MenuTarget::Channel(index)) => self
                .channels
                .get(*index)
                .map_or_else(String::new, |channel| channel.name.clone()),
            Some(MenuTarget::Lane(index)) => self
                .lanes
                .get(*index)
                .map_or_else(String::new, |lane| lane.name.clone()),
            Some(MenuTarget::MixerTrack(index)) => self
                .mixer_strips
                .get(*index)
                .map_or_else(String::new, |strip| strip.name.clone()),
            Some(MenuTarget::Prefab(index)) => self
                .prefabs
                .get(*index)
                .map_or_else(String::new, |prefab| prefab.name.clone()),
            _ => String::new(),
        }
    }

    fn write_name(&mut self, name: String) {
        let target = self.renaming.clone();
        if let Some(doc) = &mut self.options.document {
            match target {
                Some(MenuTarget::Channel(index)) => doc.rename_channel(index, &name),
                Some(MenuTarget::Lane(index)) => doc.rename_lane(index, &name),
                Some(MenuTarget::MixerTrack(index)) => doc.rename_mixer_track(index, &name),
                Some(MenuTarget::Prefab(index)) => doc.rename_prefab(index, &name),
                _ => {}
            }
        }
        self.refresh_studio();
        self.refresh_title();
    }

    /// A key pressed while a name is being typed. Whether it was taken.
    ///
    /// It takes **all** of them while it is on, the same rule the search box
    /// follows: a typed "d" is a letter in a name, not the delete tool.
    fn rename_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        use winit::keyboard::{Key, NamedKey};
        if self.renaming.is_none() {
            return false;
        }
        match &event.logical_key {
            Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Escape) => {
                self.renaming = None;
                self.status.clear();
                // The coalesced rename ends here, so the *next* one is its own
                // undo entry rather than joining this one.
                if let Some(doc) = &mut self.options.document {
                    doc.end_gesture();
                }
            }
            Key::Named(NamedKey::Backspace) => {
                let mut name = self.renaming_text();
                name.pop();
                self.write_name(name);
            }
            Key::Named(NamedKey::Space) => {
                let name = format!("{} ", self.renaming_text());
                self.write_name(name);
            }
            Key::Character(c) => {
                let name = format!("{}{c}", self.renaming_text());
                self.write_name(name);
            }
            _ => {}
        }
        self.invalidate_names();
        true
    }

    /// Brings one of the editor column's tabs to the front — a click on the
    /// tab, or its number key (see `layout::editor_tab_for_key`).
    fn show_tab(&mut self, tab: EditorTab) {
        if self.tab == tab {
            return;
        }
        self.tab = tab;
        // Nothing reads a track's meter while the mixer is hidden, and a
        // peak is the highest **since the last read** — so the first frame
        // of the tab would otherwise show the loudest moment since it was
        // last open, and then fall. Drain once and throw it away.
        if tab == EditorTab::Mixer
            && let Some(doc) = &mut self.options.document
        {
            doc.mixer_peaks();
            self.mixer_peaks.clear();
        }
        self.relayout_panels();
        self.tree.invalidate(PANEL);
    }

    /// Switches the key strip between the keyboard and the list of names.
    ///
    /// Per channel and saved with the song, so a kit stays a list and the
    /// piano beside it stays a keyboard — see `StudioHost::set_key_style`.
    fn cycle_key_style(&mut self) {
        let wanted = self.key_style.next();
        if let Some(doc) = &mut self.options.document {
            doc.set_key_style(wanted);
        }
        self.refresh_studio();
        self.refresh_title();
        // The strip is a different width in the two views, so the grid moves.
        self.relayout_panels();
        self.tree.invalidate(PANEL);
    }

    /// A toolbar button.
    fn activate(&mut self, control: RollControl) {
        match control {
            RollControl::Tool(tool) => self.set_tool(tool),
            // The chip **drops its list**; `S` still steps. See
            // `canvas::SNAP_DIVISIONS`.
            RollControl::Snap => self.open_snap_menu(false),
            RollControl::ZoomInX => self.zoom(1.25, 1.0),
            RollControl::ZoomOutX => self.zoom(0.8, 1.0),
            RollControl::ZoomInY => self.zoom(1.0, 1.25),
            RollControl::ZoomOutY => self.zoom(1.0, 0.8),
            RollControl::Velocity => {
                // Put it back the height it was, not the height it ships at:
                // somebody who dragged the lane taller and then hid it wants
                // the tall one back.
                self.roll.lane_height = if self.roll.lane_height > 0.0 {
                    0.0
                } else {
                    self.lane_height_shown
                };
                self.relayout_panels();
                self.tree.invalidate(PANEL);
            }
            RollControl::Lane => self.toggle_lane_menu(),
            RollControl::Tools => self.toggle_tools_panel(),
            RollControl::Ghost => self.cycle_ghosts(),
            RollControl::Slide => self.toggle_slide(),
            RollControl::Keys => self.cycle_key_style(),
        }
    }

    /// Makes the selection slide notes, or ordinary ones — and decides what
    /// the next note drawn will be. `A` and the toolbar chip both come here.
    fn toggle_slide(&mut self) {
        let edits = match &self.options.document {
            Some(doc) => {
                let notes = doc.notes();
                self.roll.toggle_slide(notes)
            }
            None => Vec::new(),
        };
        self.apply_roll_edits(edits);
        if let Some(doc) = &mut self.options.document {
            doc.end_gesture();
        }
        self.tree.invalidate(PANEL);
    }

    /// Opens the lane chip's menu, or shuts it if it is already open.
    fn toggle_lane_menu(&mut self) {
        self.lane_menu = match self.lane_menu {
            Some(_) => None,
            None => {
                let chip = self
                    .roll_bar
                    .items
                    .iter()
                    .find(|(control, _)| *control == RollControl::Lane)
                    .map(|(_, rect)| *rect)
                    .unwrap_or(crate::layout::Rect::ZERO);
                Some(lane_menu_layout(
                    chip,
                    self.roll_layout.frame,
                    &self.options.theme.metrics,
                ))
            }
        };
        self.shape_labels();
        self.tree.invalidate(PANEL);
    }

    /// Opens a file that was dropped on the window.
    ///
    /// What it *is* is the host's to work out from its name — a `.mid`, an FL
    /// score, or a soundfont — because this crate may not read one.
    fn drop_file(&mut self, path: &std::path::Path) {
        // Where the pointer is, which for a file that has a position on the
        // song is where it goes. winit's drop carries no coordinates of its
        // own, so this is the last place the pointer was known to be — which
        // is the drop point on every backend that tracks a drag, and the point
        // you were last at on the ones that do not.
        let at = self.drop_sample();
        let result = match &mut self.options.document {
            Some(doc) => doc.drop_file_at(path, at),
            None => return,
        };
        match result {
            // An empty message is a question going up instead: the file held
            // several parts, and `refresh_studio` will find the prompt.
            Ok(message) => {
                if !message.is_empty() {
                    self.status = message;
                }
            }
            Err(e) => self.status = e,
        }
        self.refresh_studio();
        self.refresh_title();
        self.lane_made();
        self.tree.invalidate(PANEL);
        self.tree.invalidate(RACK);
        self.tree.invalidate(TIMELINE);
        self.tree.invalidate(BROWSER);
        self.show_import_prompt();
    }

    /// Which sample of the song a dropped file should land on.
    ///
    /// The bar under the pointer when it is over the arrangement, and the top
    /// of the song otherwise: dropping a file on the browser or the rack is
    /// not a statement about *where*, and guessing a position from a pointer
    /// that was never over the timeline would scatter imports along the song.
    ///
    /// Snapped, because a clip dropped a few pixels off the bar is a clip you
    /// then have to nudge — the same rule every other gesture on this
    /// timeline follows.
    fn drop_sample(&self) -> fontelle_types::Sample {
        let (x, y) = self.cursor;
        let grid = self.timeline_layout.grid;
        if !grid.contains(x, y) {
            return 0;
        }
        let Some(doc) = &self.options.document else {
            return 0;
        };
        let tick = crate::canvas::timeline_x_to_tick(&self.timeline.view, grid, x);
        let snapped = crate::canvas::timeline_snap(&self.timeline.view, tick, doc.beats_per_bar());
        doc.sample_of_song_tick(snapped.max(0))
    }

    /// Puts up the question a file has raised, if one is waiting and no menu
    /// is already up.
    ///
    /// Centred on the piano roll rather than dropped at the pointer, because
    /// nothing was right-clicked: the file may have arrived by a drop, and a
    /// menu under a pointer that is nowhere near it reads as a misfire.
    fn show_import_prompt(&mut self) {
        if self.menu.is_some() {
            return;
        }
        // The entries themselves come from `menu_entries`, which `open_menu`
        // asks for — this only needs to know whether there is a question.
        if self
            .options
            .document
            .as_ref()
            .and_then(|doc| doc.import_prompt())
            .is_none()
        {
            return;
        }
        let bounds = self.roll_layout.frame;
        let at = (
            bounds.x + (bounds.width / 3.0).max(0.0),
            bounds.y + (bounds.height / 4.0).max(0.0),
        );
        self.open_menu(MenuTarget::ImportChoice, at.0, at.1, bounds);
    }

    /// Sends the browser to the folder one of the two importers reads.
    ///
    /// **With nowhere to look, it goes to the settings instead** — asked for
    /// as *"if I don't have a folder selected yet, take me to the settings
    /// menu where I can select my preferred folder"*. Fontelle reads nothing
    /// the user has not named (INVARIANT 10), so there is no folder to fall
    /// back on, and an empty browser is one that looks broken rather than one
    /// that says what it wants.
    fn open_import_browser(&mut self, action: ToolAction) {
        let kind = match action {
            ToolAction::ImportScore => fontelle_types::FolderKind::Scores,
            _ => fontelle_types::FolderKind::Midi,
        };
        let has_dir = match &mut self.options.document {
            Some(doc) => {
                doc.set_import_kind(kind);
                doc.has_import_dir(kind)
            }
            None => return,
        };
        self.browser_mode = if has_dir {
            BrowserMode::Import
        } else {
            self.status = format!(
                "No {} folder yet \u{2014} choose one here, and it will be remembered",
                kind.tab_label()
            );
            BrowserMode::Settings
        };
        if let Some(doc) = &mut self.options.document {
            doc.set_browser_mode(self.browser_mode);
        }
        // The panel has done its job; leaving it up would cover the list it
        // just opened.
        self.tools_panel = None;
        self.file_scroll = 0;
        self.preset_scroll = 0;
        // The lists are read on a revision change and the studio's has not
        // moved, so ask for them again rather than waiting for something else
        // to change them.
        self.studio_revision = u64::MAX;
        self.refresh_studio();
        self.relayout_panels();
        self.tree.invalidate(BROWSER);
        self.tree.invalidate(PANEL);
    }

    /// Where the Tools chip is, for whatever hangs off it.
    fn tools_chip(&self) -> crate::layout::Rect {
        self.roll_bar
            .items
            .iter()
            .find(|(control, _)| *control == RollControl::Tools)
            .map(|(_, rect)| *rect)
            .unwrap_or(crate::layout::Rect::ZERO)
    }

    /// The Tools chip: a **menu of tools**, each of which opens its own dialog.
    ///
    /// It used to drop one bench holding every tool's settings and every
    /// tool's button at once, and that was reported back as *"you split the
    /// functionality between the settings and tools button... please make it
    /// so like fl studio these menus pop up as their own menu i can then make
    /// tweaks in to chose how i want the tool to apply then i click apply."*
    /// See `canvas::tools` for the whole of that reasoning.
    fn toggle_tools_panel(&mut self) {
        // A dialog already open is what the chip shuts, so the chip is still
        // the way out of whatever it let you in to.
        if self.tools_panel.take().is_some() {
            self.shape_labels();
            self.tree.invalidate(PANEL);
            return;
        }
        let chip = self.tools_chip();
        self.open_menu(
            MenuTarget::RollTools,
            chip.x,
            chip.bottom(),
            self.roll_layout.frame,
        );
    }

    /// Opens one tool's dialog under the chip.
    fn open_tool_dialog(&mut self, kind: ToolKind) {
        self.tools_panel = Some(tools_dialog_layout(
            kind,
            self.tools_chip(),
            self.roll_layout.frame,
            &self.options.theme.metrics,
        ));
        self.shape_labels();
        self.tree.invalidate(PANEL);
    }

    /// A press while a tool's dialog is open.
    ///
    /// **The dialog stays up** unless the press missed it, which is the
    /// difference between this and a menu: a menu is a question you answer
    /// once, and this is a bench you work at — set an amount, apply it, look
    /// at the result, apply it again. A press that missed closes it, the way
    /// clicking away from anything does.
    fn press_tools_panel(&mut self, x: f32, y: f32) {
        let Some(panel) = &self.tools_panel else {
            return;
        };
        let Some(row) = tools_dialog_hit(panel, x, y) else {
            if !panel.frame.contains(x, y) {
                self.tools_panel = None;
                self.tree.invalidate(PANEL);
            }
            return;
        };
        match self.tools.action(row) {
            Some(action) => self.run_tool(action),
            // A value steps forward on a click and back on a Ctrl+click —
            // the same pair of gestures the settings tab uses, and for the
            // same reason: there is no text field in this window, and a value
            // you can reach in a few clicks is quicker than one you type.
            None => {
                let delta = if self.modifiers.control_key() { -1 } else { 1 };
                self.tools.nudge(row, delta);
            }
        }
        self.shape_labels();
        self.tree.invalidate(PANEL);
    }

    /// Carries out one of the Tools panel's actions.
    fn run_tool(&mut self, action: ToolAction) {
        // The two importers are not edits: reading a file is the window's
        // job, and what they do is send the browser to the right folder.
        if matches!(action, ToolAction::ImportMidi | ToolAction::ImportScore) {
            self.open_import_browser(action);
            return;
        }
        let selection = self.roll.selection().to_vec();
        if selection.is_empty() {
            // Said out loud rather than silently doing nothing: a tool that
            // appears to be broken is worse than one that says what it wants.
            self.status = "Select some notes first \u{2014} the tools act on what you have chosen"
                .to_string();
            return;
        }
        let edits = match &self.options.document {
            Some(doc) => self.tools.run(action, &selection, doc.notes()),
            None => Vec::new(),
        };
        if edits.is_empty() {
            return;
        }
        self.apply_roll_edits(edits);
        // Each press is one edit and one undo entry, so the gesture ends
        // here — a second press of Randomize must not merge into the first.
        if let Some(doc) = &mut self.options.document {
            doc.end_gesture();
        }
        self.status = match action {
            ToolAction::Transpose => format!(
                "Moved {} note(s) by {:+} semitone(s)",
                selection.len(),
                self.tools.semitones
            ),
            ToolAction::Add => format!(
                "Added {} to the {} of {} note(s)",
                self.tools.amount,
                self.tools.property.label(),
                selection.len()
            ),
            ToolAction::Subtract => format!(
                "Took {} off the {} of {} note(s)",
                self.tools.amount,
                self.tools.property.label(),
                selection.len()
            ),
            ToolAction::Randomize => format!(
                "Randomised the {} of {} note(s)",
                self.tools.property.label(),
                selection.len()
            ),
            ToolAction::Legato => format!("Joined up {} note(s)", selection.len()),
            ToolAction::ImportMidi | ToolAction::ImportScore => String::new(),
        };
    }

    /// Picks a property from the open menu, or shuts it because the click
    /// missed.
    fn press_lane_menu(&mut self, x: f32, y: f32) {
        let Some(menu) = &self.lane_menu else {
            return;
        };
        if let Some(property) = lane_menu_hit(menu, x, y) {
            self.roll.lane_property = property;
            // Picking a property is also asking to see it: a lane that is
            // hidden cannot show what you just chose.
            if self.roll.lane_height <= 0.0 {
                self.roll.lane_height = self.lane_height_shown;
                self.relayout_panels();
            }
        }
        // Either way the menu is done — a menu that stays up after a choice is
        // one you have to dismiss twice.
        self.lane_menu = None;
        self.shape_labels();
        self.tree.invalidate(PANEL);
    }

    /// The chip that steps the onion skin: off, everything, then one
    /// instrument at a time.
    fn cycle_ghosts(&mut self) {
        self.roll.ghosts = self.roll.ghosts.next(self.channels.len());
        self.refresh_ghosts();
        self.tree.invalidate(PANEL);
    }

    /// Re-reads the ghosted notes. Called when the filter moves and when the
    /// studio says something a panel draws has changed — never once a frame,
    /// for the reason `refresh_studio` gives.
    fn refresh_ghosts(&mut self) {
        let filter = self.roll.ghosts;
        self.ghosts = match &self.options.document {
            Some(doc) => doc.ghost_notes(filter),
            None => Vec::new(),
        };
    }

    /// The chip that walks velocity -> pan -> tuning -> release -> the two mod
    /// values, and shows the lane if it was hidden — which is what somebody
    /// reaching for pan meant.
    fn cycle_lane_property(&mut self) {
        self.roll.lane_property = self.roll.lane_property.next();
        if self.roll.lane_height <= 0.0 {
            self.roll.lane_height = self.lane_height_shown;
        }
        self.relayout_panels();
        self.tree.invalidate(PANEL);
    }

    /// Dragging the seam above the lane.
    fn drag_lane_grip(&mut self, y: f32) {
        let wanted = lane_height_at(&self.roll_layout, y);
        // Half a pixel of hysteresis: a mouse that has not moved far enough to
        // change the layout must not relayout and redraw the panel anyway.
        if (wanted - self.roll.lane_height).abs() < 0.5 {
            return;
        }
        self.roll.lane_height = wanted;
        self.lane_height_shown = wanted;
        self.relayout_panels();
        self.tree.invalidate(PANEL);
    }

    /// Sliding down the on-screen keyboard, sounding each key as it is reached.
    fn audition_at(&mut self, y: f32) {
        let grid = self.roll_layout.grid;
        let (_, y) = clamp_to_grid(grid, grid.x, y);
        let key = y_to_key(&self.roll.view, grid, y);
        if self.audition.key() != Some(key) {
            // The keyboard plays **your** instrument, so a preset you were
            // listening to lets go of the live path here.
            self.end_preview();
            self.start_audition(key, 0);
        }
    }

    /// The take being recorded, in song ticks — see
    /// [`TimelineChrome::recording`](crate::render::TimelineChrome::recording).
    ///
    /// Entirely the window's own arithmetic: `take_from` is where the tape
    /// started (after the count-in, which is why it is not the marker) and the
    /// playhead is where it has got to. The document knows nothing about it
    /// yet and should not — the clip does not exist until the transport stops.
    fn recording_span(&self) -> Option<(fontelle_types::Tick, fontelle_types::Tick)> {
        let doc = self.options.document.as_ref()?;
        if !self.view.recording || doc.record_mode() != crate::transport::RecordMode::Audio {
            return None;
        }
        // `None` while the count-in is still running: nothing is being kept
        // yet, and a block that appeared a bar early would be a lie about
        // where the take starts.
        let from = self.take_from?;
        let start = doc.playhead_song_tick(from);
        let now = doc.playhead_song_tick(self.view.position_sample);
        (now > start).then_some((start, now))
    }

    /// What a clip's mixer track is called.
    ///
    /// A track that has been deleted since the clip was routed to it says so
    /// rather than reading as the master, which is a different statement and
    /// the one that would hide the mistake.
    fn clip_route_label(&self, track: Option<fontelle_types::MixerTrackId>) -> String {
        self.clip_routes
            .iter()
            .find(|(id, _)| *id == track)
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| "\u{2014} gone".to_string())
    }

    /// What the transport bar's tempo box reads.
    ///
    /// The tempo **in force at the playhead** — see
    /// [`StudioHost::tempo_at`](crate::document::StudioHost::tempo_at). With
    /// no tempo lane in the project this is the box value and nothing looks
    /// different; with one, the box finally moves with the song.
    ///
    /// Editing is unaffected and deliberately so: `self.tempo` is still what a
    /// drag starts from and what `set_tempo` writes, because the thing you can
    /// change is the document's tempo and the thing you are watching is the
    /// curve over it.
    fn tempo_showing(&self) -> f64 {
        match &self.options.document {
            Some(doc) if self.view.available => doc.tempo_at(self.view.position_sample),
            _ => self.tempo,
        }
    }

    /// A zoom from a button or a key, about **the pointer** — see
    /// [`crate::canvas::zoom_anchor`], which is where that rule lives.
    ///
    /// It lands on whichever canvas the pointer is over, so `+` in the
    /// arrangement zooms the arrangement. Before this it was always the roll,
    /// always about the middle: two ways of being somewhere you did not ask
    /// to be.
    fn zoom(&mut self, x: f32, y: f32) {
        if self
            .layout
            .timeline
            .frame
            .contains(self.cursor.0, self.cursor.1)
        {
            let grid = self.timeline_layout.grid;
            if x != 1.0 {
                timeline_zoom_x(
                    &mut self.timeline.view,
                    grid,
                    crate::canvas::zoom_anchor(grid, self.cursor),
                    x,
                );
            }
            // The arrangement's vertical zoom is row height, which has no
            // anchor to speak of: every lane keeps its order and its place in
            // the list.
            if y != 1.0 {
                timeline_zoom_y(&mut self.timeline.view, y);
            }
            self.tree.invalidate(TIMELINE);
            return;
        }
        let grid = self.roll_layout.grid;
        if x != 1.0 {
            zoom_x(
                &mut self.roll.view,
                grid,
                crate::canvas::zoom_anchor(grid, self.cursor),
                x,
            );
        }
        if y != 1.0 {
            let (_, cy) = self.cursor;
            let anchor = if grid.contains(self.cursor.0, cy) {
                cy
            } else {
                grid.y + grid.height / 2.0
            };
            zoom_y(&mut self.roll.view, grid, anchor, y);
        }
        self.tree.invalidate(PANEL);
    }

    /// Clicking or dragging the roll's ruler puts the **time marker** there,
    /// and the playhead with it.
    ///
    /// Snapped to the grid unless Alt is held, which is what makes "put it back
    /// at the top of the bar" a gesture rather than an aiming exercise. The
    /// tick is turned into a sample by the **document**, not by arithmetic on a
    /// BPM: a song with a tempo change has no single BPM to multiply by
    /// (INVARIANT 5), and only the document holds the map.
    fn mark_at_roll(&mut self, x: f32) {
        let grid = self.roll_layout.grid;
        // Clamped, so a drag that has run off the left-hand end of the ruler
        // means bar 1 rather than nothing.
        let (x, _) = clamp_to_grid(grid, x, grid.y);
        let snap = if self.modifiers.alt_key() {
            SnapDivision::None
        } else {
            self.roll.view.snap
        };
        let beats_per_bar = self
            .options
            .document
            .as_ref()
            .map_or(BEATS_PER_BAR, |doc| doc.beats_per_bar());
        let tick = snap_tick(x_to_tick(&self.roll.view, grid, x), snap, beats_per_bar);
        let Some(sample) = self
            .options
            .document
            .as_ref()
            .map(|doc| doc.sample_of_clip_tick(tick))
        else {
            return;
        };
        self.mark(sample);
    }

    /// The same, on the transport bar's ruler, which spans the whole song.
    ///
    /// [`sample_at`] clamps, so a drag that leaves the ruler on the left lands
    /// exactly on sample zero — the fix for "I can never get it to the very
    /// start".
    fn mark_on_bar(&mut self, x: f32) {
        if !self.view.available {
            return;
        }
        let sample = sample_at(self.bar.ruler, x, self.view.length_samples);
        self.mark(sample);
    }

    /// Puts the marker — and the playhead — at `sample`.
    fn mark(&mut self, sample: fontelle_types::Sample) {
        // Only skip when *both* are already there. Comparing the marker alone
        // means dragging the ruler back to the start of a song whose playhead
        // has run on leaves the playhead where it was, because the marker was
        // already zero — which is exactly "I cannot get it back to the very
        // start" wearing a different hat.
        if sample == self.marker && sample == self.view.position_sample {
            return;
        }
        if let Some(host) = &mut self.options.host {
            self.marker = apply(host.as_mut(), TransportAction::Mark(sample), self.marker);
        } else {
            self.marker = sample.max(0);
        }
        self.tick();
        self.tree.invalidate(TRANSPORT);
        self.tree.invalidate(PANEL);
    }

    fn drag_roll(&mut self) {
        let Some(doc) = &self.options.document else {
            return;
        };
        let beats_per_bar = doc.beats_per_bar();
        let (x, y) = self.cursor;
        let grid = self.roll_layout.grid;

        // A drag held past an edge scrolls the view towards it, so a note can
        // be dragged to bar 1 from a screen away and a phrase can be dragged
        // off the end of what is showing. The roll itself clamps the pointer
        // (see `clamp_to_grid`); this is the half that makes the clamp not
        // simply a wall.
        // Copied out first: `edge_scroll_step` takes `&mut self`, and the view
        // it reads is a `Copy` snapshot rather than a borrow of it.
        let view = self.roll.view;
        let (ticks, rows) = self.edge_scroll_step(&view, grid);
        if ticks != 0 || rows != 0 {
            let v = &mut self.roll.view;
            v.scroll_tick = (v.scroll_tick + ticks).max(0);
            v.top_key = (i32::from(v.top_key) + rows).clamp(11, 127) as u8;
            self.tree.invalidate(PANEL);
        }

        let Some(doc) = &self.options.document else {
            return;
        };
        let edits = self.roll.drag(x, y, grid, doc.notes(), beats_per_bar);
        // A marquee or a cut changes nothing in the document and everything on
        // screen, so it has to dirty the panel on its own account.
        let overlay = self.roll.draws_overlay();
        self.apply_roll_edits(edits);
        if overlay {
            self.tree.invalidate(PANEL);
        }
    }

    /// A drag in the property lane.
    fn drag_lane(&mut self) {
        let Some(doc) = &self.options.document else {
            return;
        };
        let (x, y) = self.cursor;
        let edits = self.roll.drag_lane(
            x,
            y,
            self.roll_layout.velocity,
            self.roll_layout.grid,
            doc.notes(),
        );
        self.apply_roll_edits(edits);
    }

    /// The one place the roll's wishes become document changes.
    ///
    /// Note what is *not* here: any path from the roll to a `&mut Project`.
    /// INVARIANT 2 holds because there is nothing to hold it wrong with.
    fn apply_roll_edits(&mut self, edits: Vec<crate::canvas::RollEdit>) {
        if edits.is_empty() {
            return;
        }
        let mut added = Vec::new();
        let mut inserted = Vec::new();
        if let Some(doc) = &mut self.options.document {
            for edit in edits {
                let is_add = matches!(edit, crate::canvas::RollEdit::Add { .. });
                let ids = doc.edit(edit);
                if is_add {
                    added.extend(ids);
                } else {
                    inserted.extend(ids);
                }
            }
        }
        // The handshake that makes drawing and sizing one gesture — see this
        // module's own docs and `canvas::piano_roll`.
        if let Some(id) = added.first() {
            self.roll.note_added(*id);
        }
        if !inserted.is_empty() {
            self.roll.notes_inserted(inserted);
        }
        self.tree.invalidate(PANEL);
        self.refresh_title();
    }

    /// Puts what the state machine decided onto the live path.
    ///
    /// The one place `audition_on`/`audition_off` are called from, which is
    /// what makes "every note-on is matched by exactly one note-off" a
    /// property of [`Auditions`] rather than of five call sites.
    fn send_audition(&mut self, actions: impl IntoIterator<Item = AuditionAction>) {
        let Some(doc) = &mut self.options.document else {
            return;
        };
        for action in actions {
            match action {
                AuditionAction::On { key, velocity, pan } => doc.audition_on(key, velocity, pan),
                AuditionAction::Off { key } => doc.audition_off(key),
            }
        }
    }

    /// Middle C, which is what a preview plays.
    ///
    /// *"make it so if im holding ctrl while i do it, it plays it an octave
    /// lower, and if im holding shift, it does it an octave higher."* Both at
    /// once cancel out, which is the only sensible reading of "down and up".
    fn preview_key(&self) -> u8 {
        let mut key = 60i32;
        if self.modifiers.control_key() {
            key -= 12;
        }
        if self.modifiers.shift_key() {
            key += 12;
        }
        key.clamp(0, 127) as u8
    }

    /// Lets you hear preset `index` without putting it on anything.
    fn preview_preset(&mut self, index: usize) {
        let key = self.preview_key();
        let loaded = match &mut self.options.document {
            Some(doc) => doc.preview_preset(index),
            None => return,
        };
        if let Err(e) = loaded {
            self.status = e;
            return;
        }
        // Through the same state machine every other audition uses, so this
        // note-on has a note-off coming like all the rest.
        self.start_audition(key, 0);
    }

    /// Sounds a key on the live path for at least `ticks` of song time.
    ///
    /// Zero ticks means "no note behind it" — the on-screen keyboard — and
    /// gets the floor.
    fn start_audition(&mut self, key: u8, ticks: fontelle_types::Tick) {
        // The template is the note you last drew or clicked (see
        // `PianoRoll::template`), so an audition sounds with that note's
        // velocity *and* its pan — which is what makes clicking a note you
        // panned hard left sound hard left.
        let template = self.roll.template();
        let (velocity, pan) = (template.velocity, template.pan);
        let seconds = self
            .options
            .document
            .as_ref()
            .map_or(0.5 / fontelle_types::PPQN as f64, |doc| {
                doc.seconds_per_tick()
            })
            * ticks.max(0) as f64;
        let hold = std::time::Duration::from_secs_f64(seconds.clamp(0.0, 60.0));
        let actions = self
            .audition
            .start(key, velocity, pan, hold, std::time::Instant::now());
        self.send_audition(actions);
    }

    /// Aims the live path back at the selected channel, ending any preview.
    ///
    /// Called from every audition that is *about the song* — a key, a note, a
    /// drawn note — so that clicking a preset and then playing the keyboard
    /// plays your instrument rather than the one you were listening to.
    fn end_preview(&mut self) {
        if let Some(doc) = &mut self.options.document {
            doc.end_preview();
        }
    }

    /// The mouse came up. The note is *scheduled* to stop rather than stopped:
    /// a note let go after twenty milliseconds still has to have been a note.
    fn stop_audition(&mut self) {
        self.audition.release(std::time::Instant::now());
    }

    /// Releases a scheduled audition once it is due. Called on every pass of
    /// the loop, beside the meters.
    fn settle_audition(&mut self) {
        if let Some(action) = self.audition.settle(std::time::Instant::now()) {
            self.send_audition([action]);
        }
    }

    /// Stops whatever is sounding immediately — the pointer has been taken
    /// away, so nothing is going to release it.
    fn silence_audition(&mut self) {
        if let Some(action) = self.audition.silence() {
            self.send_audition([action]);
        }
    }

    /// Whatever the roll asked to be sounded, sounded — see
    /// [`crate::canvas::PianoRoll::take_audition`].
    fn sound_roll_request(&mut self) {
        if let Some(asked) = self.roll.take_audition() {
            // A note in the roll is the song, not the browser.
            self.end_preview();
            self.start_audition(asked.key, asked.ticks);
        }
    }

    /// The wheel, routed by what it is over.
    ///
    /// Over the roll: vertical scroll moves through the keys, `Shift` scrolls
    /// the song, `Ctrl` zooms time about the pointer and `Ctrl+Shift` (or
    /// `Alt`) zooms pitch. Over a list: it scrolls that list. The FL habits,
    /// and the ones a mouse can express.
    /// The wheel over an open menu, wherever that menu was opened from.
    ///
    /// `true` when the menu took it, which means the caller's own window has
    /// nothing more to do with the event. Shared by the studio and by the
    /// editor windows because a menu belongs to the app rather than to a
    /// window: `self.menu` is one menu, drawn wherever it was opened, and a
    /// long one is unusable in any window that does not hand it the wheel.
    ///
    /// The caller invalidates: the studio redraws a rectangle, an editor
    /// window redraws itself.
    fn wheel_menu(&mut self, x: f32, y: f32, steps: f32) -> bool {
        let step = self.options.theme.metrics.row_height * MENU_WHEEL_ROWS;
        let Some((_, menu)) = &mut self.menu else {
            return false;
        };
        if !menu.frame.contains(x, y) {
            return false;
        }
        menu.scroll_by(-steps * step);
        true
    }

    /// One pointer move of a scrollbar drag — see [`Drag::MenuScroll`].
    ///
    /// Invalidates the whole window rather than the menu's frame: the rows
    /// change underneath the thumb, and the frame is where they are drawn.
    /// An editor window redraws itself after every move anyway, so this is
    /// only doing work for the studio.
    fn drag_menu_scroll(&mut self, y: f32) {
        let grab = self.menu_grab;
        let Some((_, menu)) = &mut self.menu else {
            return;
        };
        menu.drag_thumb(y, grab);
        self.tree.invalidate_rect(self.layout.window);
    }

    fn scroll_roll(&mut self, dx: f32, dy: f32) {
        let (x, y) = self.cursor;

        // A menu is drawn in front of everything, so the wheel is its before
        // it is anything else's. Without this a plugin list is 357 entries
        // with no way past the first twenty-seven.
        if self.wheel_menu(x, y, dy) {
            self.tree.invalidate_rect(self.layout.window);
            return;
        }

        // The transport's document boxes take the wheel, which is how you set
        // a tempo you already know the number of rather than hunting for it
        // with a drag.
        match hit(&self.bar, &self.view, x, y) {
            Some(TransportHit::Tempo) => {
                self.set_tempo(nudge_tempo(self.tempo, dy, self.modifiers.shift_key()));
                if let Some(doc) = &mut self.options.document {
                    doc.end_gesture();
                }
                return;
            }
            Some(TransportHit::Signature) => {
                self.set_beats_per_bar(step_beats_per_bar(self.beats_per_bar(), dy.round() as i32));
                return;
            }
            _ => {}
        }

        if self.tab == EditorTab::Mixer && self.layout.panel.body.contains(x, y) {
            // The scrolling half only: the master is pinned, and its count is
            // not part of what can scroll past.
            let scrollable = self.mixer_strips.len().saturating_sub(1);
            self.mixer_scroll = scrolled(self.mixer_scroll, -(dy.round() as i32), scrollable);
            self.relayout_panels();
            self.tree.invalidate(PANEL);
            return;
        }

        if self.layout.rack.frame.contains(x, y) {
            // Whichever list is showing scrolls; the other keeps its place, so
            // switching tabs comes back to where you were.
            let step = -(dy.round() as i32);
            if self.rack_tab == crate::document::RackTab::Prefabs {
                self.prefab_scroll = scrolled(self.prefab_scroll, step, self.prefabs.len());
            } else {
                self.rack_scroll = scrolled(self.rack_scroll, step, self.channels.len());
            }
            self.relayout_panels();
            self.tree.invalidate(RACK);
            return;
        }
        if self.layout.browser.frame.contains(x, y) {
            // Over a setting, the wheel **changes it** rather than scrolling
            // past it: the list is seven rows long, so there is nothing to
            // scroll, and turning a wheel over a value is how every other
            // number in this window is set.
            if self.browser_mode == BrowserMode::Settings {
                if let Some(index) = crate::canvas::row_under(&self.browser, x, y) {
                    let step = dy.round() as i32;
                    if step != 0
                        && let Some(doc) = &mut self.options.document
                    {
                        doc.nudge_setting(index, step);
                        self.tree.invalidate(BROWSER);
                    }
                }
                return;
            }
            let over_presets = self.browser.presets.contains(x, y);
            if over_presets {
                self.preset_scroll =
                    scrolled(self.preset_scroll, -(dy.round() as i32), self.presets.len());
            } else {
                // **The list that is showing**, not the soundfonts: a scroll
                // offset clamped against the wrong list either stops short of
                // the end of a long one or runs past the end of a short one,
                // and a panel that looks empty is what the second is.
                self.file_scroll =
                    scrolled(self.file_scroll, -(dy.round() as i32), self.browser_rows());
            }
            self.relayout_panels();
            self.tree.invalidate(BROWSER);
            return;
        }

        if self.layout.timeline.frame.contains(x, y) {
            let grid = self.timeline_layout.grid;
            let (ctrl, shift, alt) = (
                self.modifiers.control_key(),
                self.modifiers.shift_key(),
                self.modifiers.alt_key(),
            );
            let v = &mut self.timeline.view;
            if ctrl && shift || alt {
                timeline_zoom_y(v, 1.15_f32.powf(dy));
            } else if ctrl {
                timeline_zoom_x(v, grid, x.max(grid.x), 1.15_f32.powf(dy));
            } else if shift || dx != 0.0 {
                let by = if dx != 0.0 { dx } else { dy };
                let step = (160.0 / v.pixels_per_tick.max(0.0001)) as fontelle_types::Tick;
                v.scroll_tick = (v.scroll_tick - by as fontelle_types::Tick * step).max(0);
            } else {
                let rows = (dy * 2.0).round() as i64;
                v.top_lane = (v.top_lane as i64 - rows).max(0) as usize;
            }
            self.tree.invalidate(TIMELINE);
            return;
        }

        if self.options.document.is_none() {
            return;
        }
        let grid = self.roll_layout.grid;
        let (ctrl, shift, alt) = (
            self.modifiers.control_key(),
            self.modifiers.shift_key(),
            self.modifiers.alt_key(),
        );

        if ctrl && shift || alt {
            zoom_y(&mut self.roll.view, grid, y.max(grid.y), 1.15_f32.powf(dy));
        } else if ctrl {
            zoom_x(&mut self.roll.view, grid, x.max(grid.x), 1.15_f32.powf(dy));
        } else if shift || dx != 0.0 {
            let by = if dx != 0.0 { dx } else { dy };
            let v = &mut self.roll.view;
            let step = (120.0 / v.pixels_per_tick.max(0.0001)) as fontelle_types::Tick;
            v.scroll_tick = (v.scroll_tick - by as fontelle_types::Tick * step).max(0);
        } else {
            let v = &mut self.roll.view;
            let rows = (dy * 3.0).round() as i32;
            v.top_key = (i32::from(v.top_key) + rows).clamp(11, 127) as u8;
        }
        self.tree.invalidate(PANEL);
    }

    // ---------------------------------------------------------- keyboard ---

    /// The subset of §16.5's keymap the gate needs. Every binding here is
    /// hard-coded, and §16.5 says all of them are remappable — the map is a
    /// later item, and one binding written down twice is one to find and move.
    /// The keys that mean the same thing in every window, answered before
    /// anything that depends on which canvas has the keyboard. Whether one of
    /// them was pressed.
    ///
    /// Reported from using the studio: *"cannot use keybinds to like pause and
    /// play when i have one of the opened windows like an eq plugin window
    /// selected"*, and *"undoing and redoing isnt working in there either"*.
    /// The editor windows are separate OS windows, so a keystroke aimed at one
    /// never reached the studio's handler at all — the whole keyboard stopped
    /// at the title bar.
    ///
    /// **These and no others.** Space is the transport, Ctrl+Z is the history
    /// and Ctrl+S is the file, and none of the three is a statement about a
    /// canvas; the tool keys and Delete are, and they stay where the canvas
    /// that owns them can hear them.
    fn global_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        use winit::keyboard::{Key, NamedKey};

        let ctrl = self.modifiers.control_key();
        let shift = self.modifiers.shift_key();
        match &event.logical_key {
            // Play from the marker; press it again and the playhead comes back
            // to the marker. See `crate::transport::TransportAction`.
            Key::Named(NamedKey::Space) => self.transport(TransportHit::Play),
            // And the front of the song, which is what the stop square does.
            Key::Named(NamedKey::Home) => self.transport(TransportHit::Stop),
            Key::Character(c) => match c.to_lowercase().as_str() {
                "z" if ctrl && !shift => self.undo(),
                "z" if ctrl && shift => self.redo(),
                "y" if ctrl => self.redo(),
                "s" if ctrl => self.save(),
                // Beside Ctrl+S, because bouncing is the other thing you do to
                // a whole project. The button is on the Projects tab; this is
                // so you do not have to go there.
                "e" if ctrl => self.export(),
                // *"make ctrl + m toggle the metronome."* A transport switch,
                // so it is here with Space rather than with the canvas keys:
                // wanting the click on while you play a part in is not a
                // statement about which panel you were last looking at, and
                // the editor windows answer this function too.
                //
                // It took Ctrl+M off muting the selected clips, which has
                // moved to Ctrl+Shift+M — see `TimelineControl::Mute`.
                "m" if ctrl && !shift => self.transport(TransportHit::ToggleMetronome),
                _ => return false,
            },
            _ => return false,
        }
        true
    }

    fn key(&mut self, event: &winit::event::KeyEvent) {
        use winit::keyboard::{Key, NamedKey};

        let ctrl = self.modifiers.control_key();

        // A menu is in front of everything it was dropped over, so it has the
        // keyboard while it is open — which is what lets the plugin picker be
        // typed at rather than only scrolled.
        if self.menu_filter_key(event) {
            return;
        }

        // While a name is being typed, that has the keyboard: the same rule
        // the search box follows, and for the same reason.
        if self.rename_key(event) {
            return;
        }

        // While the search box has the keyboard, it has all of it: a typed "d"
        // is a letter in a soundfont's name, not the delete tool.
        if self.searching {
            match &event.logical_key {
                Key::Named(NamedKey::Escape) | Key::Named(NamedKey::Enter) => {
                    self.searching = false;
                }
                Key::Named(NamedKey::Backspace) => {
                    let mut query = self.query.clone();
                    query.pop();
                    self.set_query(query);
                }
                Key::Character(c) => {
                    let mut query = self.query.clone();
                    query.push_str(c);
                    self.set_query(query);
                }
                Key::Named(NamedKey::Space) => {
                    let mut query = self.query.clone();
                    query.push(' ');
                    self.set_query(query);
                }
                _ => {}
            }
            self.tree.invalidate(BROWSER);
            return;
        }

        // **The soundfont list holds the arrows while one of its rows is
        // focused.** Clicking a preset is what focuses it, so this can only be
        // on when somebody is looking at that list — and Escape, or a click
        // anywhere else, hands the arrows back to the notes and clips.
        if self.preset_focus.is_some() {
            match &event.logical_key {
                Key::Named(NamedKey::ArrowDown) => {
                    self.step_preset_focus(1);
                    return;
                }
                Key::Named(NamedKey::ArrowUp) => {
                    self.step_preset_focus(-1);
                    return;
                }
                // *"press enter while its selected"* — the keyboard half of a
                // double-click.
                Key::Named(NamedKey::Enter) => {
                    self.choose_focused_preset();
                    return;
                }
                Key::Named(NamedKey::Escape) => {
                    self.preset_focus = None;
                    self.tree.invalidate(BROWSER);
                    return;
                }
                _ => {}
            }
        }

        // Transport, undo and save mean the same thing wherever they are
        // pressed, so they are answered before anything that depends on which
        // canvas has the keyboard — and the editor windows answer them with
        // this same function. See `global_key`.
        if self.global_key(event) {
            return;
        }

        match &event.logical_key {
            Key::Named(NamedKey::Delete) | Key::Named(NamedKey::Backspace) => {
                // Whichever canvas was last pressed owns the key: "delete the
                // selection" has two meanings and no reading that means both.
                if self.focus == Focus::Timeline {
                    // Points before clips: while some are selected, Delete
                    // means them, and the block they sit on stays.
                    let mut edits = self.timeline.delete_points();
                    if edits.is_empty() {
                        edits = self.timeline.delete_selection();
                    }
                    self.apply_arrange_edits(edits);
                } else {
                    let edits = self.roll.delete_selection();
                    self.apply_roll_edits(edits);
                }
                if let Some(doc) = &mut self.options.document {
                    doc.end_gesture();
                }
            }
            Key::Named(NamedKey::Escape) => {
                // A menu that is open is what Escape is *for*; only once it is
                // shut does Escape mean "drop the selection".
                if let Some((target, menu)) = self.menu.take() {
                    // Escaping the import question drops it, the way clicking
                    // away from it does.
                    if matches!(target, MenuTarget::ImportChoice)
                        && let Some(doc) = &mut self.options.document
                    {
                        doc.cancel_import();
                    }
                    self.tree.invalidate_rect(menu.frame);
                    return;
                }
                if self.tools_panel.take().is_some() {
                    self.tree.invalidate(PANEL);
                    return;
                }
                if self.lane_menu.take().is_some() {
                    self.tree.invalidate(PANEL);
                    return;
                }
                self.roll.clear_selection();
                self.timeline.clear_selection();
                self.tree.invalidate(PANEL);
                self.tree.invalidate(TIMELINE);
            }
            Key::Named(NamedKey::ArrowUp) => self.arrow(0, 1),
            Key::Named(NamedKey::ArrowDown) => self.arrow(0, -1),
            Key::Named(NamedKey::ArrowLeft) => self.arrow(-1, 0),
            Key::Named(NamedKey::ArrowRight) => self.arrow(1, 0),
            Key::Character(c) => {
                let c = c.to_lowercase();
                match c.as_str() {
                    "a" if ctrl => {
                        if let Some(doc) = &self.options.document {
                            self.roll.select_all(doc.notes());
                            self.tree.invalidate(PANEL);
                        }
                    }
                    "c" if ctrl => self.copy(),
                    "x" if ctrl => self.cut(),
                    "v" if ctrl => self.paste(),
                    "b" if ctrl => self.duplicate(),
                    "d" if ctrl => self.duplicate(),
                    // Mute what is selected on the arrangement. **Shifted**,
                    // since Ctrl+M became the metronome — see `global_key`.
                    "m" if ctrl && self.modifiers.shift_key() => {
                        let edits = self.timeline.toggle_mute(&self.clips);
                        self.apply_arrange_edits(edits);
                        if let Some(doc) = &mut self.options.document {
                            doc.end_gesture();
                        }
                    }
                    // *"pressing M while having a mixer track selected toggles
                    // its mute and pressing N solos."* Only while the mixer is
                    // the open tab: bare letters belong to whichever canvas is
                    // showing, the same rule the tool keys follow, and `M` over
                    // the roll is not about a mixer strip. See
                    // `canvas::mixer_key`.
                    "m" | "n" if !ctrl && self.tab == EditorTab::Mixer => {
                        if let Some(key) = crate::canvas::mixer_key(&c) {
                            self.toggle_selected_track(key);
                        }
                    }
                    // Show and hide the arrangement strip.
                    "t" if ctrl => self.toggle_timeline(),
                    // **Two things, and the selection decides which.** FL
                    // binds Ctrl+L to Quick Legato in the piano roll, which is
                    // what was asked for: *"if i press ctrl l with a note
                    // selection in the piano roll it makes all the notes
                    // lengths not have gaps."* With no notes in hand there is
                    // no phrase to close up, and the key keeps the job it had
                    // — song or clip, which the chip on the transport bar also
                    // does but which a narrow window has no room to show (see
                    // `MIN_RULER_WIDTH`), so the mode stays reachable whatever
                    // the bar had room for.
                    //
                    // The split is safe because the two can never both apply:
                    // legato needs a note selection in the roll, and the play
                    // mode is not about notes at all.
                    "l" if ctrl => self.legato_or_play_mode(),
                    "f" if ctrl => {
                        self.searching = true;
                        self.tree.invalidate(BROWSER);
                    }
                    // Tools, from the FL keymap.
                    "p" => self.pick_tool(Tool::Draw, crate::canvas::TimelineTool::Draw),
                    "b" => self.set_tool(Tool::Paint),
                    "e" => self.pick_tool(Tool::Select, crate::canvas::TimelineTool::Select),
                    "d" => self.set_tool(Tool::Delete),
                    // Snap, cycled rather than given six bindings nobody would
                    // remember.
                    "s" => self.cycle_snap(),
                    "l" => self.cycle_lane_property(),
                    "g" => self.cycle_ghosts(),
                    // `A` for a slide: `S` is the snap and every other letter
                    // in the word is a tool. FL uses a right-click menu, which
                    // this window does not have yet.
                    "a" => self.toggle_slide(),
                    // The Tools panel. `T` for tools, and no FL binding to
                    // clash with — FL has no equivalent panel.
                    "t" => self.toggle_tools_panel(),
                    // The cut tool. `C` is FL's, and `Ctrl+C` is copy — the
                    // modifier is what keeps them apart, as it does for `B`
                    // (paint) and `Ctrl+B` (duplicate).
                    "c" => self.pick_tool(Tool::Slice, crate::canvas::TimelineTool::Slice),
                    "+" | "=" => self.zoom(1.25, 1.0),
                    "-" | "_" => self.zoom(0.8, 1.0),
                    // *"swap between piano roll and mixer by pressing 1 and
                    // 2."* The number row used to pick tools, a second
                    // binding for keys that already had FL's letters, and
                    // went unused for exactly that reason.
                    "1" | "2" => {
                        if let Some(tab) = crate::layout::editor_tab_for_key(&c) {
                            self.show_tab(tab);
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn set_query(&mut self, query: String) {
        if let Some(doc) = &mut self.options.document {
            doc.set_query(&query);
        }
        self.query = query;
        self.file_scroll = 0;
        self.preset_scroll = 0;
    }

    fn copy(&mut self) {
        // Which canvas the keyboard belongs to decides what `Ctrl+C` means —
        // the same rule `duplicate` has always followed, and the reason
        // copying on the arrangement used to copy *notes*.
        if self.focus == Focus::Timeline {
            let edits = self.timeline.copy();
            let copied = edits.len();
            self.apply_arrange_edits(edits);
            if copied > 0 {
                self.status = format!("{} clip(s) copied", self.timeline.selection().len().max(1));
            }
            return;
        }
        if let Some(doc) = &self.options.document {
            let n = self.roll.copy(doc.notes());
            if n > 0 {
                self.status = format!("{n} note(s) copied");
            }
        }
    }

    fn cut(&mut self) {
        if self.focus == Focus::Timeline {
            let edits = self.timeline.cut();
            self.apply_arrange_edits(edits);
            if let Some(doc) = &mut self.options.document {
                doc.end_gesture();
            }
            return;
        }
        let edits = match &self.options.document {
            Some(doc) => self.roll.cut(doc.notes()),
            None => Vec::new(),
        };
        self.apply_roll_edits(edits);
        if let Some(doc) = &mut self.options.document {
            doc.end_gesture();
        }
    }

    fn paste(&mut self) {
        if self.focus == Focus::Timeline {
            // In song ticks, at the time marker — where play would start,
            // which is what a person means by "here" on an arrangement.
            let at = self
                .options
                .document
                .as_ref()
                .map_or(0, |doc| doc.playhead_song_tick(self.marker));
            let beats = self
                .options
                .document
                .as_ref()
                .map_or(BEATS_PER_BAR, |doc| doc.beats_per_bar());
            let edits = self.timeline.paste(at, beats);
            self.apply_arrange_edits(edits);
            if let Some(doc) = &mut self.options.document {
                doc.end_gesture();
            }
            return;
        }
        // At the playhead when it is over this clip, otherwise where the
        // pointer is — which is what people reach for when the song is
        // stopped somewhere else.
        let at = self
            .options
            .document
            .as_ref()
            .and_then(|doc| doc.playhead_tick(self.view.position_sample))
            .unwrap_or_else(|| x_to_tick(&self.roll.view, self.roll_layout.grid, self.cursor.0));
        // The roll snaps it — see `PianoRoll::paste`. Neither of the two ticks
        // above is ever on a line by itself.
        let beats = self
            .options
            .document
            .as_ref()
            .map_or(BEATS_PER_BAR, |doc| doc.beats_per_bar());
        let edits = self.roll.paste(at, beats);
        self.apply_roll_edits(edits);
        if let Some(doc) = &mut self.options.document {
            doc.end_gesture();
        }
    }

    /// One press of an arrangement toolbar button.
    ///
    /// It reuses the very same methods the keyboard does — `Ctrl+B` and the
    /// Repeat button are one code path — because a button that does *almost*
    /// what its shortcut does is worse than no button.
    fn activate_timeline(&mut self, control: crate::canvas::TimelineControl) {
        use crate::canvas::TimelineControl as C;
        match control {
            C::Draw | C::Select | C::Slice => {
                self.timeline.set_tool(match control {
                    C::Draw => crate::canvas::TimelineTool::Draw,
                    C::Select => crate::canvas::TimelineTool::Select,
                    _ => crate::canvas::TimelineTool::Slice,
                });
                self.tree.invalidate(TIMELINE);
            }
            C::Snap => self.open_snap_menu(true),
            // A setting, not an action: nothing happens to the selection,
            // the next edge drag is what changes.
            C::Stretch => {
                self.timeline.toggle_stretch();
                self.tree.invalidate(TIMELINE);
            }
            C::Repeat => self.repeat_timeline(),
            C::Loop => self.toggle_loop_timeline(),
            C::Cut => self.cut(),
            C::Copy => self.copy(),
            C::Paste => self.paste(),
            C::Mute => {
                let edits = self.timeline.toggle_mute(&self.clips);
                self.apply_arrange_edits(edits);
                if let Some(doc) = &mut self.options.document {
                    doc.end_gesture();
                }
            }
            C::ZoomOut | C::ZoomIn => {
                let grid = self.timeline_layout.grid;
                let factor = if control == C::ZoomIn { 1.25 } else { 0.8 };
                timeline_zoom_x(
                    &mut self.timeline.view,
                    grid,
                    crate::canvas::zoom_anchor(grid, self.cursor),
                    factor,
                );
                self.tree.invalidate(TIMELINE);
            }
        }
    }

    /// Bounces a row to audio, saying what happened either way.
    ///
    /// A render is slow and silent while it runs, so the one thing it must not
    /// do is finish without saying so.
    fn render_lane(
        &mut self,
        lane: usize,
        span: Option<(fontelle_types::Tick, fontelle_types::Tick)>,
    ) {
        let Some(doc) = &mut self.options.document else {
            return;
        };
        match doc.render_lane(lane, span) {
            Ok(message) => self.status = message,
            Err(e) => self.status = e,
        }
        self.lane_made();
        self.tree.invalidate(TIMELINE);
    }

    /// Makes the selected clips repeat their own content, or stops them.
    ///
    /// The button half of Shift-dragging a clip's right-hand grip. A clip that
    /// is not looping starts looping at its **current length** — so pressing
    /// Loop and then dragging the clip out is the same two-step everybody
    /// tries first, and it works.
    fn toggle_loop_timeline(&mut self) {
        let selection: Vec<fontelle_types::ClipId> = self.timeline.selection().to_vec();
        if selection.is_empty() {
            return;
        }
        // Off when all of it already loops, on otherwise — the same rule the
        // Mute button follows, so a mixed selection settles rather than
        // flipping each clip against the others.
        let all_looping = self
            .clips
            .iter()
            .filter(|clip| selection.contains(&clip.id))
            .all(|clip| clip.loop_length.is_some());
        let edits = if all_looping {
            vec![crate::canvas::ArrangeEdit::SetLoop {
                ids: selection,
                loop_length: None,
            }]
        } else {
            // Each clip loops at its own length, so a selection of different
            // lengths does not all become one period.
            self.clips
                .iter()
                .filter(|clip| selection.contains(&clip.id) && clip.loop_length.is_none())
                .map(|clip| crate::canvas::ArrangeEdit::SetLoop {
                    ids: vec![clip.id],
                    loop_length: Some(clip.length),
                })
                .collect()
        };
        self.apply_arrange_edits(edits);
        if let Some(doc) = &mut self.options.document {
            doc.end_gesture();
        }
    }

    /// The selection again, laid after itself. `Ctrl+B` on the arrangement and
    /// the Repeat button both come here.
    fn repeat_timeline(&mut self) {
        let beats = self
            .options
            .document
            .as_ref()
            .map_or(BEATS_PER_BAR, |doc| doc.beats_per_bar());
        let edits = self.timeline.repeat(&self.clips, 1, beats);
        self.apply_arrange_edits(edits);
        if let Some(doc) = &mut self.options.document {
            doc.end_gesture();
        }
    }

    fn duplicate(&mut self) {
        if self.focus == Focus::Timeline {
            // The same path the Repeat button takes — see `activate_timeline`.
            self.repeat_timeline();
            return;
        }
        let edits = match &self.options.document {
            Some(doc) => {
                let beats = doc.beats_per_bar();
                self.roll.duplicate(doc.notes(), beats)
            }
            None => Vec::new(),
        };
        self.apply_roll_edits(edits);
        if let Some(doc) = &mut self.options.document {
            doc.end_gesture();
        }
    }

    /// One arrow key, as a direction: `(x, y)` in `-1..=1`, with `y` up.
    ///
    /// Reported from using the window: *"we need to ensure our keybinds are
    /// expansive — I should be able to do ctrl and up or down arrow to move a
    /// selection in the piano roll up or down an octave."* There were no arrow
    /// keys at all, so the only way to move a note was to drag it, which is
    /// fine for a phrase you are placing and hopeless for one you are
    /// correcting.
    ///
    /// The map, and the reasoning for each:
    ///
    /// | | left/right | up/down |
    /// |---|---|---|
    /// | plain | one snap step | one semitone / one lane |
    /// | `Ctrl` | one bar | one **octave** |
    /// | `Shift` | the length | the property lane's value |
    /// | `Ctrl+Shift` | the length, by a bar | that value, by ten |
    ///
    /// Which canvas it lands on is [`Focus`], the same rule `Delete` follows:
    /// "move the selection" has two meanings and no reading that means both.
    fn arrow(&mut self, dx: i32, dy: i32) {
        let ctrl = self.modifiers.control_key();
        let shift = self.modifiers.shift_key();
        let Some(doc) = &self.options.document else {
            return;
        };
        let beats_per_bar = doc.beats_per_bar();
        let bar = fontelle_types::PPQN * fontelle_types::Tick::from(beats_per_bar.max(1));

        if self.focus == Focus::Timeline {
            let step = if ctrl {
                bar
            } else {
                self.timeline_step(beats_per_bar)
            };
            let edits = if shift && dx != 0 {
                self.timeline
                    .resize_selection(&self.clips, step * fontelle_types::Tick::from(dx as i8))
            } else {
                self.timeline.nudge(
                    &self.clips,
                    step * fontelle_types::Tick::from(dx as i8),
                    -dy,
                )
            };
            self.apply_arrange_edits(edits);
            if let Some(doc) = &mut self.options.document {
                doc.end_gesture();
            }
            self.tree.invalidate(TIMELINE);
            return;
        }

        if self.tab != EditorTab::Roll {
            // The roll is not on screen, and editing a selection nobody can
            // see is how you find out later that you transposed something.
            return;
        }
        let notes = doc.notes();
        let edits = if dx != 0 {
            let step = if ctrl {
                bar
            } else {
                self.roll.step(beats_per_bar)
            };
            let by = step * fontelle_types::Tick::from(dx as i8);
            if shift {
                self.roll.resize_selection(notes, by)
            } else {
                self.roll.nudge(notes, by, 0)
            }
        } else if shift {
            // The property lane's value, so velocity can be dialled in from
            // the keyboard rather than by aiming at a three-pixel bar.
            let by = if ctrl { 10 } else { 1 } * dy;
            self.roll.nudge_property(notes, by)
        } else {
            let by = if ctrl { 12 } else { 1 } * dy;
            self.roll.nudge(notes, 0, by as i16)
        };
        self.apply_roll_edits(edits);
        // Arrow keys sound what they touch, exactly as a drag does.
        self.sound_roll_request();
        if let Some(doc) = &mut self.options.document {
            doc.end_gesture();
        }
        self.tree.invalidate(PANEL);
    }

    /// One arrow key's worth of song time on the arrangement.
    fn timeline_step(&self, beats_per_bar: u32) -> fontelle_types::Tick {
        match crate::canvas::snap_unit(self.timeline.view.snap, beats_per_bar) {
            0 => fontelle_types::PPQN,
            unit => unit,
        }
    }

    /// `P` and `E` — the two tools both canvases have — go to whichever one
    /// the keyboard belongs to.
    ///
    /// The same rule Delete and Ctrl+C already follow. Without it, pressing
    /// `P` while working on the arrangement would silently change the *roll's*
    /// tool, which is the kind of thing you only notice three clips later.
    fn pick_tool(&mut self, roll: Tool, arrangement: crate::canvas::TimelineTool) {
        match self.focus {
            Focus::Timeline => {
                self.timeline.set_tool(arrangement);
                self.tree.invalidate(TIMELINE);
            }
            Focus::Roll => self.set_tool(roll),
        }
    }

    fn set_tool(&mut self, tool: Tool) {
        self.roll.tool = tool;
        self.tree.invalidate(PANEL);
    }

    /// `Ctrl+L`: the legato tool when the roll has a selection, and the play
    /// mode when it has not. See the binding for why one key does both.
    fn legato_or_play_mode(&mut self) {
        if self.focus == Focus::Roll && !self.roll.selection().is_empty() {
            // Through `run_tool`, which is the Tools menu's own path: the key
            // and the menu row are one gesture with two ways in, down to the
            // undo entry and the line in the status bar.
            self.run_tool(ToolAction::Legato);
            self.tree.invalidate(PANEL);
            return;
        }
        self.toggle_play_mode();
    }

    fn cycle_snap(&mut self) {
        self.roll.view.snap = self.roll.view.snap.next();
        self.tree.invalidate(PANEL);
    }

    /// Drops the snap chip's list under it — the roll's, or the arrangement's.
    ///
    /// Under the chip rather than at the pointer, because that is what a
    /// drop-down is: the list appears where the value was.
    fn open_snap_menu(&mut self, timeline: bool) {
        let chip = if timeline {
            self.timeline_bar
                .items
                .iter()
                .find(|(control, _)| *control == crate::canvas::TimelineControl::Snap)
                .map(|(_, rect)| *rect)
        } else {
            self.roll_bar
                .items
                .iter()
                .find(|(control, _)| *control == RollControl::Snap)
                .map(|(_, rect)| *rect)
        };
        let Some(chip) = chip.filter(|rect| !rect.is_empty()) else {
            // A toolbar too narrow to draw the chip has no chip to hang a
            // list under; `S` and the arrangement's own key still work.
            return;
        };
        let bounds = self.layout.window;
        self.open_menu(MenuTarget::Snap { timeline }, chip.x, chip.bottom(), bounds);
    }

    fn undo(&mut self) {
        if let Some(doc) = &mut self.options.document {
            doc.undo();
            self.roll.clear_selection();
        }
        self.tree.invalidate(PANEL);
        self.refresh_title();
    }

    fn redo(&mut self) {
        if let Some(doc) = &mut self.options.document {
            doc.redo();
            self.roll.clear_selection();
        }
        self.tree.invalidate(PANEL);
        self.refresh_title();
    }

    fn save(&mut self) {
        // **A studio with no file asks for a name rather than refusing.**
        //
        // > *"when im not in a project yet, i currently cant save that blank
        // > no project into a new project ... if i try to save and theirs no
        // > project directory it can just make a new one."*
        //
        // The same move a first recording already made (`Session::take_path`),
        // with the name asked for instead of assumed.
        if self
            .options
            .document
            .as_ref()
            .is_some_and(|doc| !doc.has_file())
        {
            let seed = self
                .options
                .document
                .as_ref()
                .map(|doc| doc.name().to_string())
                .unwrap_or_default();
            self.ask_for_a_name(NameFor::SaveAs, seed);
            return;
        }
        let Some(doc) = &mut self.options.document else {
            return;
        };
        match doc.save() {
            Ok(()) => {
                println!("Fontelle: saved");
                self.status = "saved".to_string();
            }
            Err(e) => {
                eprintln!("Fontelle: could not save — {e}");
                self.status = format!("could not save — {e}");
            }
        }
        self.tree.invalidate(BROWSER);
        self.refresh_title();
    }

    /// Keeps the dirty marker in the OS title bar honest (§17, item 10's
    /// smallest useful half).
    fn refresh_title(&mut self) {
        let Some(doc) = &self.options.document else {
            return;
        };
        let title = format!(
            "{}{} — Fontelle",
            doc.name(),
            if doc.is_dirty() { " •" } else { "" }
        );
        if let Some(live) = &self.live {
            live.window.set_title(&title);
        }
    }

    /// Asks for a frame only when there is one to draw.
    ///
    /// The counterpart to `Redraw::take_dirty` returning `None`: between them,
    /// a window with nothing happening in it neither requests nor issues a
    /// frame (§16.3).
    fn request_redraw_if_dirty(&mut self) {
        if !self.tree.has_dirty_regions() {
            return;
        }
        if let Some(live) = &self.live {
            live.window.request_redraw();
        }
    }

    /// Decides when the loop is allowed to wake next.
    ///
    /// The policy itself is [`sleep_budget`], which is tested; this is the
    /// three lines that hand it to winit, plus the `--run-for` deadline, which
    /// only ever brings the wake-up *forward*.
    ///
    /// Note that even the animating case is `WaitUntil` and not `Poll`: the
    /// thread sleeps between frames rather than spinning.
    fn arm_deadline(&mut self, event_loop: &ActiveEventLoop) {
        let now = std::time::Instant::now();

        let mut wake = match sleep_budget(
            self.tree.redraw().is_animating(),
            self.options.host.is_some(),
        ) {
            Sleep::Forever => None,
            Sleep::AtMost(budget) => Some(now + budget),
        };

        // **A plugin editor holds the loop awake.** It is drawn by somebody
        // else's code on this thread, and the only thing that runs that code
        // is the next pass of this loop — so a studio that slept until the
        // next click would be a plugin window that froze between them.
        if self.plugin_editor_open {
            wake = Some(wake.map_or(now + PLUGIN_EDITOR_FRAME, |w| {
                w.min(now + PLUGIN_EDITOR_FRAME)
            }));
        }

        // A note waiting out its minimum length has to be woken for, or a
        // window that goes back to sleep the instant the mouse comes up leaves
        // it sounding until something else happens to it.
        if let Some(due) = self.audition.due() {
            wake = Some(wake.map_or(due, |w| w.min(due)));
        }

        // And a tip waiting out its dwell, for exactly the same reason: the
        // pointer coming to rest is the *last* event, so nothing else will
        // wake the window to draw the tip that rest earned.
        if self.hover_tip.is_some() && self.due_tip().is_none() {
            let due = self.hover_since + crate::tooltip::TOOLTIP_DELAY;
            wake = Some(wake.map_or(due, |w| w.min(due)));
        }

        if let Some(limit) = self.options.run_for {
            if self.expired() {
                event_loop.exit();
                return;
            }
            let deadline = now + (limit - self.started.elapsed());
            wake = Some(wake.map_or(deadline, |w| w.min(deadline)));
        }

        event_loop.set_control_flow(match wake {
            Some(at) => ControlFlow::WaitUntil(at),
            None => ControlFlow::Wait,
        });
    }
}

/// The window `fontelle` opens when it is run with no arguments.
pub fn run() -> Result<WindowApp, WindowError> {
    run_window(WindowOptions::default())
}

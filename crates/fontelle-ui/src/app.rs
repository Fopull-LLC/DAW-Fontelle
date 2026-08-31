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
    BrowserHit, BrowserLayout, BrowserMode, DEFAULT_LANE_HEIGHT, InstrumentLayout, InstrumentView,
    MixerHit, MixerLayout, Modifiers, MouseButton, ParamKind, PianoRoll, RackHit, RackLayout,
    RollControl, RollLayout, RouteChoice, RouteMenu, SnapDivision, Timeline, TimelineHit,
    TimelineLayout, Tool, ToolbarLayout, browser_hit, browser_layout, browser_layout_for,
    clamp_to_grid, edge_scroll, fader_db_at, format_gain_db, format_pan, instrument_hit,
    instrument_layout, keyboard_width, knob_value, lane_height_at, mixer_hit, mixer_layout,
    next_value, pan_at, rack_hit, rack_layout, roll_layout_with_keys, route_label, route_menu_hit,
    route_menu_layout, scrolled, snap_tick, timeline_hit, timeline_layout, timeline_snap,
    timeline_toolbar_hit, timeline_toolbar_layout, timeline_x_to_tick, timeline_zoom_x,
    timeline_zoom_y, toolbar_hit, toolbar_layout, x_to_tick, y_to_key, zoom_x, zoom_y,
};
use crate::canvas::{lane_menu_hit, lane_menu_layout};
use crate::document::{ChannelInfo, ClipInfo, LaneInfo, LibraryEntry, MixerStrip, StudioHost};
use crate::layout::{
    DEFAULT_TIMELINE_HEIGHT, Docks, EditorTab, EditorTabs, WindowLayout, editor_tab_at,
    editor_tabs, rack_share_at, sidebar_width_at, timeline_height_at, window_layout_with,
};
use crate::pointer::{Pointer, PointerScene, pointer_at};
use crate::render::{
    ADD_CHANNEL, ARRANGEMENT, BrowserChrome, CHOOSE_FOLDER, Chrome, EMPTY_LANE, EXPORT,
    InstrumentChrome, MixerChrome, NEW_PROJECT, NO_INSTRUMENT, OPEN_FOLDER, RackChrome,
    RenderError, RollChrome, SEARCH_HINT, TAB_INSTRUMENT, TAB_MIXER, TAB_ROLL, TimelineChrome,
    TransportChrome, draw_window, key_name, label_stride, labelled_bar,
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
    /// Notes: drawing, moving, resizing, or a selection box.
    Roll,
    /// Values in the property lane.
    Lane,
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
    /// A knob on the instrument editor.
    Knob,
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
    /// Points on an automation curve. **Relative**, like the roll's note drag
    /// and for the same reason: several can move at once and they have to keep
    /// their shape.
    AutomationPoints,
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
    /// Which insert the effect tab is showing: the strip, and the slot in its
    /// chain. `None` closes the tab — a tab for a thing that is not there is a
    /// tab that does nothing when clicked.
    open_insert: Option<(usize, usize)>,
    /// That insert's parameters, read with the rest of the studio's lists
    /// rather than once a frame.
    eq: Option<fontelle_types::EqConfig>,
    eq_layout: crate::canvas::EqLayout,
    eq_curve: Vec<(f32, f32)>,
    hover_band: Option<usize>,
    /// The automation clip the editor has open, as the host describes it, and
    /// where its points are on screen.
    automation: Option<crate::canvas::AutomationView>,
    automation_layout: crate::canvas::AutomationLayout,
    automation_curve: Vec<(f32, f32)>,
    hover_point: Option<fontelle_types::PointId>,
    hover_param: Option<(usize, usize)>,
    hover_tab: Option<EditorTab>,
    /// The control being dragged, where the drag started, and the value it
    /// started from — a knob measures from where it was grabbed, not from
    /// wherever the pointer happens to be.
    knob: Option<((usize, usize), f32, f32)>,

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
    /// How many soundfonts the whole collection holds — the browser panel's
    /// heading, which does not change as you browse into a folder.
    library_count: usize,
    /// The live search (TDD §17.5), and whether it has the keyboard.
    query: String,
    searching: bool,
    status: String,
    rack_scroll: usize,
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
    /// What the held mouse button is doing, so a `CursorMoved` reaches the one
    /// thing that asked for it.
    drag: Drag,
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
    /// When a backup was last considered. See `maybe_autosave`.
    last_autosave: std::time::Instant,
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
                body: layout.panel.body,
                headings: Vec::new(),
                cells: Vec::new(),
                content_height: 0.0,
            },
            open_insert: None,
            eq: None,
            eq_layout: crate::canvas::eq_layout(
                layout.panel.body,
                &options.theme.metrics,
                &fontelle_types::EqConfig::new(),
            ),
            eq_curve: Vec::new(),
            hover_band: None,
            automation: None,
            automation_layout: crate::canvas::AutomationLayout {
                body: layout.panel.body,
                grid: layout.panel.body,
                header: layout.panel.body,
                handles: Vec::new(),
            },
            automation_curve: Vec::new(),
            hover_point: None,
            hover_param: None,
            hover_tab: None,
            knob: None,
            mixer: MixerLayout {
                body: layout.panel.body,
                list: layout.panel.body,
                strips: Vec::new(),
                master: None,
                total: 0,
                scroll: 0,
            },
            mixer_strips: Vec::new(),
            mixer_peaks: Vec::new(),
            mixer_scroll: 0,
            hover_mixer: None,
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
            projects: Vec::new(),
            library_count: 0,
            query: String::new(),
            searching: false,
            status: String::new(),
            key_map: crate::document::KeyMap::unknown(),
            timeline_bar: crate::canvas::TimelineToolbar { items: Vec::new() },
            hover_timeline: None,
            rack_scroll: 0,
            file_scroll: 0,
            preset_scroll: 0,
            hover_control: None,
            hover_browser: None,
            hover_rack: None,
            route_menu: None,
            route_names: Vec::new(),
            pointer: Pointer::Default,
            tool_cursors: std::collections::HashMap::new(),
            browser_title: "Soundfonts".to_string(),
            drag: Drag::None,
            audition: Auditions::default(),
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

        // §16.3, the whole of it: no dirty region, no frame.
        let Some(_region) = self.tree.take_dirty() else {
            return;
        };
        let Some(live) = &self.live else { return };

        // Built before the renderer is borrowed: it reads the strip names and
        // the open insert, and the borrow below is a mutable one of `self`.
        let effect = (self.tab == EditorTab::Effect)
            .then(|| {
                self.eq.map(|config| crate::render::EffectChrome {
                    layout: self.eq_layout.clone(),
                    config,
                    curve: self.eq_curve.clone(),
                    hover: self.hover_band,
                    active: match self.drag {
                        Drag::EqHandle(band) => Some(band),
                        _ => None,
                    },
                    title: self.effect_title(),
                    bypassed: self.open_insert_bypassed(),
                })
            })
            .flatten();

        let automation = (self.tab == EditorTab::Automation)
            .then(|| {
                self.automation
                    .as_ref()
                    .map(|view| crate::render::AutomationChrome {
                        layout: self.automation_layout.clone(),
                        view: view.clone(),
                        curve: self.automation_curve.clone(),
                        hover: self.hover_point,
                    })
            })
            .flatten();

        let device = &self.context.devices[live.surface.dev_id];
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
                    hover: self.hover,
                    marker_sample: self.marker,
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
                    marker_tick: doc.playhead_tick(self.marker),
                    marquee: self.roll.marquee(),
                    hover: self.hover_control,
                    lane_menu: self.lane_menu.as_ref(),
                    slice: self.roll.slice_stroke(),
                }),
                rack: self.options.document.as_ref().map(|_| RackChrome {
                    panel: self.layout.rack,
                    layout: self.rack.clone(),
                    channels: &self.channels,
                    selected: self.selected_channel,
                    hover: self.hover_rack,
                    route_names: &self.route_names,
                    strips: self.route_names.len(),
                    route_menu: self.route_menu.as_ref().map(|(_, menu)| menu),
                    route_menu_open: self.route_menu.as_ref().map(|(index, _)| *index),
                }),
                browser: self.options.document.as_ref().map(|_| BrowserChrome {
                    panel: self.layout.browser,
                    layout: self.browser.clone(),
                    mode: self.browser_mode,
                    query: &self.query,
                    files: match self.browser_mode {
                        BrowserMode::Sounds => &self.files,
                        BrowserMode::Projects => &self.projects,
                    },
                    presets: &self.presets,
                    selected_file: self.selected_file,
                    selected_preset: self.selected_preset,
                    searching: self.searching,
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
                    hover: self.hover_timeline,
                    can_paste: self
                        .options
                        .document
                        .as_ref()
                        .is_some_and(|doc| doc.clip_clipboard_len() > 0),
                }),
                instrument: self.instrument.as_ref().map(|view| InstrumentChrome {
                    layout: self.instrument_layout.clone(),
                    view,
                    hover: self.hover_param,
                    active: self.knob.map(|(which, _, _)| which),
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
                }),
                effect,
                automation,
                tabs: self.tabs,
                tab: self.tab,
                hover_tab: self.hover_tab,
                browser_title: &self.browser_title,
                labels: &self.labels,
                status: &self.status,
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

        let view = match &mut self.options.host {
            Some(host) => host.view(),
            None => TransportView::unavailable(),
        };
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

        // Whether anything on screen will change without the user doing
        // something: a rolling transport moves the playhead, and a meter above
        // the floor is still falling. Held as an animator on the tree, which
        // is what `arm_deadline` reads — one source of truth for "may the
        // window sleep", rather than two that can disagree.
        let moving = view.playing
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
            Drag::LaneGrip | Drag::Divider | Drag::Knob | Drag::Lane | Drag::SidebarSplit => {
                Some(Pointer::ResizeY)
            }
            Drag::SidebarSeam => Some(Pointer::ResizeX),
            Drag::Keys => Some(Pointer::Hand),
            // A fader and the tempo box are both vertical throws; a pan is a
            // horizontal one.
            Drag::Fader(_) | Drag::Tempo => Some(Pointer::ResizeY),
            Drag::Pan(_) => Some(Pointer::ResizeX),
            // A band handle goes wherever the pointer does, in both axes.
            Drag::EqHandle(_) | Drag::AutomationPoints => Some(Pointer::Grabbing),
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
        live.window.set_cursor(match wanted {
            Pointer::Default => winit::window::CursorIcon::Default,
            Pointer::Hand => winit::window::CursorIcon::Pointer,
            Pointer::Text => winit::window::CursorIcon::Text,
            // A note's or a clip's right-hand edge: it moves left and right,
            // which `EResize` says more precisely than a two-headed arrow.
            Pointer::ResizeX => winit::window::CursorIcon::EResize,
            Pointer::ResizeY => winit::window::CursorIcon::NsResize,
            Pointer::Grab => winit::window::CursorIcon::Grab,
            Pointer::Grabbing => winit::window::CursorIcon::Grabbing,
            // Only reached when the custom cursor could not be built — an
            // old compositor, or a platform with no custom cursors at all.
            Pointer::Draw | Pointer::Paint | Pointer::Select | Pointer::Cut => {
                winit::window::CursorIcon::Crosshair
            }
            Pointer::Erase => winit::window::CursorIcon::NotAllowed,
        });
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
        let param = (self.tab == EditorTab::Instrument)
            .then(|| instrument_hit(&self.instrument_layout, x, y))
            .flatten();
        let strip = (self.tab == EditorTab::Mixer)
            .then(|| mixer_hit(&self.mixer, x, y))
            .filter(|hit| *hit != MixerHit::Nothing);
        let band = (self.tab == EditorTab::Effect)
            .then(|| match crate::canvas::eq_hit(&self.eq_layout, x, y) {
                crate::canvas::EqHit::Handle(band) => Some(band),
                _ => None,
            })
            .flatten();
        if tab != self.hover_tab
            || param != self.hover_param
            || strip != self.hover_mixer
            || band != self.hover_band
        {
            self.hover_tab = tab;
            self.hover_param = param;
            self.hover_mixer = strip;
            self.hover_band = band;
            self.tree.invalidate(PANEL);
        }
        let over_rack = self.layout.rack.frame.contains(x, y);
        let rack = over_rack
            .then(|| rack_hit(&self.rack, x, y))
            .filter(|hit| *hit != RackHit::Nothing);
        if rack != self.hover_rack {
            self.hover_rack = rack;
            self.tree.invalidate(RACK);
        }
        self.update_cursor();
    }

    /// The browser panel's own heading, carrying the count so the number of
    /// soundfonts is visible without a line of its own.
    fn browser_heading(&self) -> String {
        // How many soundfonts the **collection** holds, which does not change
        // as you walk into a folder. Counting the rows in front of you would
        // say "3" inside a folder of three and read as the collection having
        // shrunk — and would count folders as soundfonts besides.
        match self.library_count {
            0 => "Soundfonts".to_string(),
            n => format!("Soundfonts \u{2014} {n}"),
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

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
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
                self.knob = None;
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
                        | Drag::Fader(_)
                        | Drag::Pan(_)
                        | Drag::Tempo
                        | Drag::EqHandle(_)
                        | Drag::AutomationPoints
                );
                // An EQ band drag coalesces into one history entry while it
                // runs; this is what tells the document it has stopped, the
                // same handshake a note drag has.
                if matches!(self.drag, Drag::EqHandle(_) | Drag::AutomationPoints)
                    && let Some(doc) = &mut self.options.document
                {
                    doc.end_gesture();
                }
                self.drag = Drag::None;
                self.knob = None;
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
                self.timeline
                    .release_over(x, y, &self.timeline_layout, &self.clips);
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
        // The graph the audio thread handed back is freed here, on this
        // thread — see `fontelle_engine::GraphPublisher`. Cheap, and it has to
        // happen somewhere that runs whether or not a frame does.
        if let Some(doc) = &mut self.options.document {
            doc.pump();
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
            keyboard_width(&self.key_map),
        );
        self.roll_bar = toolbar_layout(self.roll_layout.toolbar, m);
        self.timeline_layout = timeline_layout(self.layout.timeline.body, m);
        self.timeline_bar = timeline_toolbar_layout(self.timeline_layout.toolbar, m);
        self.tabs = crate::layout::editor_tabs_full(
            self.layout.panel.header,
            m,
            self.open_insert.is_some(),
            self.automation.is_some(),
        );
        if let Some(view) = &self.automation {
            self.automation_layout =
                crate::canvas::automation_layout(self.layout.panel.body, m, view);
            self.automation_curve = match &self.options.document {
                Some(doc) => match doc.automation_data() {
                    Some(data) => {
                        crate::canvas::automation_curve(&self.automation_layout, view, &data)
                    }
                    None => Vec::new(),
                },
                None => Vec::new(),
            };
        }
        let config = self.eq.unwrap_or_default();
        self.eq_layout = crate::canvas::eq_layout(self.layout.panel.body, m, &config);
        self.eq_curve = crate::canvas::eq_curve_points(&self.eq_layout, &config);
        self.instrument_layout = match &self.instrument {
            Some(view) => instrument_layout(self.layout.panel.body, m, view),
            None => InstrumentLayout {
                body: self.layout.panel.body,
                headings: Vec::new(),
                cells: Vec::new(),
                content_height: 0.0,
            },
        };
        self.mixer = mixer_layout(
            self.layout.panel.body,
            m,
            &self.mixer_strips,
            self.mixer_scroll,
        );
        self.rack = rack_layout(
            self.layout.rack.body,
            m,
            self.channels.len(),
            self.rack_scroll,
        );
        self.browser = browser_layout_for(
            self.layout.browser.body,
            m,
            self.browser_mode,
            match self.browser_mode {
                BrowserMode::Sounds => self.files.len(),
                BrowserMode::Projects => self.projects.len(),
            },
            self.presets.len(),
            self.file_scroll,
            self.preset_scroll,
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
        self.files = doc.library_files();
        self.presets = doc.library_presets();
        self.projects = doc.projects();
        self.library_count = doc.library_count();
        self.selected_channel = doc.selected_channel();
        self.selected_file = doc.selected_file();
        self.selected_preset = doc.selected_preset();
        self.lanes = doc.lanes();
        self.clips = doc.clips();
        self.instrument = doc.instrument();
        self.mixer_strips = doc.mixer_strips();
        self.route_names = doc.route_names();
        // The open insert may have been removed, or its whole strip may have —
        // in which case the tab closes rather than showing the effect that
        // happens to be at that index now.
        self.automation = doc.automation();
        if self.automation.is_none() && self.tab == EditorTab::Automation {
            self.tab = EditorTab::Mixer;
        }
        self.eq = match self.open_insert {
            Some((strip, slot)) => doc.eq_config(strip, slot),
            None => None,
        };
        if self.eq.is_none() {
            self.open_insert = None;
            if self.tab == EditorTab::Effect {
                self.tab = EditorTab::Mixer;
            }
        }
        self.tempo = doc.tempo();
        self.key_map = doc.key_map();
        let filter = self.roll.ghosts;
        self.ghosts = doc.ghost_notes(filter);
        self.query = doc.query().to_string();
        let fallback = match self.browser_mode {
            BrowserMode::Sounds => doc.library_status(),
            BrowserMode::Projects => doc.project_status(),
        };
        self.status = doc.take_message().unwrap_or(fallback);

        // A list that shrank under a scroll offset leaves a panel that looks
        // empty until somebody scrolls back up.
        self.file_scroll = self.file_scroll.min(self.files.len().saturating_sub(1));
        self.preset_scroll = self.preset_scroll.min(self.presets.len().saturating_sub(1));
        self.rack_scroll = self.rack_scroll.min(self.channels.len().saturating_sub(1));
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
        let font = self.options.theme.font.clone();
        let want = |labels: &mut Labels, text: &mut TextContext, s: &str| {
            labels.ensure(s, &font, text);
        };

        for fixed in [
            "Channels",
            ADD_CHANNEL,
            SEARCH_HINT,
            OPEN_FOLDER,
            CHOOSE_FOLDER,
            NEW_PROJECT,
            EXPORT,
            BrowserMode::Sounds.label(),
            BrowserMode::Projects.label(),
            "S",
            "M",
            ARRANGEMENT,
            EMPTY_LANE,
            TAB_ROLL,
            TAB_INSTRUMENT,
            TAB_MIXER,
            NO_INSTRUMENT,
        ] {
            want(&mut self.labels, &mut self.text, fixed);
        }
        if self.lane_menu.is_some() {
            for property in crate::canvas::LANE_PROPERTIES {
                want(&mut self.labels, &mut self.text, property.label());
            }
        }
        // The transport's two document boxes. Shaped rather than cached in
        // `Labels`, like the position read-out beside them: a tempo that is
        // being dragged never repeats a string, and caching one that never
        // repeats is a leak.
        if self.shaped_tempo != self.tempo {
            let tempo = format_tempo(self.tempo);
            self.tempo_text = self.text.layout(&tempo, &font, None);
            self.shaped_tempo = self.tempo;
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
        }

        let heading = self.browser_title.clone();
        want(&mut self.labels, &mut self.text, &heading);
        let ghost_caption = self.roll.ghosts.label();
        want(&mut self.labels, &mut self.text, &ghost_caption);
        let lane_caption = crate::canvas::lane_caption(self.roll.lane_property);
        want(&mut self.labels, &mut self.text, &lane_caption);
        for (control, _) in &self.roll_bar.items {
            let caption = match control {
                RollControl::Snap => self.roll.view.snap.label(),
                RollControl::Lane => &lane_caption,
                RollControl::Ghost => &ghost_caption,
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
                BrowserMode::Sounds => self.files.get(index).cloned(),
                BrowserMode::Projects => self.projects.get(index).cloned(),
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
        if self.tab == EditorTab::Instrument
            && let Some(view) = self.instrument.clone()
        {
            for group in &view.groups {
                self.labels.ensure(&group.name, &font, &mut self.text);
                for param in &group.params {
                    self.labels.ensure(&param.label, &font, &mut self.text);
                    self.labels.ensure(&param.display, &font, &mut self.text);
                }
            }
        }

        // The arrangement's toolbar. Its snap chip says its division, which
        // changes, so it is shaped from the live value rather than once.
        for (control, _) in &self.timeline_bar.items {
            let caption = match control {
                crate::canvas::TimelineControl::Snap => self.timeline.view.snap.label(),
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
                } else if key % 12 == 0 {
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

        // An open menu is above everything, including the transport bar, and a
        // click anywhere shuts it — which is what every menu on every desktop
        // does. Before the bar, or clicking Play with the menu up would both
        // start the song and leave the menu hanging over the roll.
        if self.lane_menu.is_some() {
            self.press_lane_menu(x, y);
            return;
        }
        if self.route_menu.is_some() {
            self.press_route_menu(x, y);
            return;
        }

        // The transport bar first: it is the only thing above the panels.
        if let Some(what) = hit(&self.bar, &self.view, x, y) {
            if button == winit::event::MouseButton::Left {
                // The two boxes that write to the *document* rather than to
                // the engine. `transport` would do nothing with either —
                // `action` returns `None` for them — so they are dealt with
                // here, where the document is reachable.
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
            self.press_rack(x, y);
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
            if self.tab != tab {
                self.tab = tab;
                // Nothing reads a track's meter while the mixer is hidden, and
                // a peak is the highest **since the last read** — so the first
                // frame of the tab would otherwise show the loudest moment
                // since it was last open, and then fall. Drain once and throw
                // it away.
                if tab == EditorTab::Mixer
                    && let Some(doc) = &mut self.options.document
                {
                    doc.mixer_peaks();
                    self.mixer_peaks.clear();
                }
                self.relayout_panels();
                self.tree.invalidate(PANEL);
            }
            return;
        }
        // Everything below is the editor, so the keyboard is now its.
        if self.focus != Focus::Roll {
            self.focus = Focus::Roll;
            self.tree.invalidate(TIMELINE);
            self.tree.invalidate(PANEL);
        }

        if self.tab == EditorTab::Instrument {
            if button == MouseButton::Left {
                self.press_instrument(x, y);
            }
            return;
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
                    _ => {}
                },
            }
            return;
        }
        if self.tab == EditorTab::Effect {
            self.press_effect_tab(button, x, y);
            return;
        }
        if self.tab == EditorTab::Automation {
            self.press_automation(button, x, y);
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
        } else if self.roll_layout.velocity.contains(x, y)
            || self.roll_layout.keys.contains(x, y)
            || self.roll_layout.ruler.contains(x, y)
        {
            return;
        }
        self.drag = Drag::Roll;
        self.press_roll(button, x, y);
    }

    /// One pointer move, sent to whatever the press decided this drag is.
    fn drag_pointer(&mut self) {
        let (x, y) = self.cursor;
        match self.drag {
            Drag::None => {}
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
            Drag::Knob => self.drag_knob(y),
            Drag::Fader(strip) => self.drag_fader(strip, y),
            Drag::EqHandle(band) => self.drag_eq(band, x, y),
            Drag::AutomationPoints => self.drag_automation(x, y),
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
                if let Some(doc) = &mut self.options.document {
                    doc.add_insert(strip, fontelle_types::EffectKind::Eq);
                }
                self.refresh_studio();
                self.refresh_title();
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
            // A track carries a channel, and the rack and the roll follow the
            // selection — so clicking a strip's name opens what it plays,
            // which is the same handshake clicking a clip already has.
            MixerHit::Name(strip) => {
                let channel = self
                    .mixer_strips
                    .get(strip)
                    .filter(|s| !s.is_master)
                    .map(|_| strip);
                if let (Some(index), Some(doc)) = (channel, &mut self.options.document)
                    && index < self.channels.len()
                {
                    doc.select_channel(index);
                }
            }
            MixerHit::Nothing => {}
        }
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
        self.refresh_studio();
        self.refresh_title();
        self.tab = EditorTab::Automation;
        self.relayout_panels();
        self.tree.invalidate(PANEL);
    }

    /// A press in the effect tab. Right-click makes an automation lane for
    /// whatever is under the pointer (§12.4).
    fn press_effect_tab(&mut self, button: MouseButton, x: f32, y: f32) {
        match button {
            MouseButton::Left => self.press_effect(x, y),
            MouseButton::Right => {
                if let crate::canvas::EqHit::Handle(band) =
                    crate::canvas::eq_hit(&self.eq_layout, x, y)
                {
                    // A band's *gain* is what a right-click on its handle
                    // means: it is the axis the handle moves vertically, and
                    // the one a sweep is nearly always drawn on.
                    self.automate_insert_param(&format!("band{}.gain", band + 1));
                }
            }
        }
    }

    /// Makes an automation lane for one parameter of the open insert.
    fn automate_insert_param(&mut self, param: &str) {
        let Some((strip, slot)) = self.open_insert else {
            return;
        };
        // Both worked out before the document is borrowed mutably.
        let label = format!("{} \u{2014} {param}", self.effect_title());
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
        self.refresh_studio();
        self.refresh_title();
        self.tab = EditorTab::Automation;
        self.relayout_panels();
        self.tree.invalidate(PANEL);
    }

    /// A press in the automation editor.
    ///
    /// A click on empty grid **makes a point there and picks it up**, which is
    /// the gesture every curve editor has and the same handshake drawing a
    /// note in the roll has. Right-click deletes.
    fn press_automation(&mut self, button: MouseButton, x: f32, y: f32) {
        let hit = crate::canvas::automation_hit(&self.automation_layout, x, y);
        let Some(view) = self.automation.clone() else {
            return;
        };
        match (button, hit) {
            (MouseButton::Left, crate::canvas::AutomationHit::Point(_)) => {
                self.drag = Drag::AutomationPoints;
                self.pointer_anchor = Some((x, y));
            }
            (MouseButton::Left, crate::canvas::AutomationHit::Grid) => {
                let grid = self.automation_layout.grid;
                let tick = crate::canvas::auto_tick_at(grid, view.length, x);
                let value = crate::canvas::auto_value_at(grid, y);
                if let Some(doc) = &mut self.options.document {
                    doc.edit_automation(crate::canvas::AutomationEdit::Add { tick, value });
                }
                self.drag = Drag::AutomationPoints;
                self.pointer_anchor = Some((x, y));
                self.refresh_studio();
                self.refresh_title();
                self.relayout_panels();
                self.tree.invalidate(PANEL);
            }
            (MouseButton::Right, crate::canvas::AutomationHit::Point(id)) => {
                if let Some(doc) = &mut self.options.document {
                    doc.edit_automation(crate::canvas::AutomationEdit::Remove(vec![id]));
                }
                self.refresh_studio();
                self.refresh_title();
                self.relayout_panels();
                self.tree.invalidate(PANEL);
            }
            _ => {}
        }
    }

    /// One step of an automation-point drag: deltas from where the pointer was
    /// last, so a whole gesture merges into one undo entry.
    fn drag_automation(&mut self, x: f32, y: f32) {
        let (Some(view), Some((from_x, from_y))) = (self.automation.clone(), self.pointer_anchor)
        else {
            return;
        };
        let selected: Vec<fontelle_types::PointId> = view
            .points
            .iter()
            .filter(|point| point.selected)
            .map(|point| point.id)
            .collect();
        if selected.is_empty() {
            return;
        }
        let grid = self.automation_layout.grid;
        let tick_delta = crate::canvas::auto_tick_at(grid, view.length, x)
            - crate::canvas::auto_tick_at(grid, view.length, from_x);
        let value_delta =
            crate::canvas::auto_value_at(grid, y) - crate::canvas::auto_value_at(grid, from_y);
        if tick_delta == 0 && value_delta.abs() < 1e-9 {
            return;
        }
        if let Some(doc) = &mut self.options.document {
            doc.edit_automation(crate::canvas::AutomationEdit::Move {
                ids: selected,
                tick_delta,
                value_delta,
            });
        }
        self.pointer_anchor = Some((x, y));
        self.refresh_studio();
        self.refresh_title();
        self.relayout_panels();
        self.tree.invalidate(PANEL);
    }

    /// A press in the EQ editor.
    ///
    /// A press on empty curve **switches the nearest unused band on** where it
    /// landed, which is the gesture every EQ has: you point at the frequency
    /// you want changed and drag. The alternative — eight handles sitting on a
    /// flat line waiting to be found — is eight things to knock by accident to
    /// save one click.
    fn press_effect(&mut self, x: f32, y: f32) {
        match crate::canvas::eq_hit(&self.eq_layout, x, y) {
            crate::canvas::EqHit::Handle(band) => {
                self.drag = Drag::EqHandle(band);
            }
            crate::canvas::EqHit::Curve => {
                let Some(config) = self.eq else { return };
                let Some(band) = config.bands.iter().position(|band| !band.enabled) else {
                    // Eight is all there is. Saying nothing is better than
                    // silently rewriting one somebody set.
                    return;
                };
                let Some((strip, slot)) = self.open_insert else {
                    return;
                };
                let mut value = config.bands[band];
                value.enabled = true;
                value.band_type = fontelle_types::BandType::Bell;
                value.freq_hz = crate::canvas::eq_freq_at(self.eq_layout.curve, x);
                value.gain_db = crate::canvas::eq_gain_at(self.eq_layout.curve, y);
                if let Some(doc) = &mut self.options.document {
                    doc.set_eq_band(strip, slot, band, value);
                }
                // And the drag continues from the handle it just made, so
                // placing a band and aiming it are one gesture — the same
                // handshake drawing a note and sizing it has.
                self.drag = Drag::EqHandle(band);
                self.refresh_studio();
                self.refresh_title();
                self.relayout_panels();
                self.tree.invalidate(PANEL);
            }
            crate::canvas::EqHit::Nothing => {}
        }
    }

    /// Opens one insert in the effect tab.
    ///
    /// Switching the tab as well as selecting the slot, because clicking a
    /// three-letter row in a mixer strip means "show me this" and a click that
    /// selected something out of sight would be a click that did nothing.
    fn open_insert(&mut self, strip: usize, slot: usize) {
        let config = self
            .options
            .document
            .as_ref()
            .and_then(|doc| doc.eq_config(strip, slot));
        if config.is_none() {
            return;
        }
        self.open_insert = Some((strip, slot));
        self.eq = config;
        self.tab = EditorTab::Effect;
        self.relayout_panels();
        self.tree.invalidate(PANEL);
    }

    /// What the effect panel calls what it is showing: the strip's name and
    /// the effect's, because "EQ" on its own does not say which track.
    fn effect_title(&self) -> String {
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
            ref kind => {
                let value = next_value(kind, param.value);
                self.set_param(&param.address, value);
                if let Some(doc) = &mut self.options.document {
                    doc.end_gesture();
                }
            }
        }
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
            if button == MouseButton::Left {
                self.drag = Drag::TimelineRuler;
                self.mark_on_timeline(x);
            }
            return;
        }
        if button == MouseButton::Left
            && let TimelineHit::Lane(lane) = timeline_hit(
                &self.timeline.view,
                &self.timeline_layout,
                &self.clips,
                x,
                y,
            )
            && self.timeline_layout.headers.contains(x, y)
        {
            if let Some(doc) = &mut self.options.document {
                doc.toggle_lane_mute(lane);
            }
            self.tree.invalidate(TIMELINE);
            return;
        }
        let edits = self.timeline.press(
            button,
            x,
            y,
            &self.timeline_layout,
            &self.clips,
            self.beats_per_bar(),
        );
        self.drag = Drag::Timeline;
        self.apply_arrange_edits(edits);
        // Clicking a clip opens it in the roll — the two panels are two views
        // of one piece, and having to find the channel in the rack to edit the
        // clip you just pointed at is two panels rather than one workflow.
        if let Some(clip) = self.timeline.take_open()
            && let Some(doc) = &mut self.options.document
        {
            doc.open_clip(clip);
            self.roll.clear_selection();
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
        let (ticks, rows) = edge_scroll(
            &crate::canvas::RollView {
                scroll_tick: self.timeline.view.scroll_tick,
                top_key: 0,
                pixels_per_tick: self.timeline.view.pixels_per_tick,
                key_height: self.timeline.view.lane_height,
                snap: self.timeline.view.snap,
            },
            grid,
            x,
            y,
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
        let dragging_box = self.timeline.marquee().is_some();
        self.apply_arrange_edits(edits);
        if dragging_box {
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
        let mut created = Vec::new();
        if let Some(doc) = &mut self.options.document {
            for edit in edits {
                created.extend(doc.arrange(edit));
            }
        }
        // The copy becomes the selection, so repeating walks along the
        // arrangement instead of stacking clips in one place — see
        // `Timeline::clips_inserted`.
        self.timeline.clips_inserted(created);
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
                // Arming throws the last take away, so a new one does not
                // begin with the end of the one before it still in the ring.
                if decision == TransportAction::SetArmed(true)
                    && let Some(doc) = &mut self.options.document
                {
                    doc.discard_take();
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
        let Some(doc) = &mut self.options.document else {
            return;
        };
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
        // What you touch, you hear — even stopped. Drawing a note, clicking an
        // existing one, and dragging one to a new pitch all sound it, on the
        // live path rather than by starting the transport: that is the
        // difference between hearing what you just wrote and playing the song.
        self.sound_roll_request();
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

    fn press_rack(&mut self, x: f32, y: f32) {
        match rack_hit(&self.rack, x, y) {
            RackHit::Row(index) => {
                if let Some(doc) = &mut self.options.document {
                    doc.select_channel(index);
                }
                self.roll.clear_selection();
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
                self.tab = EditorTab::Instrument;
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
            RackHit::Add => {
                // Nothing to add yet if no preset is chosen; the browser is
                // where that happens, so say so rather than doing nothing.
                self.add_channel_from_browser();
            }
            RackHit::Nothing => {}
        }
        self.tree.invalidate(RACK);
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
        }
        self.refresh_title();
    }

    fn press_browser(&mut self, x: f32, y: f32) {
        match browser_hit(&self.browser, x, y) {
            BrowserHit::Search => {
                self.searching = true;
            }
            BrowserHit::File(index) => {
                // A row that **moves** the browser rather than opening
                // something: the list under the pointer is about to be a
                // different list, so the scroll offset it was read at means
                // nothing in it.
                let moving = matches!(
                    self.files.get(index).map(|entry| entry.kind),
                    Some(crate::document::LibraryKind::Folder | crate::document::LibraryKind::Up)
                );
                // The same rows in both modes, and the same click: a soundfont
                // opens to show its presets, a folder opens to show what is
                // in it, and a project opens as a project.
                let result = match (&mut self.options.document, self.browser_mode) {
                    (Some(doc), BrowserMode::Sounds) => doc.open_file(index),
                    (Some(doc), BrowserMode::Projects) => doc.open_project(index),
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
            BrowserHit::OpenFolder => {
                if let Some(doc) = &mut self.options.document {
                    match self.browser_mode {
                        BrowserMode::Sounds => doc.reveal_library_dir(),
                        BrowserMode::Projects => doc.reveal_projects_dir(),
                    }
                }
            }
            BrowserHit::ChooseFolder => {
                // Ctrl adds a folder instead of replacing the list, the same
                // way Ctrl on a preset adds a channel instead of replacing its
                // instrument. The button says "Change" because replacing is
                // what somebody clicking it means.
                let add = self.modifiers.control_key();
                if let Some(doc) = &mut self.options.document {
                    doc.choose_library_dir(add);
                }
                // The picker held the event loop while it was up, so the meters
                // and the playhead have a gap in them to catch up on.
                self.last_tick = std::time::Instant::now();
            }
            BrowserHit::Mode(mode) => {
                if self.browser_mode != mode {
                    self.browser_mode = mode;
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
            BrowserHit::NewProject => {
                if let Some(doc) = &mut self.options.document
                    && let Err(e) = doc.new_project()
                {
                    self.status = e;
                }
                self.refresh_title();
            }
            BrowserHit::Export => self.export(),
            BrowserHit::Nothing => {}
        }
        self.tree.invalidate(BROWSER);
        self.tree.invalidate(RACK);
    }

    /// The rack's add button: the selected preset onto a new channel.
    fn add_channel_from_browser(&mut self) {
        let preset = self
            .browser
            .preset_rows
            .first()
            .map(|(index, _)| *index)
            .unwrap_or(0);
        if self.presets.is_empty() {
            self.status =
                "pick a soundfont below first — then its preset goes on a new channel".to_string();
            return;
        }
        if let Some(doc) = &mut self.options.document
            && let Err(e) = doc.add_channel_with(preset)
        {
            self.status = e;
        }
    }

    /// A toolbar button.
    fn activate(&mut self, control: RollControl) {
        match control {
            RollControl::Tool(tool) => self.set_tool(tool),
            RollControl::Snap => self.cycle_snap(),
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
            RollControl::Ghost => self.cycle_ghosts(),
            RollControl::Slide => self.toggle_slide(),
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
            self.start_audition(key, 0);
        }
    }

    /// Zoom about the middle of the grid — what a button press means, as
    /// against the wheel, which zooms about the pointer.
    fn zoom(&mut self, x: f32, y: f32) {
        let grid = self.roll_layout.grid;
        if x != 1.0 {
            zoom_x(&mut self.roll.view, grid, grid.x + grid.width / 2.0, x);
        }
        if y != 1.0 {
            zoom_y(&mut self.roll.view, grid, grid.y + grid.height / 2.0, y);
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
        let (ticks, rows) = edge_scroll(&self.roll.view, grid, x, y);
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
        let dragging_box = self.roll.marquee().is_some();
        self.apply_roll_edits(edits);
        self.sound_roll_request();
        if dragging_box {
            // A marquee changes nothing in the document and everything on
            // screen, so it has to dirty the panel on its own account.
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
            self.start_audition(asked.key, asked.ticks);
        }
    }

    /// The wheel, routed by what it is over.
    ///
    /// Over the roll: vertical scroll moves through the keys, `Shift` scrolls
    /// the song, `Ctrl` zooms time about the pointer and `Ctrl+Shift` (or
    /// `Alt`) zooms pitch. Over a list: it scrolls that list. The FL habits,
    /// and the ones a mouse can express.
    fn scroll_roll(&mut self, dx: f32, dy: f32) {
        let (x, y) = self.cursor;

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
            self.rack_scroll =
                scrolled(self.rack_scroll, -(dy.round() as i32), self.channels.len());
            self.relayout_panels();
            self.tree.invalidate(RACK);
            return;
        }
        if self.layout.browser.frame.contains(x, y) {
            let over_presets = self.browser.presets.contains(x, y);
            if over_presets {
                self.preset_scroll =
                    scrolled(self.preset_scroll, -(dy.round() as i32), self.presets.len());
            } else {
                self.file_scroll =
                    scrolled(self.file_scroll, -(dy.round() as i32), self.files.len());
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
    fn key(&mut self, event: &winit::event::KeyEvent) {
        use winit::keyboard::{Key, NamedKey};

        let ctrl = self.modifiers.control_key();
        let shift = self.modifiers.shift_key();

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

        match &event.logical_key {
            // Play from the marker; press it again and the playhead comes back
            // to the marker. See `crate::transport::TransportAction`.
            Key::Named(NamedKey::Space) => self.transport(TransportHit::Play),
            // And the front of the song, which is what the stop square does.
            Key::Named(NamedKey::Home) => self.transport(TransportHit::Stop),
            Key::Named(NamedKey::Delete) | Key::Named(NamedKey::Backspace) => {
                // Whichever canvas was last pressed owns the key: "delete the
                // selection" has two meanings and no reading that means both.
                if self.focus == Focus::Timeline {
                    let edits = self.timeline.delete_selection();
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
                    "z" if ctrl && !shift => self.undo(),
                    "z" if ctrl && shift => self.redo(),
                    "y" if ctrl => self.redo(),
                    "a" if ctrl => {
                        if let Some(doc) = &self.options.document {
                            self.roll.select_all(doc.notes());
                            self.tree.invalidate(PANEL);
                        }
                    }
                    "s" if ctrl => self.save(),
                    // Beside Ctrl+S, because bouncing is the other thing you
                    // do to a whole project. The button is on the Projects
                    // tab; this is so you do not have to go there.
                    "e" if ctrl => self.export(),
                    "c" if ctrl => self.copy(),
                    "x" if ctrl => self.cut(),
                    "v" if ctrl => self.paste(),
                    "b" if ctrl => self.duplicate(),
                    "d" if ctrl => self.duplicate(),
                    // Mute what is selected on the arrangement.
                    "m" if ctrl => {
                        let edits = self.timeline.toggle_mute(&self.clips);
                        self.apply_arrange_edits(edits);
                        if let Some(doc) = &mut self.options.document {
                            doc.end_gesture();
                        }
                    }
                    // Show and hide the arrangement strip.
                    "t" if ctrl => self.toggle_timeline(),
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
                    // The cut tool. `C` is FL's, and `Ctrl+C` is copy — the
                    // modifier is what keeps them apart, as it does for `B`
                    // (paint) and `Ctrl+B` (duplicate).
                    "c" => self.set_tool(Tool::Slice),
                    "+" | "=" => self.zoom(1.25, 1.0),
                    "-" | "_" => self.zoom(0.8, 1.0),
                    // Every tool by number as well as by letter: the letters
                    // are FL's and are worth keeping, and a number row is what
                    // somebody who has not learnt them will try.
                    "1" => self.pick_tool(Tool::Draw, crate::canvas::TimelineTool::Draw),
                    "2" => self.set_tool(Tool::Paint),
                    "3" => self.pick_tool(Tool::Select, crate::canvas::TimelineTool::Select),
                    "4" => self.set_tool(Tool::Delete),
                    "5" => self.set_tool(Tool::Slice),
                    "6" => self.set_tool(Tool::Mute),
                    "7" => self.set_tool(Tool::Slip),
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
            C::Draw | C::Select => {
                self.timeline.set_tool(if control == C::Draw {
                    crate::canvas::TimelineTool::Draw
                } else {
                    crate::canvas::TimelineTool::Select
                });
                self.tree.invalidate(TIMELINE);
            }
            C::Snap => {
                self.timeline.cycle_snap();
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
                    grid.x + grid.width / 2.0,
                    factor,
                );
                self.tree.invalidate(TIMELINE);
            }
        }
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

        if self.tab == EditorTab::Instrument {
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

    fn cycle_snap(&mut self) {
        self.roll.view.snap = self.roll.view.snap.next();
        self.tree.invalidate(PANEL);
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

        // A note waiting out its minimum length has to be woken for, or a
        // window that goes back to sleep the instant the mouse comes up leaves
        // it sounding until something else happens to it.
        if let Some(due) = self.audition.due() {
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

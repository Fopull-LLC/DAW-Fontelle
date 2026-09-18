//! The `wgpu` device and the scene that gets drawn onto it (TDD §16.2).
//!
//! Two things live here, and the split is deliberate:
//!
//! - [`draw_window`] is a **pure function of the theme, the layout and the
//!   text**: it puts shapes into a `vello::Scene` and touches no GPU state. So
//!   it can be rendered and inspected without a window, which is what
//!   `tests/render_headless.rs` does.
//! - [`GpuContext`] and [`Headless`] are the two places a scene turns into
//!   pixels — onto a window's surface, and into memory.
//!
//! `vello` is the vector path (§16.2). If it proves troublesome the documented
//! fallback is direct `wgpu` pipelines with `lyon`; keeping the scene building
//! separate from the device is also what would make that swap a rewrite of one
//! function rather than of the crate.

mod bridge;

pub use bridge::SkyFrame;

use vello::kurbo::{Affine, BezPath, Point, Rect as KRect, RoundedRect, RoundedRectRadii, Stroke};
use vello::peniko::{BlendMode, Fill};
use vello::util::RenderContext;
use vello::wgpu;
use vello::{AaConfig, RenderParams, Renderer, RendererOptions, Scene};

use fontelle_model::{Arena, Note};
use fontelle_types::{NoteId, PPQN, Tick};

use crate::canvas::{
    BrowserHit, BrowserLayout, InstrumentLayout, InstrumentView, LaneProperty, ParamKind, RackHit,
    RackLayout, RollControl, RollLayout, RollView, SnapDivision, TimelineControl, TimelineLayout,
    TimelineToolbar, TimelineView, Tool, ToolbarLayout, choice_index, clip_rect, lane_baseline_y,
    lane_to_y, lane_y_of_value, tick_to_x, timeline_tick_to_x, timeline_visible_ticks,
    visible_keys, visible_lanes, visible_ticks,
};
use crate::document::{
    ChannelInfo, ClipInfo, GhostFilter, GhostNote, LaneInfo, LibraryEntry, LibraryKind,
};
use crate::layout::{EditorTab, EditorTabs, PanelLayout, Rect, WindowLayout};
use crate::text::{Labels, TextLayout};
use crate::theme::{Color, Theme};
use crate::transport::{
    Meter, TransportBarLayout, TransportHit, TransportView, meter_fill, playhead_x,
};

/// Everything the window draws that had to be shaped or measured first.
///
/// Text shaping needs a mutable `FontSystem`, and [`draw_window`] is a pure
/// function of its inputs — so whatever needs shaping is shaped by the caller
/// and handed over already positioned. That split is what keeps the whole
/// picture testable off a GPU and off a window.
pub struct Chrome<'a> {
    pub panel_title: &'a TextLayout,
    pub transport: TransportChrome<'a>,
    /// The editor column's contents. `None` draws an empty panel — which is
    /// what a window with no clip open shows.
    pub roll: Option<RollChrome<'a>>,
    /// The mixer, likewise.
    pub mixer: Option<MixerChrome<'a>>,
    /// The editor column's two tabs, and which of them is on. The instrument,
    /// the effect and the automation curve are windows of their own — see
    /// [`draw_editor_window`].
    pub tabs: EditorTabs,
    pub tab: EditorTab,
    /// Which tab the pointer is over.
    pub hover_tab: Option<EditorTab>,
    /// The channel rack down the left. `None` when there is no studio behind
    /// the window at all, and `None` when the panel is showing its other tab.
    pub rack: Option<RackChrome<'a>>,
    /// The prefab list, when that is the tab showing (TDD §10.5). Exactly one
    /// of this and [`rack`](Self::rack) is `Some` while there is a studio —
    /// they are one panel.
    pub prefabs: Option<PrefabChrome<'a>>,
    pub browser: Option<BrowserChrome<'a>>,
    /// The arrangement above the editor. `None` when it is hidden.
    pub timeline: Option<TimelineChrome<'a>>,
    /// The browser panel's heading, carrying the count of what is in the bank —
    /// so "how many soundfonts have I got" needs no line of its own.
    pub browser_title: &'a str,
    /// Everything that had to be shaped: bar numbers, key names, channel and
    /// soundfont names, the toolbar's captions. Looked up by the string being
    /// drawn, because that is the only key both sides can agree on without the
    /// pure half of the renderer owning a font system.
    pub labels: &'a Labels,
    /// One line along the bottom of the browser: what went wrong, or where the
    /// soundfonts are meant to go.
    pub status: &'a str,
    /// A transient banner — its text and whether it offers an Undo — drawn over
    /// everything but the modal. See [`crate::canvas::toast_layout`].
    pub toast: Option<(&'a str, bool)>,
    /// A confirm modal's question, while one is up. Drawn last of all, over a
    /// scrim. See [`crate::canvas::confirm_layout`].
    pub confirm: Option<&'a str>,
    /// The hover tip, once the pointer has sat still long enough — what it
    /// says and where it goes. `None` for the great majority of frames.
    pub tooltip: Option<(&'a str, Rect)>,
    /// The right-click menu, while one is open. Drawn **last**, over
    /// everything, because that is what a menu is.
    pub menu: Option<&'a crate::canvas::ContextMenu>,
    /// The field of whatever is being typed into right now — a name prompt,
    /// the browser's search, an inline rename. `None` when nothing is.
    pub field: Option<TextFieldChrome>,
    /// A row carried out of the browser, while one is in the air. Drawn
    /// **after** the menu and the tip, because it is in hand: nothing can be
    /// on top of the thing the pointer is holding.
    pub carry: Option<CarryChrome<'a>>,
    /// The start menu, while it is up. When it is, it is the whole picture:
    /// the studio behind it is not drawn at all (see [`draw_welcome`]).
    pub welcome: Option<WelcomeChrome<'a>>,
    /// The keyboard shortcuts sheet, while it is up. Over everything, the
    /// start menu included — it is opened from both. See
    /// [`crate::canvas::keybinds_layout`].
    pub keybinds: Option<KeybindsChrome<'a>>,
}

/// The shortcuts sheet (`canvas::keybinds`), as the window draws it.
pub struct KeybindsChrome<'a> {
    /// How far the list is scrolled.
    pub scroll: f32,
    /// What every rebindable row's chip says.
    pub keymap: &'a crate::canvas::Keymap,
    /// The row waiting for a new shortcut, if one is.
    pub listening: Option<crate::canvas::Action>,
    /// The rebindable row under the pointer, drawn lit so it reads as a
    /// thing a press does something to.
    pub hover: Option<crate::canvas::Action>,
    /// A line under the title in place of the usual hint — what the last
    /// rebind took from whom. Empty for none.
    pub note: &'a str,
}

/// Everything the start menu draws (`canvas::welcome`).
pub struct WelcomeChrome<'a> {
    pub layout: crate::canvas::WelcomeLayout,
    /// The name, shaped at twice the chrome's size — the one string in the
    /// window not drawn from [`Labels`], because the cache has one size.
    pub title: &'a TextLayout,
    /// "Version 0.1.0".
    pub version: &'a str,
    /// What the update check has to say — [`crate::canvas::update_line`],
    /// shaped to the column's width so a sentence wraps rather than runs.
    pub update: &'a TextLayout,
    /// The offer, when there is one.
    pub update_button: Option<&'a str>,
    /// The bar in the offer's slot while an archive comes down:
    /// `Some(Some(fraction))`, or `Some(None)` for a size the server did
    /// not say. See `canvas::update_progress`.
    pub progress: Option<Option<f32>>,
    pub recent: &'a [crate::document::RecentProject],
    pub hover: Option<crate::canvas::WelcomeHit>,
    /// What went wrong with the last press, if anything did. Shaped to the
    /// column like the update line.
    pub message: &'a TextLayout,
}

/// A row from the browser in mid-air, and what letting go would do with it.
///
/// > *"i cant see any visuals of the thing being dragged ... please also ensure
/// > that it shows a visual of where its about to go so you know youre actually
/// > placing it right / that is a legal action before you do it."*
///
/// Two pictures from one value, which is what keeps them honest: the
/// [`target`](Self::target) both lights up what would change and decides
/// whether the chip is drawn as a drop or as a refusal, so a mark cannot
/// promise something the release will not do. See [`crate::canvas::carry_target`].
pub struct CarryChrome<'a> {
    /// The name of what is being carried, as it is written on the chip.
    pub label: &'a str,
    /// What letting go here would do, in words
    /// ([`crate::canvas::carry_note`]). Empty draws as one line.
    pub note: &'a str,
    /// Where the pointer is.
    pub at: (f32, f32),
    pub target: crate::canvas::CarryTarget,
    /// What the chip has to stay inside — the window, or an editor window's
    /// own frame when the pointer is over one of those.
    pub bounds: Rect,
    /// Whether the chip sits above the pointer rather than under it: a file
    /// from the desktop comes with the desktop's own picture hanging below
    /// ([`crate::canvas::carry_chip_lifted`]).
    pub lifted: bool,
}

/// The channel rack's contents.
pub struct RackChrome<'a> {
    pub panel: PanelLayout,
    pub layout: RackLayout,
    pub channels: &'a [ChannelInfo],
    pub selected: usize,
    /// What the pointer is over. A row that lights up under the pointer is how
    /// a list says it is a list of things you can click.
    pub hover: Option<RackHit>,
    /// Every mixer strip's name, master last — what the route menu's rows say.
    pub route_names: &'a [String],
    /// How many strips there are, master included. The route chip's numbering
    /// needs it to tell "the master" from "the last track somebody made".
    pub strips: usize,
    /// The route menu, while it is open, and whose row opened it. Drawn last,
    /// over everything in the rack.
    pub route_menu: Option<&'a crate::canvas::RouteMenu>,
    pub route_menu_open: Option<usize>,
    /// Which row is having its name typed into, so a caret is drawn on it.
    ///
    /// A rename writes straight into the document and the row updates live,
    /// which is the right mechanism and gives no sign that the keyboard has
    /// been captured. The search box has had a caret since it was written and
    /// this is the same claim for a row's name.
    pub renaming: Option<usize>,
    /// The field's caret and selection, while a row is being renamed.
    pub rename: Option<RenameMarks>,
}

/// Where the caret and the selection of an inline rename are, in points
/// from the name's left — measured by the window, since `draw_window`
/// cannot shape. One value serves whichever panel is renaming, because only
/// one thing is ever being renamed.
///
/// > *"in the mixer track when typing its not showing selection highlights
/// > like when i do ctrl a for example"*
///
/// A rename used to draw one caret at the end of the name, whatever the
/// field's caret and selection actually were; this is the field's own truth,
/// drawn the way the name prompt draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenameMarks {
    pub caret_x: f32,
    pub selection: Option<(f32, f32)>,
    pub caret_on: bool,
}

/// The prefab list's contents (TDD §10.5).
pub struct PrefabChrome<'a> {
    pub panel: PanelLayout,
    pub layout: crate::canvas::PrefabLayout,
    pub prefabs: &'a [crate::document::PrefabInfo],
    /// What the pointer is over, so a row lights up.
    pub hover: Option<crate::canvas::PrefabHit>,
    /// Which row is having its name typed into, so a caret is drawn on it.
    pub renaming: Option<usize>,
    pub rename: Option<RenameMarks>,
}

/// The soundfont browser's contents (TDD §17.5).
pub struct BrowserChrome<'a> {
    pub panel: PanelLayout,
    pub layout: BrowserLayout,
    pub query: &'a str,
    pub files: &'a [LibraryEntry],
    pub presets: &'a [LibraryEntry],
    pub selected_file: Option<usize>,
    /// Which preset the selected channel is playing, so the browser can say
    /// which instrument is on it.
    pub selected_preset: Option<usize>,
    /// Which preset row has the **keyboard**, against `selected_preset`'s
    /// "which one is on the channel". Two different questions, so two marks:
    /// an outline for where the arrow keys are, a wash for what is playing.
    pub focus_preset: Option<usize>,
    /// Whether the search box has the keyboard, so the caret is drawn.
    pub searching: bool,
    /// What the pointer is over, so a button can light up.
    pub hover: Option<BrowserHit>,
    /// Which of the panel's lists is showing.
    pub mode: crate::canvas::BrowserMode,
    /// Which kind of file the Import tab is showing, so its two buttons can
    /// say which one is on.
    pub import_kind: fontelle_types::FolderKind,
    /// What control each settings row is drawn as, parallel to `files` in
    /// [`BrowserMode::Settings`](crate::canvas::BrowserMode::Settings) — a
    /// slider's groove, a switch's pill, a choice's caret. Empty in every other
    /// mode, so a soundfont row draws none.
    pub settings_controls: &'a [crate::canvas::SettingControl],
    /// Which settings row the arrow keys are on, so it wears the focus outline.
    pub focus_setting: Option<usize>,
}

/// The arrangement's contents. Read-only, like every other canvas here
/// (INVARIANT 2).
pub struct TimelineChrome<'a> {
    pub panel: PanelLayout,
    pub layout: TimelineLayout,
    pub view: TimelineView,
    pub lanes: &'a [LaneInfo],
    pub clips: &'a [ClipInfo],
    pub selection: &'a [fontelle_types::ClipId],
    /// Where the playhead is in the *song*, and where the time marker is.
    pub playhead_tick: Tick,
    pub marker_tick: Tick,
    pub beats_per_bar: u32,
    pub marquee: Option<Rect>,
    /// Whether the arrangement is the panel the keyboard is talking to, so
    /// Delete and Ctrl+D visibly belong to one canvas rather than to both.
    pub focused: bool,
    /// The controls across the top.
    pub toolbar: TimelineToolbar,
    /// Which tool is on, so its chip is lit.
    pub tool: crate::canvas::TimelineTool,
    /// Whether the Stretch switch is on, so its chip is lit — see
    /// `TimelineControl::Stretch`.
    pub stretch: bool,
    /// Which one the pointer is over, so it lights before it is pressed.
    pub hover: Option<TimelineControl>,
    /// The block under the pointer and which part of it, so an audio block
    /// shows its fade handles before it is chosen and the handle under the
    /// pointer lights — FL shows its fade handles on the clip under the
    /// pointer, and a corner that looks like the rest of the caption is one
    /// nobody finds.
    pub hover_clip: Option<(fontelle_types::ClipId, crate::canvas::ClipPart)>,
    /// The fade being dragged right now, so the block can say how long it
    /// is while it is being set.
    pub fading: Option<(fontelle_types::ClipId, crate::canvas::FadeEnd)>,
    /// Whether there is anything on the clip clipboard, so Paste can say
    /// whether pressing it would do anything.
    pub can_paste: bool,
    /// The cut tool's stroke, while it is being drawn. The same thing the
    /// roll's has, and for the same reason: a tool whose gesture leaves no
    /// mark is one you have to aim blind.
    pub slice: Option<((f32, f32), (f32, f32))>,
    /// Which lane header is having its name typed into. See
    /// [`RackChrome::renaming`].
    pub renaming: Option<usize>,
    pub rename: Option<RenameMarks>,
    /// The selected points of an automation block, and whose they are, so
    /// they are drawn lit. See `Timeline::point_selection`.
    pub point_clip: Option<fontelle_types::ClipId>,
    pub point_selection: &'a [fontelle_types::PointId],
    /// The time selection, in song ticks — the loop, or the one being
    /// dragged out on the ruler right now. Drawn on the ruler and as a band
    /// down the grid, so what will loop is visible where it is edited.
    pub loop_range: Option<(Tick, Tick)>,
    /// The take being recorded, in song ticks, while one is being recorded.
    ///
    /// > *"i cannot see the clip being made as im recording please add that so
    /// > i can see the clip being recorded in the arrangement as im
    /// > recording."*
    ///
    /// Not a clip: it is not in the document and will not be until the
    /// transport stops, so it is drawn as a band rather than as a block, at
    /// the bottom of the grid where `AddAudioClip` will put the row it makes.
    /// Drawing it as an ordinary clip would be a picture of a document that
    /// does not exist yet.
    pub recording: Option<(Tick, Tick)>,
    /// The notes of a take being recorded into the **open** clip, in that
    /// clip's ticks, drawn into its block as it is played. The note-take
    /// counterpart of [`recording`](Self::recording): not in the document
    /// until the transport stops, and drawn in the record colour so they
    /// cannot be mistaken for notes that are.
    pub take_notes: &'a [crate::document::NotePreview],
}

/// Everything the piano roll draws from. All of it is read-only: the roll is a
/// view and never a mutator (INVARIANT 2).
pub struct RollChrome<'a> {
    /// Whether the roll is the canvas the keyboard is talking to.
    ///
    /// The arrangement has said so since it was written; the roll had no way
    /// to, so half the answer was missing and *"you don't accidentally do
    /// something in the wrong window"* only worked in one direction.
    pub focused: bool,
    pub layout: RollLayout,
    pub toolbar: ToolbarLayout,
    pub view: RollView,
    pub notes: &'a Arena<NoteId, Note>,
    pub selection: &'a [NoteId],
    /// Where the playhead is *within the clip*, or `None` when the transport
    /// is somewhere this clip does not cover.
    pub playhead_tick: Option<Tick>,
    pub beats_per_bar: u32,
    /// What the mouse is doing, so the toolbar can light the tool that is on.
    pub tool: Tool,
    pub snap: SnapDivision,
    /// Which note property the lane under the grid is showing.
    pub lane_property: LaneProperty,
    /// Which keys the instrument on the channel can actually play, and what
    /// each one is called. `KeyMap::unknown` greys nothing — see
    /// [`crate::document::KeyMap`].
    pub key_map: &'a crate::document::KeyMap,
    /// Other instruments' notes, drawn behind these ones.
    pub ghosts: &'a [GhostNote],
    /// Which filter the chip is on, so it can say so and light up.
    pub ghost_filter: GhostFilter,
    /// The notes of the take being recorded, so far, drawn over the clip's
    /// own in the record colour — see [`TimelineChrome::take_notes`] and
    /// `DocumentHost::recording_notes`.
    pub recording: &'a [crate::document::NotePreview],
    /// The time selection, in **this clip's** ticks, or `None`. Drawn on the
    /// ruler and as a band down the grid, like the arrangement's.
    pub loop_range: Option<(Tick, Tick)>,
    /// Where the **time marker** is within this clip, or `None` when it is
    /// somewhere the clip does not cover. Play starts here.
    pub marker_tick: Option<Tick>,
    /// The selection box being dragged, if one is.
    pub marquee: Option<Rect>,
    pub hover: Option<RollControl>,
    /// The lane chip's menu, while it is open. Drawn last, over everything.
    pub lane_menu: Option<&'a crate::canvas::LaneMenu>,
    /// The Tools panel, while it is open. Drawn last, like the menu.
    pub tools_panel: Option<&'a crate::canvas::ToolsDialog>,
    /// What the Tools panel is set to, which is where its rows read their
    /// names and values from.
    pub tools: &'a crate::canvas::Tools,
    /// How long the clip being edited is, in its own ticks, or `None` for a
    /// host with no clip. The grid past it is shaded: a note written there
    /// does not sound, because a clip's length is the window on its content
    /// (TDD §11.4), and a silent note with nothing to show for it is a bug
    /// report waiting to happen.
    pub clip_length: Option<Tick>,
    /// The cut tool's line while it is being drawn, in screen points.
    pub slice: Option<((f32, f32), (f32, f32))>,
    /// Whether the strip down the side is a keyboard or a list of names.
    pub key_style: crate::canvas::KeyStyle,
    /// Which keys a MIDI keyboard is holding down right now, one bit each —
    /// lit on the keyboard down the side so a phrase can be played and then
    /// written in. Zero when nothing is plugged in, which draws the keyboard
    /// exactly as it always was. See `fontelle_midi::LiveKeys`.
    pub live_keys: u128,
}

/// The instrument editor's contents (TDD §7.2).
pub struct InstrumentChrome<'a> {
    pub layout: InstrumentLayout,
    pub view: &'a InstrumentView,
    /// Which control the pointer is over, so a knob lights up before it is
    /// grabbed.
    pub hover: Option<(usize, usize)>,
    /// Which control is being dragged.
    pub active: Option<(usize, usize)>,
    /// Which preset chip the pointer is over, so it lights before it is
    /// clicked. Its own field rather than a value of `hover`, because a chip
    /// is not a control — see `instrument_preset_hit`.
    pub hover_preset: Option<usize>,
    /// The same, for the key row.
    pub hover_key: Option<usize>,
}

/// The mixer panel's contents (TDD §13).
pub struct MixerChrome<'a> {
    pub layout: crate::canvas::MixerLayout,
    pub strips: &'a [crate::document::MixerStrip],
    /// The peak each strip has hit since the last frame, per channel, in the
    /// same order as `strips`. Shorter is read as silence, so a frame taken
    /// while the graph is being replaced draws quiet meters rather than
    /// panicking.
    pub peaks: &'a [[f32; 2]],
    /// What the pointer is over, so a control lights up before it is grabbed.
    pub hover: Option<crate::canvas::MixerHit>,
    /// And what is being dragged, which is also what makes the read-out under
    /// a strip show its **pan** while the pan is the thing moving.
    pub active: Option<crate::canvas::MixerHit>,
    /// Which strip the options column is about, drawn with a ring so the
    /// column and the strip it describes are visibly one thing.
    pub selected: usize,
    /// The strip whose name is being typed over, if one is — drawn with a
    /// caret, the same as the rack's rows and the arrangement's lanes.
    pub renaming: Option<usize>,
    pub rename: Option<RenameMarks>,
    /// What the options column's output row says — worked out where the route
    /// names are, rather than in the drawing code.
    pub output_label: String,
    /// And what its **input** row says (TDD §15.4): the device this track
    /// records from, or that it records nothing.
    pub input_label: String,
    /// An insert being dragged up or down the chain: the slot it started in
    /// and the slot it is over.
    pub insert_drag: Option<(usize, usize)>,
    /// The output row's menu, while it is open, and the names its rows read.
    pub output_menu: Option<&'a crate::canvas::RouteMenu>,
    /// The send menu, likewise — a separate field because the two are over the
    /// same list and mean different things, and only one is ever open.
    pub send_menu: Option<&'a crate::canvas::RouteMenu>,
    pub route_names: &'a [String],
    /// Where the selected track's output goes, as an index into
    /// `route_names` — so the menu can say where you are as well as where you
    /// could go.
    pub output: Option<usize>,
}

/// One EQ, as the editor draws it.
pub struct EffectChrome {
    pub layout: crate::canvas::EqLayout,
    pub config: fontelle_types::EqConfig,
    /// The analyser behind the curve, as an outline — see
    /// [`crate::canvas::spectrum_points`]. Empty draws nothing at all, which
    /// is what a stopped transport gets.
    pub spectrum: Vec<(f32, f32)>,
    /// The curve, already turned into points — computed once with the rest of
    /// the frame's arithmetic rather than inside the drawing code.
    pub curve: Vec<(f32, f32)>,
    /// The selected band's own response, drawn faintly behind the sum so the
    /// band in hand can be seen against the shape it is part of.
    pub band_curve: Vec<(f32, f32)>,
    /// Which band the pointer is over, and which one it has hold of.
    pub hover: Option<usize>,
    pub active: Option<usize>,
    /// Which control under the curve the pointer is over.
    pub hover_field: Option<crate::canvas::EqField>,
    /// What the panel says this insert is: the strip's name and the effect's.
    pub title: String,
    pub bypassed: bool,
}

pub struct TransportChrome<'a> {
    pub layout: TransportBarLayout,
    pub view: TransportView,
    pub meters: [Meter; 2],
    /// The position read-out, already shaped.
    pub readout: &'a TextLayout,
    /// The tempo box's number, already shaped. Its own field rather than a
    /// `Labels` lookup for the same reason `readout` is: it changes every time
    /// it is dragged, and caching a string that never repeats is a leak.
    pub tempo: &'a TextLayout,
    /// The time signature, likewise.
    pub signature: &'a TextLayout,
    /// "Song" or "Clip" — see [`PlayMode`](crate::document::PlayMode).
    pub mode: &'a TextLayout,
    /// What the pointer is over, so the control under it can light up.
    pub hover: Option<TransportHit>,
    /// The **time marker**: where play starts and where a stop comes back to.
    pub marker_sample: i64,
    /// Whether the transport is in clip mode, so the chip can be lit.
    pub clip_mode: bool,
    /// The tempo box while it is being **typed into**: the field replaces
    /// the number. `None` — nearly always — draws the number.
    pub tempo_field: Option<TextFieldChrome>,
}

/// Builds the whole window picture.
///
/// Everything the window looks like is decided here, from data. That is what
/// makes "the theme is ignored" and "the panel is in the wrong place" testable
/// failures rather than things somebody has to notice.
pub fn draw_window(scene: &mut Scene, theme: &Theme, layout: &WindowLayout, chrome: &Chrome<'_>) {
    scene.reset();

    let p = &theme.palette;
    let m = &theme.metrics;

    // The ground. Also painted by `RenderParams::base_color`, but painting it
    // here too is what keeps this function the whole picture — a partial
    // redraw clips to a dirty region and never gets a fresh base.
    fill_rect(scene, layout.window, p.window);

    // The start menu is the whole picture while it is up. Not an overlay on
    // a dimmed studio: a launch shows a menu, and the studio appears when
    // something on it is chosen.
    if let Some(welcome) = &chrome.welcome {
        draw_welcome(scene, theme, chrome.labels, welcome);
        // The one thing above it: the name prompt *New project* opens.
        draw_context_menu(
            scene,
            theme,
            chrome.labels,
            chrome.menu,
            chrome.field.as_ref(),
        );
        if let Some(keybinds) = &chrome.keybinds {
            draw_keybinds(scene, theme, chrome.labels, layout.window, keybinds);
        }
        return;
    }

    draw_transport_bar(scene, theme, chrome.labels, &chrome.transport);

    if let Some(rack) = &chrome.rack {
        draw_panel_frame(scene, theme, &rack.panel);
        draw_label(
            scene,
            chrome.labels,
            "Channels",
            rack.panel.header,
            m,
            p.text,
        );
        draw_rack(scene, theme, chrome.labels, rack);
    }
    if let Some(prefabs) = &chrome.prefabs {
        draw_panel_frame(scene, theme, &prefabs.panel);
        draw_label(
            scene,
            chrome.labels,
            "Prefabs",
            prefabs.panel.header,
            m,
            p.text,
        );
        draw_prefabs(scene, theme, chrome.labels, prefabs);
    }
    if let Some(browser) = &chrome.browser {
        draw_panel_frame(scene, theme, &browser.panel);
        draw_label(
            scene,
            chrome.labels,
            chrome.browser_title,
            browser.panel.header,
            m,
            p.text,
        );
        draw_browser(scene, theme, chrome.labels, browser, chrome.status);
    }

    if let Some(timeline) = &chrome.timeline {
        draw_panel_frame(scene, theme, &timeline.panel);
        draw_label(
            scene,
            chrome.labels,
            ARRANGEMENT,
            timeline.panel.header,
            m,
            p.text,
        );
        draw_timeline(scene, theme, chrome.labels, timeline);
    }
    // The seam between the arrangement and the editor, drawn as a grip so it
    // is visibly a thing you can drag.
    for seam in [layout.divider, layout.sidebar_split] {
        if seam.is_empty() {
            continue;
        }
        let width = (seam.width * 0.06).clamp(20.0, 72.0).min(seam.width);
        fill_rect_rounded(
            scene,
            Rect::new(
                seam.x + (seam.width - width) / 2.0,
                seam.y + (seam.height - 2.0).max(0.0) / 2.0,
                width,
                2.0,
            ),
            1.0,
            p.border,
        );
    }
    // The sidebar's own seam runs the other way, so its grip does too.
    if !layout.sidebar_seam.is_empty() {
        let seam = layout.sidebar_seam;
        let height = (seam.height * 0.06).clamp(20.0, 72.0).min(seam.height);
        fill_rect_rounded(
            scene,
            Rect::new(
                seam.x + (seam.width - 2.0).max(0.0) / 2.0,
                seam.y + (seam.height - height) / 2.0,
                2.0,
                height,
            ),
            1.0,
            p.border,
        );
    }

    if layout.panel.frame.is_empty() {
        return;
    }

    let frame = rounded(layout.panel.frame, m.corner_radius);
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        p.panel.to_peniko(),
        None,
        &frame,
    );

    // The header rounds at the top only. Drawing it as a plain rectangle would
    // square off the panel's top corners; drawing it fully rounded would round
    // its bottom two into the body it sits against.
    if !layout.panel.header.is_empty() {
        let h = layout.panel.header;
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            p.panel_header.to_peniko(),
            None,
            &RoundedRect::from_rect(
                KRect::new(h.x as f64, h.y as f64, h.right() as f64, h.bottom() as f64),
                RoundedRectRadii::new(m.corner_radius as f64, m.corner_radius as f64, 0.0, 0.0),
            ),
        );
    }

    if m.border_width > 0.0 {
        scene.stroke(
            &Stroke::new(m.border_width as f64),
            Affine::IDENTITY,
            p.border.to_peniko(),
            None,
            &frame,
        );
    }

    match chrome.tab {
        EditorTab::Roll => {
            if let Some(roll) = &chrome.roll {
                draw_piano_roll(scene, theme, chrome.labels, roll);
            }
        }
        EditorTab::Mixer => {
            if let Some(mixer) = &chrome.mixer {
                draw_mixer(scene, theme, chrome.labels, mixer);
            }
        }
    }
    draw_editor_tabs(scene, theme, chrome);

    draw_text(
        scene,
        chrome.panel_title,
        layout.panel.header.x + m.panel_padding,
        // Vertically centred in the header by its own measured height, so a
        // theme with a bigger font stays centred without a second number to
        // keep in sync.
        layout.panel.header.y + (layout.panel.header.height - chrome.panel_title.height) / 2.0,
        p.text,
    );

    // Absolutely last: a tip is above everything, including an open menu —
    // which is the one place in this window where a control's meaning is least
    // obvious and its explanation most wanted.
    // Over everything else in the window, including the transport bar: a menu
    // drawn under the thing it was opened from is a menu you cannot read.
    draw_context_menu(
        scene,
        theme,
        chrome.labels,
        chrome.menu,
        chrome.field.as_ref(),
    );
    draw_tooltip(scene, theme, chrome);
    // Last of all: what the pointer is holding.
    if let Some(carry) = &chrome.carry {
        draw_carry(scene, theme, chrome.labels, carry);
    }
    // Over even that: the transient banner, and — above everything — the modal.
    if let Some((text, undoable)) = chrome.toast {
        draw_toast(scene, theme, chrome.labels, layout.window, text, undoable);
    }
    if let Some(question) = chrome.confirm {
        draw_confirm(scene, theme, chrome.labels, layout.window, question);
    }
    // And the shortcuts sheet, which is a page rather than a prompt: over
    // the studio and its menus, under nothing.
    if let Some(keybinds) = &chrome.keybinds {
        draw_keybinds(scene, theme, chrome.labels, layout.window, keybinds);
    }
}

/// The keyboard shortcuts sheet (`canvas::keybinds`): a scrim, a card, and
/// the catalogue on it in columns — a heading per section, and under it one
/// line per binding with the key in a chip and what it does beside it.
fn draw_keybinds(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    window: Rect,
    chrome: &KeybindsChrome<'_>,
) {
    use crate::canvas::{
        KEYBIND_SECTIONS, KEYBINDS_CLOSE, KEYBINDS_HINT, KEYBINDS_LISTENING, KEYBINDS_PRESS,
        KEYBINDS_RESET, KEYBINDS_TITLE, KeybindRow,
    };
    let p = &theme.palette;
    let m = &theme.metrics;
    fill_rect(scene, window, p.window.with_alpha(200));
    let l = crate::canvas::keybinds_layout(window, m, chrome.scroll);
    if l.frame.is_empty() {
        return;
    }
    fill_rect_rounded(scene, l.frame, m.corner_radius * 2.0, p.panel);
    stroke_rect_rounded(scene, l.frame, m.corner_radius * 2.0, 1.0, p.border);

    if let Some(text) = labels.get(KEYBINDS_TITLE) {
        draw_text_clipped(
            scene,
            text,
            l.title,
            l.title.x,
            l.title.y + (l.title.height - text.height) / 2.0,
            p.text,
        );
    }
    // The line under the title says what the page is waiting for: the new
    // shortcut while a row listens, what the last one took from whom just
    // after, and how to use the page the rest of the time.
    let (hint, hint_ink) = if chrome.listening.is_some() {
        (KEYBINDS_LISTENING, p.accent)
    } else if !chrome.note.is_empty() {
        (chrome.note, p.text)
    } else {
        (KEYBINDS_HINT, p.text_muted)
    };
    if let Some(text) = labels.get_small(hint) {
        draw_text_clipped(
            scene,
            text,
            l.hint,
            l.hint.x,
            l.hint.y + (l.hint.height - text.height) / 2.0,
            hint_ink,
        );
    }
    // Reset, as a plain button; quiet, because it undoes every change at
    // once and is not the thing most visits are for.
    if !l.reset.is_empty() {
        fill_rect_rounded(scene, l.reset, m.corner_radius, p.panel_header);
        stroke_rect_rounded(scene, l.reset, m.corner_radius, m.border_width, p.border);
        if let Some(text) = labels.get_small(KEYBINDS_RESET) {
            draw_text_clipped(
                scene,
                text,
                l.reset,
                l.reset.x + ((l.reset.width - text.width) / 2.0).max(2.0),
                l.reset.y + (l.reset.height - text.height) / 2.0,
                p.text,
            );
        }
    }
    // The ×, on a plate so it reads as the button it is.
    if !l.close.is_empty() {
        fill_rect_rounded(scene, l.close, m.corner_radius, p.panel_header);
        stroke_rect_rounded(scene, l.close, m.corner_radius, m.border_width, p.border);
        if let Some(text) = labels.get(KEYBINDS_CLOSE) {
            draw_text_clipped(
                scene,
                text,
                l.close,
                l.close.x + (l.close.width - text.width) / 2.0,
                l.close.y + (l.close.height - text.height) / 2.0,
                p.text,
            );
        }
    }

    // The list, clipped to its body: a row half off the top draws its
    // visible half and nothing else, which is what a scrolling list is.
    if l.body.is_empty() {
        return;
    }
    scene.push_layer(
        Fill::NonZero,
        BlendMode::default(),
        1.0,
        Affine::IDENTITY,
        &KRect::new(
            l.body.x as f64,
            l.body.y as f64,
            l.body.right() as f64,
            l.body.bottom() as f64,
        ),
    );
    for row in &l.rows {
        match row {
            KeybindRow::Heading { section, rect } => {
                let Some(section) = KEYBIND_SECTIONS.get(*section) else {
                    continue;
                };
                // A rule under the heading, the way the panels' headers sit
                // over their bodies, so a section is a block and not a run.
                if let Some(text) = labels.get(section.title) {
                    draw_text_clipped(
                        scene,
                        text,
                        *rect,
                        rect.x,
                        rect.y + (rect.height - text.height) / 2.0,
                        p.accent,
                    );
                }
                fill_rect(
                    scene,
                    Rect::new(rect.x, rect.bottom() - 1.0, rect.width, 1.0),
                    p.grid_line,
                );
            }
            KeybindRow::Bind {
                section,
                index,
                action,
                keys,
                does,
            } => {
                let Some(bind) = KEYBIND_SECTIONS
                    .get(*section)
                    .and_then(|section| section.binds.get(*index))
                else {
                    continue;
                };
                let listening = action.is_some() && *action == chrome.listening;
                // The whole line lights under the pointer — the chip and
                // the words are one target (`keybinds_hit`).
                if action.is_some() && *action == chrome.hover && !listening {
                    fill_rect_rounded(
                        scene,
                        keys.union(does),
                        m.corner_radius,
                        p.accent.with_alpha(0x28),
                    );
                }
                // The key in a chip the width of its text, not the column:
                // a chip that is always the column's width is a table cell.
                // A rebindable chip wears the accent on its edge, so the
                // rows a press can change look pressable and the fixed ones
                // do not; the one listening is filled with it and says so.
                let caption = if listening {
                    KEYBINDS_PRESS.to_string()
                } else {
                    bind.keys(chrome.keymap)
                };
                if let Some(text) = labels.get_small(&caption) {
                    let chip = Rect::new(
                        keys.x,
                        keys.y + 2.0,
                        (text.width + 12.0).min(keys.width),
                        (keys.height - 4.0).max(0.0),
                    );
                    let (fill, edge, ink) = if listening {
                        (p.accent, p.accent, p.window)
                    } else if action.is_some() {
                        (p.panel_header, p.accent.with_alpha(0x90), p.text)
                    } else {
                        (p.panel_header, p.border, p.text)
                    };
                    fill_rect_rounded(scene, chip, m.corner_radius, fill);
                    stroke_rect_rounded(scene, chip, m.corner_radius, 1.0, edge);
                    draw_text_clipped(
                        scene,
                        text,
                        chip,
                        chip.x + ((chip.width - text.width) / 2.0).max(2.0),
                        chip.y + (chip.height - text.height) / 2.0,
                        ink,
                    );
                }
                if let Some(text) = labels.get_small(bind.does()) {
                    draw_text_clipped(
                        scene,
                        text,
                        *does,
                        does.x,
                        does.y + (does.height - text.height) / 2.0,
                        p.text,
                    );
                }
            }
        }
    }
    scene.pop_layer();
}

/// The transient banner: a rounded bar with the note, and — when the action can
/// be taken back — an Undo button on the right.
fn draw_toast(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    window: Rect,
    text: &str,
    undoable: bool,
) {
    let p = &theme.palette;
    let m = &theme.metrics;
    let l = crate::canvas::toast_layout(window, m, undoable);
    if l.frame.is_empty() {
        return;
    }
    fill_rect_rounded(scene, l.frame, m.corner_radius, p.panel_header);
    stroke_rect_rounded(scene, l.frame, m.corner_radius, 1.0, p.grid_line);
    let inset = m.panel_padding.min(l.frame.width / 4.0);
    let text_right = l.undo.map_or(l.frame.right() - inset, |u| u.x - inset);
    if let Some(shaped) = labels.get(text) {
        draw_text_clipped(
            scene,
            shaped,
            Rect::new(
                l.frame.x + inset,
                l.frame.y,
                (text_right - l.frame.x - inset).max(0.0),
                l.frame.height,
            ),
            l.frame.x + inset,
            l.frame.y + (l.frame.height - shaped.height) / 2.0,
            p.text,
        );
    }
    if let Some(undo) = l.undo {
        fill_rect_rounded(scene, undo, m.corner_radius, p.accent);
        if let Some(shaped) = labels.get(UNDO) {
            draw_text_clipped(
                scene,
                shaped,
                undo,
                undo.x + (undo.width - shaped.width) / 2.0,
                undo.y + (undo.height - shaped.height) / 2.0,
                p.window,
            );
        }
    }
}

/// The confirm modal: a scrim over the window, then a card with the question
/// and two buttons — Cancel, and a Remove tinted so it reads as the weighty one.
fn draw_confirm(scene: &mut Scene, theme: &Theme, labels: &Labels, window: Rect, question: &str) {
    let p = &theme.palette;
    let m = &theme.metrics;
    // A scrim, so what is under the modal reads as out of reach.
    fill_rect(scene, window, p.window.with_alpha(190));
    let l = crate::canvas::confirm_layout(window, m);
    if l.frame.is_empty() {
        return;
    }
    fill_rect_rounded(scene, l.frame, m.corner_radius, p.panel);
    stroke_rect_rounded(scene, l.frame, m.corner_radius, 1.0, p.grid_line);
    if let Some(shaped) = labels.get(question) {
        draw_text_clipped(
            scene,
            shaped,
            l.question,
            l.question.x,
            l.question.y + (l.question.height - shaped.height) / 2.0,
            p.text,
        );
    }
    let buttons = [
        (l.cancel, CONFIRM_CANCEL, p.panel_header, p.text),
        (l.confirm, CONFIRM_REMOVE, p.accent, p.window),
    ];
    for (rect, word, fill, ink) in buttons {
        if rect.is_empty() {
            continue;
        }
        fill_rect_rounded(scene, rect, m.corner_radius, fill);
        if let Some(shaped) = labels.get(word) {
            draw_text_clipped(
                scene,
                shaped,
                rect,
                rect.x + (rect.width - shaped.width) / 2.0,
                rect.y + (rect.height - shaped.height) / 2.0,
                ink,
            );
        }
    }
}

/// The start menu (`canvas::welcome`).
///
/// One card on the window's ground. The logo is drawn in the theme's text
/// ink (see [`crate::branding`]) so it is a mark on the light theme too; the
/// two ways in are the biggest things on the card; the update offer is
/// smaller, because it is an offer; and the footer says who made it, with
/// the links in the accent so they read as links.
pub fn draw_welcome(scene: &mut Scene, theme: &Theme, labels: &Labels, chrome: &WelcomeChrome<'_>) {
    use crate::canvas::{
        FOOTER_TEXT, NEW_PROJECT_LABEL, NOTHING_RECENT, OPEN_PROJECT_LABEL, RECENT_HEADING,
        REPOSITORY_LABEL, WEBSITE_LABEL, WelcomeHit,
    };
    let p = &theme.palette;
    let m = &theme.metrics;
    let l = &chrome.layout;
    let hot = |hit: WelcomeHit| chrome.hover == Some(hit);

    // The card.
    fill_rect_rounded(scene, l.frame, m.corner_radius * 2.0, p.panel);
    stroke_rect_rounded(
        scene,
        l.frame,
        m.corner_radius * 2.0,
        m.border_width,
        p.border,
    );

    // The mark, scaled into its square.
    let logo = crate::branding::logo(p.text);
    if logo.width > 0 && !l.logo.is_empty() {
        let scale = (l.logo.width / logo.width as f32) as f64;
        scene.draw_image(
            &logo,
            Affine::translate((l.logo.x as f64, l.logo.y as f64)) * Affine::scale(scale),
        );
    }

    // The name and the version beside it.
    draw_text_clipped(
        scene,
        chrome.title,
        l.title,
        l.title.x,
        l.title.y + (l.title.height - chrome.title.height) / 2.0,
        p.text,
    );
    draw_line(scene, labels, chrome.version, l.version, p.text_muted);

    // What the check found, and the offer if there is one.
    draw_text_clipped(
        scene,
        chrome.update,
        l.update,
        l.update.x,
        l.update.y,
        p.text_muted,
    );
    if let (Some(button), Some(rect)) = (chrome.update_button, l.update_button) {
        draw_welcome_button(scene, theme, labels, button, rect, hot(WelcomeHit::Update));
    } else if let (Some(progress), Some(rect)) = (chrome.progress, l.update_button) {
        draw_progress_bar(scene, theme, rect, progress);
    }

    // What went wrong, in the warning ink, just above the way out of it.
    draw_text_clipped(
        scene,
        chrome.message,
        l.message,
        l.message.x,
        l.message.y,
        p.meter_peak,
    );

    // The two ways in.
    draw_welcome_button(
        scene,
        theme,
        labels,
        NEW_PROJECT_LABEL,
        l.new_button,
        hot(WelcomeHit::NewProject),
    );
    draw_welcome_button(
        scene,
        theme,
        labels,
        OPEN_PROJECT_LABEL,
        l.open_button,
        hot(WelcomeHit::OpenProject),
    );

    // The recent list.
    draw_line(scene, labels, RECENT_HEADING, l.recent_heading, p.text);
    if let Some(empty) = l.empty_recent {
        draw_line(scene, labels, NOTHING_RECENT, empty, p.text_muted);
    }
    for (i, row) in l.rows.iter().enumerate() {
        let Some(project) = chrome.recent.get(i) else {
            break;
        };
        if hot(WelcomeHit::Recent(i)) || hot(WelcomeHit::Forget(i)) {
            fill_rect_rounded(scene, row.frame, m.corner_radius, p.panel_header);
        }
        // A project whose bundle has gone is drawn in the muted ink, name
        // and all: still a row, so it can be forgotten on purpose, but not
        // one that promises to open.
        let name_ink = if project.exists { p.text } else { p.text_muted };
        let name_area = Rect::new(
            row.frame.x,
            row.frame.y,
            (row.frame.width - row.forget.width - m.panel_padding).max(0.0),
            row.frame.height / 2.0,
        );
        if let Some(text) = labels.get(&project.name) {
            draw_text_clipped(
                scene,
                text,
                name_area,
                name_area.x + m.panel_padding,
                name_area.y + (name_area.height - text.height) / 2.0 + 2.0,
                name_ink,
            );
        }
        let path_area = Rect::new(
            name_area.x,
            row.frame.y + row.frame.height / 2.0,
            name_area.width,
            row.frame.height / 2.0,
        );
        if let Some(text) = labels.get_small(&project.path.display().to_string()) {
            draw_text_clipped(
                scene,
                text,
                path_area,
                path_area.x + m.panel_padding,
                path_area.y + (path_area.height - text.height) / 2.0 - 2.0,
                p.text_muted,
            );
        }
        // The ×: quiet until the pointer is on it, warning-coloured then,
        // because it forgets something.
        let cross_ink = if hot(WelcomeHit::Forget(i)) {
            p.meter_peak
        } else {
            p.text_muted
        };
        if let Some(text) = labels.get("\u{00d7}") {
            draw_text_clipped(
                scene,
                text,
                row.forget,
                row.forget.x + (row.forget.width - text.width) / 2.0,
                row.forget.y + (row.forget.height - text.height) / 2.0,
                cross_ink,
            );
        }
    }

    // The `?` in the corner: a plate like the buttons, the glyph from the
    // icon set, lit under the pointer.
    if !l.help.is_empty() {
        let hot_help = hot(WelcomeHit::Help);
        fill_rect_rounded(
            scene,
            l.help,
            m.corner_radius,
            if hot_help { p.accent } else { p.panel_header },
        );
        stroke_rect_rounded(scene, l.help, m.corner_radius, m.border_width, p.border);
        draw_icon(
            scene,
            crate::icon::Icon::Help,
            l.help.inset(l.help.height * 0.2),
            if hot_help { p.window } else { p.text },
        );
    }

    // The footer: who made it, and where they are.
    draw_line(scene, labels, FOOTER_TEXT, l.footer, p.text_muted);
    draw_welcome_link(
        scene,
        theme,
        labels,
        WEBSITE_LABEL,
        l.website,
        hot(WelcomeHit::Website),
    );
    draw_welcome_link(
        scene,
        theme,
        labels,
        REPOSITORY_LABEL,
        l.repository,
        hot(WelcomeHit::Repository),
    );
}

/// One shaped string, left-aligned and vertically centred in `area`.
fn draw_line(scene: &mut Scene, labels: &Labels, caption: &str, area: Rect, ink: Color) {
    let Some(text) = labels.get(caption) else {
        return;
    };
    draw_text_clipped(
        scene,
        text,
        area,
        area.x,
        area.y + (area.height - text.height) / 2.0,
        ink,
    );
}

/// A start-menu button: a plate with its label centred, lit in the accent
/// under the pointer.
/// A progress bar: the trough in the panel's header ink, the fill in the
/// accent to `fraction` of it — or, when the size is not known, a third of
/// the trough marching along it, its position taken from the clock so it
/// moves without anybody keeping a counter.
fn draw_progress_bar(scene: &mut Scene, theme: &Theme, rect: Rect, fraction: Option<f32>) {
    let p = &theme.palette;
    let m = &theme.metrics;
    // A bar the height of a thin button, centred in the slot it was given.
    let height = (rect.height * 0.5).max(6.0);
    let trough = Rect::new(
        rect.x,
        rect.y + (rect.height - height) / 2.0,
        rect.width,
        height,
    );
    fill_rect_rounded(scene, trough, m.corner_radius, p.panel_header);
    let fill = match fraction {
        Some(fraction) => Rect::new(
            trough.x,
            trough.y,
            trough.width * fraction.clamp(0.0, 1.0),
            trough.height,
        ),
        None => {
            let phase = (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() % 1500)
                .unwrap_or(0)) as f32
                / 1500.0;
            let width = trough.width / 3.0;
            let x = trough.x + (trough.width + width) * phase - width;
            let left = x.max(trough.x);
            let right = (x + width).min(trough.x + trough.width);
            Rect::new(left, trough.y, (right - left).max(0.0), trough.height)
        }
    };
    if fill.width > 0.0 {
        fill_rect_rounded(scene, fill, m.corner_radius, p.accent);
    }
    stroke_rect_rounded(scene, trough, m.corner_radius, m.border_width, p.border);
}

fn draw_welcome_button(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    caption: &str,
    area: Rect,
    hot: bool,
) {
    let p = &theme.palette;
    let m = &theme.metrics;
    let (plate, ink) = if hot {
        (p.accent, p.window)
    } else {
        (p.panel_header, p.text)
    };
    fill_rect_rounded(scene, area, m.corner_radius, plate);
    stroke_rect_rounded(scene, area, m.corner_radius, m.border_width, p.border);
    let Some(text) = labels.get(caption) else {
        return;
    };
    draw_text_clipped(
        scene,
        text,
        area,
        area.x + (area.width - text.width) / 2.0,
        area.y + (area.height - text.height) / 2.0,
        ink,
    );
}

/// A footer link: accent ink with a rule under it, brighter under the
/// pointer. Right-aligned in its cell, so the two sit flush with the card's
/// edge whatever their lengths.
fn draw_welcome_link(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    caption: &str,
    area: Rect,
    hot: bool,
) {
    let p = &theme.palette;
    let Some(text) = labels.get(caption) else {
        return;
    };
    let ink = if hot { p.text } else { p.accent };
    let x = area.x + (area.width - text.width).max(0.0);
    let y = area.y + (area.height - text.height) / 2.0;
    draw_text_clipped(scene, text, area, x, y, ink);
    fill_rect(
        scene,
        Rect::new(x, y + text.height - 1.0, text.width.min(area.width), 1.0),
        ink,
    );
}

/// The hover tip (see [`crate::tooltip`]).
/// Everything a floating editor window draws (TDD §7.2, §12, §13.4).
///
/// The three panels behind this were tabs of the editor column until §7.5's
/// plugin hosting made that untenable — a VST or CLAP editor is handed a
/// parent window and draws into it, so a thing that can only be a tab is a
/// thing the plugin path cannot be built on. The *drawing* did not change: the
/// same three functions take the same three chromes, against a body rectangle
/// that is now a window's rather than a column's.
// Eight, because a floating editor window is made of eight things: a scene, a
// theme, a layout, the shaped labels, its title, its panel, its preset bar and
// the menu that may be open over it. A struct to carry them would be a struct
// that exists to satisfy a lint.
#[allow(clippy::too_many_arguments)]
pub fn draw_editor_window(
    scene: &mut Scene,
    theme: &Theme,
    layout: &PanelLayout,
    labels: &Labels,
    title: &TextLayout,
    chrome: &EditorWindowChrome<'_>,
    // The preset bar, when this window is showing a device that can have one.
    // Every editor window carries it (§P.7) — every one but the audio clip
    // editor, whose subject is a clip and not a device.
    preset: Option<&PresetBarChrome<'_>>,
    // The right-click menu, when the one that is open belongs to *this*
    // window — a knob's, opened on the instrument editor.
    menu: Option<&crate::canvas::ContextMenu>,
    // And its name prompt's field, when that menu is one.
    field: Option<&TextFieldChrome>,
    // A row from the browser held over *this* window — the instrument
    // window's name field takes one (§the rack's own rule, applied to the
    // window that shows one channel).
    carry: Option<&CarryChrome<'_>>,
) {
    // The bridge is painted in its own palette, dark under both themes —
    // `Theme::for_bridge` says why — and everything drawn in its window
    // with it: the header, the preset bar, a menu, a field, the chip.
    let bridge;
    let theme = match chrome {
        EditorWindowChrome::Flopsynth(_) => {
            bridge = theme.for_bridge();
            &bridge
        }
        _ => theme,
    };
    let m = &theme.metrics;
    let p = &theme.palette;

    fill_rect(scene, layout.frame, p.window);
    fill_rect(scene, layout.header, p.panel_header);
    draw_text(
        scene,
        title,
        layout.header.x + m.panel_padding,
        layout.header.y + (layout.header.height - title.height) / 2.0,
        p.text,
    );

    if let Some(preset) = preset {
        draw_preset_bar(
            scene,
            theme,
            labels,
            &preset.layout,
            preset.view,
            preset.hover,
        );
    }

    match chrome {
        EditorWindowChrome::Instrument(Some(instrument)) => {
            draw_instrument(scene, theme, labels, instrument)
        }
        // A channel with no soundfont on it: say so, rather than leaving an
        // empty window that looks broken.
        EditorWindowChrome::Instrument(None) => {
            draw_label(scene, labels, NO_INSTRUMENT, layout.body, m, p.text_muted)
        }
        EditorWindowChrome::Flopsynth(flopsynth) => draw_flopsynth(scene, theme, labels, flopsynth),
        EditorWindowChrome::Tune(tune) => draw_tune(scene, theme, labels, tune),
        EditorWindowChrome::Effect(effect) => draw_effect(scene, theme, labels, effect),
        EditorWindowChrome::Insert(insert) => draw_instrument(scene, theme, labels, insert),
        EditorWindowChrome::AudioClip(clip) => draw_audio_editor(scene, theme, labels, clip),
    }

    draw_context_menu(scene, theme, labels, menu, field);
    if let Some(carry) = carry {
        draw_carry(scene, theme, labels, carry);
    }
}

/// The audio clip editor (TDD §15.1).
///
/// *"double clicking on an audio clip should open a menu that lets me make
/// changes to that audio."* The rows are this window's own shape — a name, a
/// value, a click that steps it — and across the top is the clip's own
/// waveform, because a fade you cannot see is a fade you are aiming blind.
fn draw_audio_editor(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    chrome: &AudioEditorChrome<'_>,
) {
    let p = &theme.palette;
    let m = &theme.metrics;
    let l = &chrome.layout;

    // The waveform, against a ground of its own so the strip reads as a
    // picture rather than as the top of the list.
    if !l.waveform.is_empty() {
        fill_rect_rounded(scene, l.waveform, m.corner_radius, p.panel_header);
        let area = l.waveform.inset(3.0);
        if !area.is_empty() && !chrome.preview.peaks.is_empty() {
            let middle = area.y + area.height / 2.0;
            let half = area.height / 2.0;
            // A rule down the middle: zero, so a quiet take reads as quiet
            // rather than as an empty box.
            fill_rect(
                scene,
                Rect::new(area.x, middle, area.width, 1.0),
                p.grid_line,
            );
            let peaks = &chrome.preview.peaks;
            let mut x = area.x;
            while x < area.right() {
                let t = ((x - area.x) / area.width).clamp(0.0, 1.0);
                let bucket = ((t * peaks.len() as f32) as usize).min(peaks.len() - 1);
                let (low, high) = peaks[bucket];
                // Through the same bend the player and the block use, so
                // the three pictures of a fade are one picture.
                let mut gain = 1.0;
                if chrome.preview.fade_in > 0.0 {
                    let along = (t / chrome.preview.fade_in).clamp(0.0, 1.0);
                    gain *= fontelle_types::bend(f64::from(along), chrome.preview.fade_in_tension)
                        as f32;
                }
                if chrome.preview.fade_out > 0.0 {
                    let along = ((1.0 - t) / chrome.preview.fade_out).clamp(0.0, 1.0);
                    gain *= fontelle_types::bend(f64::from(along), chrome.preview.fade_out_tension)
                        as f32;
                }
                let top = middle - (high.clamp(-1.0, 1.0) * half * gain).max(0.0);
                let bottom = middle - (low.clamp(-1.0, 1.0) * half * gain).min(0.0);
                fill_rect(
                    scene,
                    Rect::new(x, top.min(middle), 1.0, (bottom - top).max(1.0)),
                    p.accent,
                );
                x += 1.0;
            }
        }
    }

    for (field, rect) in &l.rows {
        if rect.is_empty() {
            continue;
        }
        use crate::canvas::AudioControl;
        let control = crate::canvas::audio_row_control(*field);
        // A knob is a cell of its own — the dial, then its name and value under
        // it — not a name-and-track row, so it is drawn and the rest skipped.
        if control == AudioControl::Knob {
            draw_audio_knob(scene, theme, labels, chrome, *field, *rect);
            continue;
        }
        let heading = control == AudioControl::None;
        if !heading && chrome.hover == Some(*field) {
            fill_rect_rounded(scene, rect.inset(1.0), m.corner_radius, p.panel_header);
        }
        let inset = m.panel_padding.min(rect.width);
        if let Some(text) = labels.get(crate::canvas::audio_row_label(*field)) {
            draw_text_clipped(
                scene,
                text,
                *rect,
                rect.x + inset,
                rect.y + (rect.height - text.height) / 2.0,
                if heading { p.text_muted } else { p.text },
            );
        }
        let value = crate::canvas::audio_row_value_at(
            chrome.clip,
            *field,
            chrome.sample_rate,
            chrome.route_label,
        );
        let track = crate::canvas::audio_row_control_rect(*rect, m);
        // The control itself, under the number. Which one it is is the row's
        // to say (`AudioControl`), so the panel cannot draw a track over
        // something that is clicked or a switch over something that is
        // dragged.
        match control {
            AudioControl::None => {}
            // Handled above, before the row-style label and track are drawn.
            AudioControl::Knob => {}
            AudioControl::Slider => draw_audio_track(scene, theme, chrome, *field, track),
            AudioControl::Switch => {
                let on = crate::canvas::audio_row_is_on(chrome.clip, *field);
                draw_audio_switch(scene, theme, track, on);
            }
            // A field with a chevron on it, which is what a drop-down looks
            // like everywhere: the list is a press away, not eighteen.
            AudioControl::Choice => {
                fill_rect_rounded(scene, track, m.corner_radius, p.panel_header);
                draw_chevron(scene, track, p.text_muted);
            }
        }
        // A switch says which way it is by *being* that way; the word "on"
        // beside a pill that is visibly on is a word nobody reads.
        if value.is_empty() || control == AudioControl::Switch {
            continue;
        }
        // Over the control, right-aligned, so a column of values reads down
        // the panel — and so a track's number is on the track rather than
        // beside it, where it would need a column of its own.
        let Some(text) = labels.get(&value) else {
            continue;
        };
        let right = match control {
            // Clear of the chevron.
            AudioControl::Choice => track.right() - CHEVRON_PX * 2.0,
            _ => track.right() - inset.min(track.width / 4.0),
        };
        draw_text_clipped(
            scene,
            text,
            rect.union(&track),
            right - text.width,
            rect.y + (rect.height - text.height) / 2.0,
            if control == AudioControl::Slider {
                p.text
            } else {
                p.accent
            },
        );
    }
}

/// One knob cell on the audio clip editor: the dial, its name under it, and its
/// value under that — the FL-style control for a compact continuous param.
fn draw_audio_knob(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    chrome: &AudioEditorChrome<'_>,
    field: crate::canvas::AudioField,
    cell: Rect,
) {
    let p = &theme.palette;
    let m = &theme.metrics;
    let hot = chrome.hover == Some(field);
    if hot {
        fill_rect_rounded(scene, cell.inset(1.0), m.corner_radius, p.panel_header);
    }
    let knob = crate::canvas::audio_knob_rect(cell, m);
    if knob.is_empty() {
        return;
    }
    let value = crate::canvas::audio_row_fraction(chrome.clip, field).unwrap_or(0.0);
    draw_knob(scene, theme, knob, value, hot, false);

    // The name and the value, centred under the dial and stacked, each on its
    // own line — "descriptions under it", the way a channel-strip knob reads.
    let mut text_y = knob.bottom();
    let room = (cell.bottom() - text_y).max(0.0);
    if let Some(text) = labels.get(crate::canvas::audio_row_label(field)) {
        let line = text.height.min(room);
        if line > 0.0 {
            draw_text_clipped(
                scene,
                text,
                cell,
                cell.x + (cell.width - text.width) / 2.0,
                text_y,
                p.text_muted,
            );
            text_y += line;
        }
    }
    let shown = crate::canvas::audio_row_value_at(
        chrome.clip,
        field,
        chrome.sample_rate,
        chrome.route_label,
    );
    if let Some(text) = labels.get(&shown)
        && text_y + text.height <= cell.bottom() + 0.5
    {
        draw_text_clipped(
            scene,
            text,
            cell,
            cell.x + (cell.width - text.width) / 2.0,
            text_y,
            p.text,
        );
    }
}

/// How wide a drop-down's chevron is.
const CHEVRON_PX: f32 = 7.0;

/// One slider on the audio clip editor: a groove, and a bar over it that grows
/// from wherever that row's *neutral* is.
///
/// Growing from neutral rather than from the left is what makes a cut and a
/// boost read as opposite things: a fill that always started at the left edge
/// would draw a clip trimmed two decibels and one boosted twenty as the same
/// gesture by different amounts. See `canvas::audio_row_neutral`.
fn draw_audio_track(
    scene: &mut Scene,
    theme: &Theme,
    chrome: &AudioEditorChrome<'_>,
    field: crate::canvas::AudioField,
    track: Rect,
) {
    let p = &theme.palette;
    if track.is_empty() {
        return;
    }
    let radius = (track.height / 2.0).min(theme.metrics.corner_radius);
    fill_rect_rounded(scene, track, radius, p.panel_header);
    let Some(value) = crate::canvas::audio_row_fraction(chrome.clip, field) else {
        return;
    };
    let from = crate::canvas::audio_row_neutral(field).unwrap_or(0.0);
    let (low, high) = if from <= value {
        (from, value)
    } else {
        (value, from)
    };
    let fill = Rect::new(
        track.x + track.width * low,
        track.y,
        (track.width * (high - low)).max(0.0),
        track.height,
    )
    .intersection(&track);
    if !fill.is_empty() {
        fill_rect_rounded(scene, fill, radius, p.accent);
    }
    // The handle, so the value has a thing you can see yourself grabbing —
    // and so a value sitting exactly on its neutral is still visible.
    let handle = Rect::new(
        (track.x + track.width * value - 1.5).clamp(track.x, track.right() - 3.0),
        track.y,
        3.0,
        track.height,
    )
    .intersection(&track);
    fill_rect_rounded(scene, handle, 1.5, p.text);
}

/// One switch: a pill that lights up.
fn draw_audio_switch(scene: &mut Scene, theme: &Theme, track: Rect, on: bool) {
    let p = &theme.palette;
    if track.is_empty() {
        return;
    }
    // A pill at the right-hand end of the control column, not the whole of it:
    // a switch as wide as a slider reads as a slider.
    let width = (track.height * 2.0).min(track.width);
    let pill = Rect::new(track.right() - width, track.y, width, track.height);
    let radius = pill.height / 2.0;
    fill_rect_rounded(
        scene,
        pill,
        radius,
        if on { p.accent } else { p.panel_header },
    );
    let knob = (pill.height - 4.0).max(2.0);
    let x = if on {
        pill.right() - knob - 2.0
    } else {
        pill.x + 2.0
    };
    fill_rect_rounded(
        scene,
        Rect::new(x, pill.y + 2.0, knob, knob),
        knob / 2.0,
        if on { p.window } else { p.text_muted },
    );
}

/// The little downward wedge that says a field drops a list.
fn draw_chevron(scene: &mut Scene, field: Rect, colour: crate::theme::Color) {
    if field.width < CHEVRON_PX * 2.0 {
        return;
    }
    let x = field.right() - CHEVRON_PX - 4.0;
    let middle = field.y + field.height / 2.0;
    // Two short rules meeting in the middle, which is all a wedge is at this
    // size and is cheaper than a path.
    for step in 0..3 {
        let step = step as f32;
        fill_rect(
            scene,
            Rect::new(x + step, middle - 1.0 + step, 1.0, 1.0),
            colour,
        );
        fill_rect(
            scene,
            Rect::new(x + CHEVRON_PX - 1.0 - step, middle - 1.0 + step, 1.0, 1.0),
            colour,
        );
    }
}

/// The captions that never change, so they can be shaped once.
pub const SAVE: &str = "Save";

/// The word on a toast's Undo button.
pub const UNDO: &str = "Undo";
/// The two buttons on the confirm modal.
pub const CONFIRM_CANCEL: &str = "Cancel";
pub const CONFIRM_REMOVE: &str = "Remove";
pub const SAVE_AS: &str = "Save as\u{2026}";

/// The preset bar, across the right-hand end of an editor window's header
/// (`docs/flopsynth-plan.md` §P.7).
///
/// One drawing for every device, which is the whole of §P: what a device
/// contributes is its state, and the bar above it is the same bar.
///
/// A rectangle the layout left empty is skipped — the one rule that keeps what
/// is drawn and what can be pressed from being two lists that disagree.
pub fn draw_preset_bar(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    layout: &crate::canvas::PresetBarLayout,
    view: &crate::canvas::PresetBarView,
    hover: Option<crate::canvas::PresetBarHit>,
) {
    use crate::canvas::PresetBarHit;

    let p = &theme.palette;
    let m = &theme.metrics;
    if layout.frame.is_empty() {
        return;
    }

    // The two arrows: a chevron each, lying on their sides. Drawn rather than
    // typed, because "◀" is a glyph a system font may not have and a missing
    // glyph is a box where a button should be.
    for (rect, forwards) in [(layout.previous, false), (layout.next, true)] {
        if rect.is_empty() {
            continue;
        }
        let lit = hover
            == Some(match forwards {
                true => PresetBarHit::Next,
                false => PresetBarHit::Previous,
            });
        if lit {
            fill_rect_rounded(scene, rect, m.corner_radius, p.panel);
        }
        draw_side_chevron(scene, rect, forwards, p.text);
    }

    // The name: a field, because it drops a list — the same sunken-and-
    // outlined shape the instrument panel's name field has, for the same
    // reason (it is a thing you put something *in*).
    if !layout.name.is_empty() {
        fill_rect_rounded(scene, layout.name, m.corner_radius, p.window);
        stroke_rect_rounded(
            scene,
            layout.name,
            m.corner_radius,
            1.0,
            match hover == Some(PresetBarHit::Name) {
                true => p.accent,
                false => p.border,
            },
        );
        if let Some(text) = labels.get(&crate::canvas::preset_bar_name(view)) {
            draw_text_clipped(
                scene,
                text,
                // Short of the chevron, so a long name is elided by the field
                // rather than running under the wedge.
                Rect::new(
                    layout.name.x,
                    layout.name.y,
                    (layout.name.width - CHEVRON_PX - 8.0).max(0.0),
                    layout.name.height,
                ),
                layout.name.x + 6.0,
                layout.name.y + (layout.name.height - text.height) / 2.0,
                p.text,
            );
        }
        draw_chevron(scene, layout.name, p.text_muted);
    }

    // The category, muted: it says what kind of thing this is, and is not the
    // thing you came to read.
    if !layout.category.is_empty()
        && let Some(text) = labels.get(&view.category)
    {
        draw_text_clipped(
            scene,
            text,
            layout.category,
            layout.category.x + 2.0,
            layout.category.y + (layout.category.height - text.height) / 2.0,
            match hover == Some(PresetBarHit::Category) {
                true => p.text,
                false => p.text_muted,
            },
        );
    }

    if !layout.star.is_empty() {
        let icon = match view.favourite {
            true => crate::icon::Icon::StarFilled,
            false => crate::icon::Icon::Star,
        };
        draw_icon(
            scene,
            icon,
            layout.star.inset(STAR_INSET),
            match (view.favourite, hover == Some(PresetBarHit::Star)) {
                (true, _) => p.accent,
                (false, true) => p.text,
                (false, false) => p.text_muted,
            },
        );
    }

    // The two buttons. **Save is drawn even when it cannot write** — a factory
    // preset is read-only and hiding the button teaches nobody why.
    for (rect, caption, enabled, hit) in [
        (layout.save, SAVE, view.can_save, PresetBarHit::Save),
        (layout.save_as, SAVE_AS, true, PresetBarHit::SaveAs),
    ] {
        if rect.is_empty() {
            continue;
        }
        fill_rect_rounded(scene, rect, m.corner_radius, p.panel);
        if enabled && hover == Some(hit) {
            stroke_rect_rounded(scene, rect, m.corner_radius, 1.0, p.accent);
        }
        if let Some(text) = labels.get(caption) {
            draw_text_clipped(
                scene,
                text,
                rect,
                rect.x + (rect.width - text.width).max(0.0) / 2.0,
                rect.y + (rect.height - text.height) / 2.0,
                match enabled {
                    true => p.text,
                    false => p.text_muted,
                },
            );
        }
    }
}

/// A chevron lying on its side: `‹` or `›`.
///
/// The same two-rule construction [`draw_chevron`] uses, turned a quarter
/// turn, so the three wedges in this window are one shape at three angles
/// rather than three drawings.
fn draw_side_chevron(scene: &mut Scene, area: Rect, forwards: bool, colour: crate::theme::Color) {
    let middle_x = area.x + area.width / 2.0;
    let middle_y = area.y + area.height / 2.0;
    // The **apex** is at step 0 and the arms open away from it, so a wedge
    // that points right has its apex on the right: `x` walks *back* from the
    // apex as the arms spread. Drawn the other way round first, which put a
    // `>` on the button that means "the one before this".
    for step in 0..4 {
        let step = step as f32;
        let x = match forwards {
            true => middle_x + 2.0 - step,
            false => middle_x - 2.0 + step,
        };
        fill_rect(scene, Rect::new(x, middle_y - step, 1.0, 1.0), colour);
        fill_rect(scene, Rect::new(x, middle_y + step, 1.0, 1.0), colour);
    }
}

/// The preset bar in one editor window's header.
pub struct PresetBarChrome<'a> {
    pub layout: crate::canvas::PresetBarLayout,
    pub view: &'a crate::canvas::PresetBarView,
    pub hover: Option<crate::canvas::PresetBarHit>,
}

/// Which panel a floating editor window is drawing, and what it needs.
pub enum EditorWindowChrome<'a> {
    Instrument(Option<InstrumentChrome<'a>>),
    /// **Flopsynth**, which draws a picture of a signal path because that is
    /// what a synthesiser is (`docs/flopsynth-plan.md` §8). Its own variant
    /// rather than a flag on `Instrument`, for the reason `Effect` is one: the
    /// EQ's window and the knob grid are told apart here, and a third kind of
    /// instrument window is the same decision one more time.
    Flopsynth(FlopsynthChrome<'a>),
    /// **The pitch corrector**, which draws a picture of the note because
    /// that is what a corrector is (`docs/tune-plan.md` §7). Its own variant
    /// for the reason Flopsynth's is one: a window with a display across the
    /// top of it and a keyboard under that is not a grid of knobs, and a flag
    /// on `Insert` would be a branch inside every drawing routine rather than
    /// one here.
    Tune(TuneChrome<'a>),
    /// An EQ, which draws a curve because a curve is what an EQ is.
    Effect(EffectChrome),
    /// Every other effect: a grid of knobs read off its own parameter list —
    /// see `fontelle_app::effect_panel`. The same chrome the instrument panel
    /// uses, because a grid of knobs is a grid of knobs.
    Insert(InstrumentChrome<'a>),
    /// One audio clip's properties (TDD §15.1): its waveform, and the rows that
    /// change it.
    AudioClip(AudioEditorChrome<'a>),
}

/// What the audio clip editor draws.
pub struct AudioEditorChrome<'a> {
    pub layout: crate::canvas::AudioEditorLayout,
    pub clip: &'a fontelle_types::AudioClipData,
    /// The waveform strip across the top — the **same** summary the block on
    /// the arrangement draws, so the two pictures cannot disagree.
    pub preview: &'a crate::document::AudioPreview,
    pub sample_rate: u32,
    /// What the clip's mixer track is **called**, worked out where the route
    /// names are rather than in the drawing code — the same arrangement
    /// `MixerChrome::output_label` has, and for the same reason: a canvas may
    /// not see a `Project` (INVARIANT 2).
    pub route_label: &'a str,
    pub hover: Option<crate::canvas::AudioField>,
}

/// The right-click menu (see [`crate::canvas::context_menu_layout`]).
///
/// Its own function taking the menu rather than a chrome, because two surfaces
/// draw one: the studio's window and the instrument editor's, which is where a
/// right-clicked knob opens one.
pub fn draw_context_menu(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    menu: Option<&crate::canvas::ContextMenu>,
    // The name prompt's field, when this menu is one. Drawn **over the first
    // row**, which is the row that used to be the prompt's caption with a
    // block character stuck on the end.
    field: Option<&TextFieldChrome>,
) {
    let Some(menu) = menu else { return };
    if menu.frame.is_empty() {
        return;
    }
    let p = &theme.palette;
    let m = &theme.metrics;
    // A border against the panel's own ground, for the reason `draw_lane_menu`
    // gives: it reads as "in front of" without needing a blur.
    fill_rect_rounded(scene, menu.frame, m.corner_radius, p.border);
    fill_rect_rounded(
        scene,
        menu.frame.inset(1.0),
        m.corner_radius,
        p.panel_header,
    );
    // A menu laid out in columns has a rule between them, so the eye reads
    // three lists rather than one wide one; a menu that scrolls has a thumb,
    // so the wheel is something you know to reach for.
    for column in 0..menu.columns() {
        fill_rect(scene, menu.column_rule(column), p.border);
    }
    let thumb = menu.scrollbar();
    if !thumb.is_empty() {
        fill_rect_rounded(
            scene,
            thumb,
            thumb.width / 2.0,
            p.text_muted.with_alpha(0xa0),
        );
    }

    for (index, (row, entry)) in menu.rows.iter().zip(menu.entries.iter()).enumerate() {
        if row.is_empty() {
            continue;
        }
        // A favourite is **lit**: a wash of the accent under the whole row,
        // so it is the most visible thing in the list wherever it appears —
        // in the favourites section and again in the full list below.
        if entry.is_favorite() {
            fill_rect_rounded(
                scene,
                *row,
                m.corner_radius,
                p.accent.with_alpha(FAVORITE_WASH),
            );
        }
        if entry.separator && row.y > menu.frame.y + 1.0 {
            fill_rect(
                scene,
                Rect::new(row.x, row.y, row.width, m.border_width.max(1.0)),
                p.border,
            );
        }
        // The star, at the row's right end, on rows that can have one. Lit in
        // the accent when it is a favourite; a quiet outline when it is not,
        // which says "this could be" without shouting on every row.
        let star = menu.star_rect(index);
        if !star.is_empty() {
            let (icon, ink) = if entry.is_favorite() {
                (crate::icon::Icon::StarFilled, p.accent)
            } else {
                (crate::icon::Icon::Star, p.text_muted)
            };
            draw_icon(scene, icon, star.inset(STAR_INSET), ink);
        }
        // **The first row of a name prompt is the field.** It was already the
        // row that held what you had typed; it is now a box you can see it in.
        if index == 0
            && let Some(field) = field
        {
            draw_text_field(scene, theme, labels, row.inset(2.0), field);
            continue;
        }
        let Some(text) = labels_get(labels, &entry.label) else {
            continue;
        };
        // The caption stops where the star starts, rather than running under
        // it.
        let caption = Rect::new(row.x, row.y, (row.width - star.width).max(0.0), row.height);
        draw_text_clipped(
            scene,
            text,
            caption,
            row.x + crate::canvas::MENU_TEXT_INSET,
            row.y + (row.height - text.height) / 2.0,
            // A greyed entry is drawn in the same ink as a panel's border,
            // which is this theme's "there, and not for you".
            if entry.enabled { p.text } else { p.border },
        );
    }
}

/// What a text field needs drawn, beyond the string itself.
///
/// The **pixel** offsets are here rather than worked out below, because
/// placing a caret means measuring the text in front of it and only the layer
/// that shapes text can do that (INVARIANT 2). The window measures; this
/// paints.
pub struct TextFieldChrome {
    /// **Owned**, not borrowed: the window builds this while it still holds
    /// itself immutably and then hands it to a draw that needs the scene
    /// mutably. A short string cloned once a frame, only while something is
    /// being typed into, is not a cost worth a lifetime for.
    pub entry: crate::canvas::TextEntry,
    /// How far along the text the caret sits, in points from the text's left.
    pub caret_x: f32,
    /// And the two ends of the selection, when there is one.
    pub selection: Option<(f32, f32)>,
    /// What the field says when it is empty — the prompt, greyed.
    pub placeholder: &'static str,
    /// Whether the caret is in its **on** half. A caret that does not blink
    /// is easy to mistake for a character; one that blinks is unmistakably a
    /// cursor, and it is the cheapest way to say "type here".
    pub caret_on: bool,
}

/// How far the text sits in from the field's own edge.
const FIELD_INSET: f32 = 6.0;
/// How wide the caret is drawn. Two, not one: a single point disappears
/// against text at this size on a high-density screen.
const CARET_WIDTH: f32 = 2.0;

/// **A box you can obviously type in.**
///
/// > *"it doesn't look like a input field it's just text on a background
/// > making it look like it's a label and not somewhere you can type."*
///
/// Exactly so, and the fix is the three things a field has that a label does
/// not: a **recess** it is sunk into, so the eye reads a hole rather than a
/// caption; a **lit border** while it has the keyboard, so it is obvious which
/// box the typing is going into; and a **caret**, which is the only thing that
/// says where the next character will land.
pub fn draw_text_field(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    rect: Rect,
    field: &TextFieldChrome,
) {
    if rect.is_empty() {
        return;
    }
    let p = &theme.palette;
    let m = &theme.metrics;
    // The recess: darker than the panel it sits on, which is what makes it
    // read as somewhere text goes in rather than somewhere text comes out.
    fill_rect_rounded(scene, rect, m.corner_radius, p.window.with_alpha(0xd0));
    stroke_rect_rounded(
        scene,
        rect,
        m.corner_radius,
        1.0,
        // Lit while it has the keyboard. This is the same signal the focused
        // pane's edge gives, in the same ink, so the two read as one idea.
        if field.caret_on || field.selection.is_some() {
            p.accent.with_alpha(0xd0)
        } else {
            p.accent.with_alpha(0x70)
        },
    );

    let text_x = rect.x + FIELD_INSET;
    let entry = &field.entry;

    // The selection, under the text: a wash rather than an inversion, so the
    // characters keep the colour they had and stay legible.
    if let Some((from, to)) = field.selection {
        let band = Rect::new(
            text_x + from.min(to),
            rect.y + 3.0,
            (to - from).abs().max(1.0),
            (rect.height - 6.0).max(1.0),
        )
        .intersection(&rect);
        if !band.is_empty() {
            fill_rect_rounded(scene, band, 2.0, p.accent.with_alpha(0x55));
        }
    }

    // The text, or the prompt when there is none.
    let (caption, ink) = if entry.is_empty() {
        (field.placeholder, p.text_muted)
    } else {
        (entry.text(), p.text)
    };
    if let Some(shaped) = labels_get(labels, caption) {
        draw_text_clipped(
            scene,
            shaped,
            rect.inset(2.0),
            text_x,
            rect.y + (rect.height - shaped.height) / 2.0,
            ink,
        );
    }

    // The caret, last and over everything. Only on its **on** half, and never
    // while there is a selection — a caret inside a highlighted range is two
    // answers to "where does the next character go".
    if field.caret_on && field.selection.is_none() {
        let caret = Rect::new(
            text_x + field.caret_x,
            rect.y + 3.0,
            CARET_WIDTH,
            (rect.height - 6.0).max(1.0),
        )
        .intersection(&rect);
        if !caret.is_empty() {
            fill_rect(scene, caret, p.accent);
        }
    }
}

/// How strong the accent wash under a favourite row is. Enough to read as
/// highlighted against the menu's ground; not so much that the caption on it
/// loses contrast.
const FAVORITE_WASH: u8 = 0x38;

/// How far the star sits in from its own box, so it does not touch the row's
/// edge or the rule above it.
const STAR_INSET: f32 = 4.0;

fn draw_tooltip(scene: &mut Scene, theme: &Theme, chrome: &Chrome<'_>) {
    let Some((caption, rect)) = chrome.tooltip else {
        return;
    };
    if rect.is_empty() {
        return;
    }
    let Some(text) = labels_get(chrome.labels, caption) else {
        return;
    };
    let p = &theme.palette;
    let m = &theme.metrics;
    // Its own outline, because it floats over whatever is underneath: without
    // one a tip over a panel reads as a hole in the panel.
    fill_rect_rounded(scene, rect, m.corner_radius, p.border);
    fill_rect_rounded(scene, rect.inset(1.0), m.corner_radius, p.panel_header);
    draw_text_clipped(
        scene,
        text,
        rect,
        rect.x + crate::tooltip::TOOLTIP_PAD,
        rect.y + (rect.height - text.height) / 2.0,
        p.text,
    );
}

/// What the pointer is carrying, and what would happen if it let go.
///
/// Two things are drawn, and they are two different claims:
///
/// - **The mark**, over whatever would change — the channel row that would
///   take the sound, the band a new channel would appear in, the row at the
///   foot of the arrangement a clip would be made on. In the accent, because
///   the accent is what this window uses for "this one".
/// - **The chip**, under the pointer, saying what is in hand and what would
///   become of it. Outlined in the warning ink when the answer is *nothing*:
///   a gesture that will not work has to look different **before** the button
///   comes up, which is the whole of the report this exists for.
///
/// The warning ink is [`crate::theme::Palette::meter_peak`] rather than a
/// colour of its own. It is the palette's one "something is wrong here" hue —
/// a clipping meter, the limiter's read-out — and a refused drop is the same
/// sentence said about a gesture. A new field would mean a theme format
/// version, for a border.
pub fn draw_carry(scene: &mut Scene, theme: &Theme, labels: &Labels, chrome: &CarryChrome<'_>) {
    let p = &theme.palette;
    let m = &theme.metrics;
    let target = &chrome.target;

    // The mark first, so the chip is over it rather than under it.
    if let Some(rect) = target.mark()
        && !rect.is_empty()
    {
        fill_rect_rounded(scene, rect, m.corner_radius, p.accent.with_alpha(0x33));
        stroke_rect_rounded(scene, rect.inset(0.75), m.corner_radius, 1.5, p.accent);
    }
    // A clip lands at one tick on that row, and the row is the whole width of
    // the grid — so without this the mark would say "somewhere along here".
    if let crate::canvas::CarryTarget::Clip { row, at, .. } = target
        && !row.is_empty()
    {
        let caret = Rect::new(at - 1.0, row.y, 2.0, row.height).intersection(row);
        fill_rect(scene, caret, p.accent);
    }
    // A new channel goes on the end of the list; the plus says "another one"
    // rather than "this one".
    if let crate::canvas::CarryTarget::NewChannel { rect } = target
        && rect.height >= 12.0
    {
        let side = rect.height.min(16.0);
        draw_icon(
            scene,
            crate::icon::Icon::Plus,
            Rect::new(
                rect.x + 4.0,
                rect.y + (rect.height - side) / 2.0,
                side,
                side,
            ),
            p.accent,
        );
    }

    // The chip. Sized to its own words, so a long file name is readable and a
    // short one is not padded out into a banner.
    let label = labels.get(chrome.label);
    let note = (!chrome.note.is_empty())
        .then(|| labels.get_small(chrome.note))
        .flatten();
    let width = label
        .map_or(0.0, |text| text.width)
        .max(note.map_or(0.0, |text| text.width));
    let height = label.map_or(0.0, |text| text.height)
        + note.map_or(0.0, |text| text.height + CARRY_LINE_GAP);
    // Capped, and the words clipped inside it: a sample called
    // `Doll_Break_120_PL_FINAL_v3.wav` is a real file name, and a chip too
    // wide for the window is a chip `carry_chip` refuses to place — which
    // would take the whole gesture's feedback away over a long name.
    let width = width.min((chrome.bounds.width * 0.4).max(CARRY_MIN_W));
    if width <= 0.0 || height <= 0.0 {
        return;
    }
    let size = (
        width + crate::canvas::CARRY_PAD * 2.0,
        height + crate::canvas::CARRY_PAD * 2.0,
    );
    let chip = if chrome.lifted {
        crate::canvas::carry_chip_lifted(size, chrome.at, chrome.bounds)
    } else {
        crate::canvas::carry_chip(size, chrome.at, chrome.bounds)
    };
    if chip.is_empty() {
        return;
    }
    let edge = if target.refuses() {
        p.meter_peak
    } else if target.lands() {
        p.accent
    } else {
        // Back over the list it came from: nothing is wrong and nothing is
        // promised.
        p.border
    };
    fill_rect_rounded(scene, chip, m.corner_radius, edge);
    fill_rect_rounded(scene, chip.inset(1.5), m.corner_radius, p.panel_header);
    let mut y = chip.y + crate::canvas::CARRY_PAD;
    if let Some(text) = label {
        draw_text_clipped(
            scene,
            text,
            chip,
            chip.x + crate::canvas::CARRY_PAD,
            y,
            if target.refuses() {
                p.text_muted
            } else {
                p.text
            },
        );
        y += text.height + CARRY_LINE_GAP;
    }
    if let Some(text) = note {
        draw_text_clipped(
            scene,
            text,
            chip,
            chip.x + crate::canvas::CARRY_PAD,
            y,
            if target.refuses() {
                p.meter_peak
            } else {
                p.text_muted
            },
        );
    }
}

/// Between the chip's two lines.
const CARRY_LINE_GAP: f32 = 2.0;

/// The narrowest the chip is allowed to be squeezed to on a small window.
const CARRY_MIN_W: f32 = 80.0;

/// The transport bar (item 7 of `docs/first-usable-plan.md`).
///
/// Drawn whether or not there is an engine behind it — a window that changes
/// shape when the sound card goes away is worse than one that says so — but
/// everything in it is muted and the playhead is absent when `view.available`
/// is false.
pub fn draw_transport_bar(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    chrome: &TransportChrome<'_>,
) {
    let l = &chrome.layout;
    let view = &chrome.view;
    let p = &theme.palette;
    let m = &theme.metrics;

    if l.bar.is_empty() {
        return;
    }

    let bar = rounded(l.bar, m.corner_radius);
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        p.panel_header.to_peniko(),
        None,
        &bar,
    );
    if m.border_width > 0.0 {
        scene.stroke(
            &Stroke::new(m.border_width as f64),
            Affine::IDENTITY,
            p.border.to_peniko(),
            None,
            &bar,
        );
    }

    // A control nobody can use is drawn in the muted ink, which is the same
    // signal a disabled control gives everywhere else.
    let ink = if view.available { p.text } else { p.text_muted };

    for (rect, what) in [
        (l.play, TransportHit::Play),
        (l.stop, TransportHit::Stop),
        (l.loop_toggle, TransportHit::ToggleLoop),
        (l.record, TransportHit::ToggleRecord),
        (l.metronome, TransportHit::ToggleMetronome),
    ] {
        if rect.is_empty() {
            continue;
        }
        if chrome.hover == Some(what) && view.available {
            scene.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                p.border.to_peniko(),
                None,
                &rounded(rect, m.corner_radius),
            );
        }
        // Lit when the thing it controls is on: play while rolling, the loop
        // button while looping. Recording takes the peak colour, because it is
        // the one transport state with a consequence on disk.
        let colour = match what {
            TransportHit::Play if view.recording => p.meter_peak,
            TransportHit::Play if view.playing => p.accent,
            TransportHit::ToggleLoop if view.looping => p.accent,
            // Armed takes the peak ink — it is the one transport state with a
            // consequence on disk, and the one worth not pressing by accident.
            TransportHit::ToggleRecord if view.armed => p.meter_peak,
            TransportHit::ToggleMetronome if view.metronome => p.accent,
            _ => ink,
        };
        let glyph = rect.inset(rect.height * 0.28);
        match what {
            // Drawn from the icon set rather than by hand here, so the play
            // triangle on the bar and the one anywhere else are one shape.
            TransportHit::Play => draw_icon(scene, crate::icon::Icon::Play, glyph, colour),
            TransportHit::Stop => draw_icon(scene, crate::icon::Icon::Stop, glyph, colour),
            TransportHit::ToggleLoop => draw_icon(scene, crate::icon::Icon::Loop, glyph, colour),
            TransportHit::ToggleRecord => {
                draw_icon(scene, crate::icon::Icon::Record, glyph, colour)
            }
            TransportHit::ToggleMetronome => {
                draw_icon(scene, crate::icon::Icon::Metronome, glyph, colour)
            }
            // Neither the ruler nor any of the document's boxes is a glyph
            // button, and each is drawn by its own code below.
            TransportHit::Scrub(_)
            | TransportHit::Tempo
            | TransportHit::Signature
            | TransportHit::Mode
            | TransportHit::Help => {}
        }
    }

    // The `?`: a glyph button like the five on the left, drawn quieter — it
    // is a hint, not a transport control — until the pointer is on it.
    if !l.help.is_empty() {
        let hot = chrome.hover == Some(TransportHit::Help) && view.available;
        if hot {
            fill_rect_rounded(scene, l.help, m.corner_radius, p.border);
        }
        draw_icon(
            scene,
            crate::icon::Icon::Help,
            l.help.inset(l.help.height * 0.28),
            if hot { p.text } else { p.text_muted },
        );
    }

    draw_text(
        scene,
        chrome.readout,
        l.readout.x,
        l.readout.y + (l.readout.height - chrome.readout.height) / 2.0,
        ink,
    );

    // The two document boxes. Framed **always**, not only on hover: the last
    // time a read-out here was drawn as a bare word between two buttons, it
    // was reported as a missing feature rather than as a control nobody could
    // see (see the snap chip, and the lane chip before it).
    // The mode chip is the third box: it says what a press of play will do,
    // and it is lit in the accent while it says "Clip", because a transport
    // that plays one part of a song is a state worth noticing.
    for (rect, text, what) in [
        (l.tempo, chrome.tempo, TransportHit::Tempo),
        (l.signature, chrome.signature, TransportHit::Signature),
        (l.mode, chrome.mode, TransportHit::Mode),
    ] {
        if rect.is_empty() {
            continue;
        }
        let box_rect = rect.inset(2.0);
        // While a tempo is being typed the box **is** a field — the recess,
        // the lit edge, the caret — so there is no doubt where the keys go.
        if what == TransportHit::Tempo
            && let Some(field) = &chrome.tempo_field
        {
            draw_text_field(scene, theme, labels, box_rect, field);
            continue;
        }
        let clip_mode = what == TransportHit::Mode && chrome.clip_mode;
        fill_rect_rounded(
            scene,
            box_rect,
            m.corner_radius,
            if clip_mode { p.accent } else { p.window },
        );
        scene.stroke(
            &Stroke::new(m.border_width as f64),
            Affine::IDENTITY,
            if chrome.hover == Some(what) && view.available {
                p.accent.to_peniko()
            } else {
                p.border.to_peniko()
            },
            None,
            &rounded(box_rect, m.corner_radius),
        );
        draw_text_clipped(
            scene,
            text,
            box_rect,
            box_rect.x + ((box_rect.width - text.width) / 2.0).max(2.0),
            box_rect.y + (box_rect.height - text.height) / 2.0,
            if clip_mode { p.panel } else { ink },
        );
    }

    draw_ruler(scene, theme, l.ruler, view, chrome.marker_sample);
    draw_meter(scene, theme, l.meter, view, &chrome.meters);
}

/// The mixer: a fader, a pan, two switches and a meter per track (TDD §13).
/// The EQ editor: a grid, the curve, and a handle per band.
/// The frequencies the EQ's grid is drawn at, and what each line says.
///
/// The ones people name out loud. Labelled, unlike the first draft: a curve
/// over an unlabelled grid says a band is "somewhere around there", and
/// "somewhere around there" is not a decision anybody can repeat.
const EQ_GRID_HZ: [(f32, &str); 6] = [
    (50.0, "50"),
    (100.0, "100"),
    (500.0, "500"),
    (1_000.0, "1k"),
    (5_000.0, "5k"),
    (10_000.0, "10k"),
];

/// Every caption the axis needs shaped, so the window that owns the font cache
/// can prepare them without knowing how the grid is drawn.
pub const EQ_AXIS_CAPTIONS: [&str; 9] = ["50", "100", "500", "1k", "5k", "10k", "+12", "0", "-12"];

fn draw_effect(scene: &mut Scene, theme: &Theme, labels: &Labels, chrome: &EffectChrome) {
    use crate::canvas::{EQ_MAX_DB, EqField, eq_x_of_freq, eq_y_of_gain};
    let p = &theme.palette;
    let m = &theme.metrics;
    let area = chrome.layout.curve;
    if area.is_empty() {
        return;
    }

    fill_rect(scene, area, p.panel);

    // The decade lines, with their frequencies along the bottom.
    for (hz, caption) in EQ_GRID_HZ {
        let x = eq_x_of_freq(area, hz);
        fill_rect(
            scene,
            Rect::new(x, area.y, m.border_width.max(1.0), area.height),
            p.grid_line,
        );
        if let Some(text) = labels_get(labels, caption) {
            draw_text_clipped(
                scene,
                text,
                area,
                x + 3.0,
                area.bottom() - text.height - 2.0,
                p.text_muted,
            );
        }
    }
    // And the dB lines, with 0 picked out — it is the one a curve is read
    // against, and a grid where every line looks the same has no datum.
    for (db, caption) in [
        (EQ_MAX_DB / 2.0, "+12"),
        (0.0, "0"),
        (-EQ_MAX_DB / 2.0, "-12"),
    ] {
        let y = eq_y_of_gain(area, db);
        fill_rect(
            scene,
            Rect::new(area.x, y, area.width, m.border_width.max(1.0)),
            if db == 0.0 {
                p.grid_line_strong
            } else {
                p.grid_line
            },
        );
        if let Some(text) = labels_get(labels, caption) {
            draw_text_clipped(scene, text, area, area.x + 3.0, y + 1.0, p.text_muted);
        }
    }

    let ink = if chrome.bypassed {
        p.text_muted
    } else {
        p.accent
    };

    // The analyser, **behind everything**: it is what the curve is drawn
    // against, so a band's shape has to read on top of it rather than under
    // it. Filled to the floor of the plot rather than to the zero line the
    // curve fills to — a spectrum is a level and the curve is a gain, and the
    // two do not share a datum.
    if chrome.spectrum.len() > 1 {
        let floor = area.bottom() as f64;
        let mut fill = BezPath::new();
        fill.move_to((chrome.spectrum[0].0 as f64, floor));
        for (x, y) in &chrome.spectrum {
            fill.line_to((*x as f64, *y as f64));
        }
        fill.line_to((chrome.spectrum[chrome.spectrum.len() - 1].0 as f64, floor));
        fill.close_path();
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            // The panel's own text colour at low alpha rather than the
            // accent: the accent is the *curve*, and two shapes in one ink
            // over each other is a picture with nothing to read.
            Color([p.text.0[0], p.text.0[1], p.text.0[2], 44]).to_peniko(),
            None,
            &fill,
        );
        let mut path = BezPath::new();
        path.move_to((chrome.spectrum[0].0 as f64, chrome.spectrum[0].1 as f64));
        for (x, y) in &chrome.spectrum[1..] {
            path.line_to((*x as f64, *y as f64));
        }
        scene.stroke(
            &Stroke::new(1.0),
            Affine::IDENTITY,
            Color([p.text.0[0], p.text.0[1], p.text.0[2], 0x88]).to_peniko(),
            None,
            &path,
        );
    }

    // The band in hand, behind the sum and fainter: what every parametric EQ
    // shows, so a cut can be seen against the shape it is being made in.
    if chrome.band_curve.len() > 1 {
        let mut path = BezPath::new();
        path.move_to((chrome.band_curve[0].0 as f64, chrome.band_curve[0].1 as f64));
        for (x, y) in &chrome.band_curve[1..] {
            path.line_to((*x as f64, *y as f64));
        }
        scene.stroke(
            &Stroke::new(1.0),
            Affine::IDENTITY,
            p.text_muted.to_peniko(),
            None,
            &path,
        );
    }

    // The curve itself, filled back to the zero line so it reads as a shape
    // rather than as a wire. Drawn even when the EQ is bypassed, greyed rather
    // than hidden: what a bypassed effect is *set to* is what you are deciding
    // whether to switch back in.
    if chrome.curve.len() > 1 {
        let zero = eq_y_of_gain(area, 0.0) as f64;
        let mut fill = BezPath::new();
        fill.move_to((chrome.curve[0].0 as f64, zero));
        for (x, y) in &chrome.curve {
            fill.line_to((*x as f64, *y as f64));
        }
        fill.line_to((chrome.curve[chrome.curve.len() - 1].0 as f64, zero));
        fill.close_path();
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            Color([ink.0[0], ink.0[1], ink.0[2], 56]).to_peniko(),
            None,
            &fill,
        );

        let mut path = BezPath::new();
        path.move_to((chrome.curve[0].0 as f64, chrome.curve[0].1 as f64));
        for (x, y) in &chrome.curve[1..] {
            path.line_to((*x as f64, *y as f64));
        }
        scene.stroke(
            &Stroke::new(2.0),
            Affine::IDENTITY,
            ink.to_peniko(),
            None,
            &path,
        );
    }

    for handle in &chrome.layout.handles {
        let selected = chrome.layout.selected == handle.band;
        let lit =
            selected || chrome.active == Some(handle.band) || chrome.hover == Some(handle.band);
        fill_rect_rounded(
            scene,
            handle.rect,
            handle.rect.width / 2.0,
            if lit { p.accent } else { p.note },
        );
        scene.stroke(
            &Stroke::new(m.border_width as f64),
            Affine::IDENTITY,
            if selected { p.text } else { p.border }.to_peniko(),
            None,
            &RoundedRect::from_rect(
                KRect::new(
                    handle.rect.x as f64,
                    handle.rect.y as f64,
                    handle.rect.right() as f64,
                    handle.rect.bottom() as f64,
                ),
                handle.rect.width as f64 / 2.0,
            ),
        );
        // The band's number on its handle, which is what ties it to the chip
        // below and to `band3.gain` in an automation lane's name.
        if let Some(text) = labels_get(labels, &(handle.band + 1).to_string()) {
            draw_text_clipped(
                scene,
                text,
                handle.rect,
                handle.rect.x + (handle.rect.width - text.width) / 2.0,
                handle.rect.y + (handle.rect.height - text.height) / 2.0,
                p.panel,
            );
        }
    }

    // The eight chips. A fresh EQ has every band switched off and therefore no
    // handles at all, and this row is what makes that state usable: the bands
    // are visibly there, and clicking one switches it on.
    for (index, chip) in &chrome.layout.bands {
        if chip.is_empty() {
            continue;
        }
        let band = chrome.config.bands[*index];
        let selected = chrome.layout.selected == *index;
        fill_rect_rounded(
            scene,
            *chip,
            m.corner_radius * 0.5,
            if band.enabled { p.accent } else { p.panel },
        );
        stroke_rect_rounded(
            scene,
            chip.inset(0.5),
            m.corner_radius * 0.5,
            m.border_width.max(1.0),
            if selected { p.text } else { p.border },
        );
        if let Some(text) = labels_get(labels, &(index + 1).to_string()) {
            draw_text_clipped(
                scene,
                text,
                *chip,
                chip.x + (chip.width - text.width) / 2.0,
                chip.y + (chip.height - text.height) / 2.0,
                if band.enabled { p.panel } else { p.text_muted },
            );
        }
    }

    // And the selected band's controls: type, frequency, gain, Q, which part
    // of the stereo image it works on, solo, off — and the effect's wet/dry.
    for (field, rect) in &chrome.layout.fields {
        if rect.is_empty() {
            continue;
        }
        let band = chrome.config.bands[chrome.layout.selected];
        let on = match field {
            EqField::Solo => band.solo,
            EqField::Delete => !band.enabled,
            _ => false,
        };
        let lit = chrome.hover_field == Some(*field);
        fill_rect_rounded(
            scene,
            *rect,
            m.corner_radius * 0.5,
            if on {
                p.accent
            } else if lit {
                p.border
            } else {
                p.panel
            },
        );
        let caption =
            crate::canvas::eq_field_caption(*field, &chrome.config, chrome.layout.selected);
        if let Some(text) = labels_get(labels, &caption) {
            draw_text_clipped(
                scene,
                text,
                *rect,
                rect.x + (rect.width - text.width).max(0.0) / 2.0,
                rect.y + (rect.height - text.height) / 2.0,
                if on {
                    p.panel
                } else if band.enabled {
                    p.text
                } else {
                    p.text_muted
                },
            );
        }
    }
}

fn draw_mixer(scene: &mut Scene, theme: &Theme, labels: &Labels, chrome: &MixerChrome<'_>) {
    let p = &theme.palette;
    let m = &theme.metrics;
    let l = &chrome.layout;

    for layout in l.strips.iter().chain(l.master.iter()) {
        let Some(strip) = chrome.strips.get(layout.index) else {
            continue;
        };
        let peaks = chrome.peaks.get(layout.index).copied().unwrap_or([0.0; 2]);
        draw_mixer_strip(scene, theme, labels, chrome, layout, strip, peaks);
    }

    // The `+` past the last strip. *"then the plus button moves to the next
    // empty space so you can just add as many new tracks as you want within
    // the mixer itself"* — so it is drawn as an empty column waiting to be
    // filled rather than as a button sitting on the background.
    if !l.add_track.is_empty() {
        let lit = chrome.hover == Some(crate::canvas::MixerHit::AddTrack);
        fill_rect_rounded(
            scene,
            l.add_track,
            m.corner_radius,
            if lit { p.panel_header } else { p.panel },
        );
        stroke_rect_rounded(
            scene,
            l.add_track.inset(0.5),
            m.corner_radius,
            m.border_width.max(1.0),
            p.border,
        );
        if let Some(text) = labels_get(labels, ADD_TRACK) {
            draw_text_clipped(
                scene,
                text,
                l.add_track,
                l.add_track.x + (l.add_track.width - text.width) / 2.0,
                l.add_track.y + (l.add_track.height - text.height) / 2.0,
                if lit { p.text } else { p.text_muted },
            );
        }
    }

    if let Some(options) = &l.options {
        draw_track_options(scene, theme, labels, chrome, options);
    }

    // The seam the master sits behind, so the eye reads it as a different kind
    // of thing rather than as the strip that happens to be first. Drawn on its
    // *right*, which is the side the strips are on now that the master is
    // pinned to the panel's left edge — on the far side it fell outside the
    // panel entirely and drew nothing.
    if let Some(master) = &l.master
        && !l.list.is_empty()
    {
        fill_rect(
            scene,
            Rect::new(
                master.frame.right() + 2.0,
                master.frame.y,
                m.border_width.max(1.0),
                master.frame.height,
            ),
            p.border,
        );
    }

    // Last, over everything: an open menu is above the panel it hangs from.
    draw_output_menu(scene, theme, labels, chrome);
}

/// The track-options column (TDD §13.2, §13.4).
fn draw_track_options(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    chrome: &MixerChrome<'_>,
    options: &crate::canvas::TrackOptionsLayout,
) {
    use crate::canvas::{MixerHit, OptionsHit};

    let p = &theme.palette;
    let m = &theme.metrics;
    let hovering = |what: OptionsHit| chrome.hover == Some(MixerHit::Options(what));
    let Some(strip) = chrome.strips.get(options.track) else {
        return;
    };

    fill_rect_rounded(scene, options.frame, m.corner_radius, p.panel);
    stroke_rect_rounded(
        scene,
        options.frame.inset(0.5),
        m.corner_radius,
        m.border_width.max(1.0),
        p.border,
    );

    // The track's own colour, the same cap its strip wears — which is what
    // says the column and the highlighted strip are one thing.
    fill_rect(
        scene,
        Rect::new(options.frame.x, options.frame.y, options.frame.width, 3.0),
        if strip.is_master {
            p.accent
        } else {
            Color(strip.color)
        },
    );

    let row_text = |scene: &mut Scene, caption: &str, rect: Rect, x: f32, colour: Color| {
        if let Some(text) = labels_get(labels, caption) {
            draw_text_clipped(
                scene,
                text,
                rect,
                x,
                rect.y + (rect.height - text.height) / 2.0,
                colour,
            );
        }
    };

    row_text(
        scene,
        &strip.name,
        options.title,
        options.title.x + 2.0,
        p.text,
    );
    if chrome.renaming == Some(options.track) {
        // The width is read separately from the draw, so a name backspaced
        // down to nothing still shows a caret: an empty field with nothing in
        // it at all is one you cannot tell from a dead panel, and clearing the
        // name is the first thing anybody does when renaming.
        let width = labels_get(labels, &strip.name).map_or(0.0, |text| text.width);
        draw_rename_marks(
            scene,
            theme,
            options.title,
            options.title.x + 3.0,
            width,
            chrome.rename,
        );
    }

    // Where it comes from (TDD §15.4). Above the output row, so the column
    // reads top to bottom as a signal path.
    if !options.input.is_empty() {
        let lit = hovering(OptionsHit::Input);
        fill_rect_rounded(
            scene,
            options.input,
            m.corner_radius * 0.5,
            if lit { p.border } else { p.panel_header },
        );
        let caption = chrome.input_label.clone();
        row_text(
            scene,
            &caption,
            options.input,
            options.input.x + 4.0,
            if lit { p.text } else { p.text_muted },
        );
    }

    // Where it goes (§13.2).
    if !options.output.is_empty() {
        let lit = hovering(OptionsHit::Output);
        fill_rect_rounded(
            scene,
            options.output,
            m.corner_radius * 0.5,
            if lit { p.border } else { p.panel_header },
        );
        let caption = chrome.output_label.clone();
        row_text(
            scene,
            &caption,
            options.output,
            options.output.x + 4.0,
            if lit { p.text } else { p.text_muted },
        );
    }

    row_text(
        scene,
        EFFECTS_HEADING,
        options.inserts_title,
        options.inserts_title.x + 2.0,
        p.text_muted,
    );

    for row in &options.inserts {
        let Some(insert) = strip.inserts.get(row.slot) else {
            continue;
        };
        // Where the dragged row would land, drawn as the row's own highlight
        // so a reorder shows what it is about to do rather than only what it
        // has done.
        let landing = chrome.insert_drag.is_some_and(|(_, over)| over == row.slot);
        let lit = landing
            || hovering(OptionsHit::Insert(row.slot))
            || hovering(OptionsHit::Grip(row.slot))
            || hovering(OptionsHit::Bypass(row.slot))
            || hovering(OptionsHit::Remove(row.slot));
        fill_rect_rounded(
            scene,
            row.frame.inset(1.0),
            m.corner_radius * 0.5,
            if landing {
                p.accent
            } else if lit {
                p.border
            } else {
                p.panel_header
            },
        );

        // A filled dot is in the chain, a hollow one is out — the same shape
        // the strip's own rack uses, because they are the same switch.
        let dot_size = (row.bypass.height * 0.4).min(7.0);
        fill_rect_rounded(
            scene,
            Rect::new(
                row.bypass.x + (row.bypass.width - dot_size) / 2.0,
                row.bypass.y + (row.bypass.height - dot_size) / 2.0,
                dot_size,
                dot_size,
            ),
            dot_size / 2.0,
            if insert.bypassed {
                p.text_muted
            } else {
                p.accent
            },
        );
        row_text(
            scene,
            &insert.label,
            row.name,
            row.name.x + 2.0,
            if insert.bypassed {
                p.text_muted
            } else {
                p.text
            },
        );
        // Wet/dry, as a **dial** with its number beside it. A knob rather than
        // a groove because that is what it is: a mix is a setting you turn to
        // taste, and a horizontal bar in a rack of them reads as a level.
        if !row.mix.is_empty() {
            let dial = crate::canvas::insert_mix_dial(row.mix);
            let hot = hovering(OptionsHit::InsertMix(row.slot));
            draw_knob(
                scene,
                theme,
                dial,
                insert.mix.clamp(0.0, 1.0),
                hot,
                insert.mix_automated,
            );
            let caption = crate::canvas::format_mix(insert.mix);
            let beside = Rect::new(
                dial.right() + 2.0,
                row.mix.y,
                (row.mix.right() - dial.right() - 2.0).max(0.0),
                row.mix.height,
            );
            if let Some(text) = labels_get(labels, &caption)
                && !beside.is_empty()
            {
                draw_text_clipped(
                    scene,
                    text,
                    beside,
                    beside.x,
                    beside.y + (beside.height - text.height) / 2.0,
                    if insert.bypassed {
                        p.text_muted
                    } else {
                        p.text
                    },
                );
            }
        }
        row_text(scene, GRIP, row.grip, row.grip.x + 2.0, p.text_muted);
        row_text(
            scene,
            REMOVE,
            row.remove,
            row.remove.x + (row.remove.width - 6.0).max(0.0) / 2.0,
            if hovering(OptionsHit::Remove(row.slot)) {
                p.text
            } else {
                p.text_muted
            },
        );
    }

    if !options.add_insert.is_empty() {
        let lit = hovering(OptionsHit::AddInsert);
        if lit {
            fill_rect_rounded(
                scene,
                options.add_insert.inset(1.0),
                m.corner_radius * 0.5,
                p.border,
            );
        }
        row_text(
            scene,
            ADD_EFFECT,
            options.add_insert,
            options.add_insert.x + 4.0,
            if lit { p.text } else { p.text_muted },
        );
    }

    // --- the sends (§13.2) ---
    if !options.sends_title.is_empty() {
        row_text(
            scene,
            SENDS_HEADING,
            options.sends_title,
            options.sends_title.x + 2.0,
            p.text_muted,
        );
    }
    for row in &options.sends {
        let Some(send) = strip.sends.get(row.index) else {
            continue;
        };
        let lit = hovering(OptionsHit::Send(row.index))
            || hovering(OptionsHit::SendTap(row.index))
            || hovering(OptionsHit::SendLevel(row.index))
            || hovering(OptionsHit::SendRemove(row.index));
        fill_rect_rounded(
            scene,
            row.frame.inset(1.0),
            m.corner_radius * 0.5,
            if lit { p.border } else { p.panel_header },
        );

        // The tap point, as the two letters that name it. A switch with two
        // answers, so it says which one is on rather than dropping a menu.
        row_text(
            scene,
            if send.pre_fader { SEND_PRE } else { SEND_POST },
            row.tap,
            row.tap.x + 1.0,
            if send.pre_fader {
                p.accent
            } else {
                p.text_muted
            },
        );
        row_text(
            scene,
            &send.target_name,
            row.target,
            row.target.x + 2.0,
            p.text,
        );

        // The level: a groove filled from the left, the way a horizontal
        // fader reads, with its value written over it. A number alone would be
        // exact and unreadable at a glance; a bar alone would be readable and
        // unrepeatable.
        if !row.level.is_empty() {
            let groove = row.level.inset(2.0);
            fill_rect_rounded(scene, groove, 2.0, p.panel);
            let at = crate::canvas::send_x_of_level(groove, send.level_db);
            fill_rect_rounded(
                scene,
                Rect::new(groove.x, groove.y, (at - groove.x).max(0.0), groove.height),
                2.0,
                p.accent,
            );
            let caption = crate::canvas::format_send_db(send.level_db);
            if let Some(text) = labels_get(labels, &caption) {
                draw_text_clipped(
                    scene,
                    text,
                    groove,
                    groove.right() - text.width - 2.0,
                    groove.y + (groove.height - text.height) / 2.0,
                    p.text,
                );
            }
        }
        row_text(
            scene,
            REMOVE,
            row.remove,
            row.remove.x + (row.remove.width - 6.0).max(0.0) / 2.0,
            if hovering(OptionsHit::SendRemove(row.index)) {
                p.text
            } else {
                p.text_muted
            },
        );
    }
    if !options.add_send.is_empty() {
        let lit = hovering(OptionsHit::AddSend);
        if lit {
            fill_rect_rounded(
                scene,
                options.add_send.inset(1.0),
                m.corner_radius * 0.5,
                p.border,
            );
        }
        row_text(
            scene,
            ADD_SEND,
            options.add_send,
            options.add_send.x + 4.0,
            if lit { p.text } else { p.text_muted },
        );
    }
}

/// The output row's menu. The same object the rack's route chip drops, over
/// the same list of names.
fn draw_output_menu(scene: &mut Scene, theme: &Theme, labels: &Labels, chrome: &MixerChrome<'_>) {
    // Only one is ever open — a press anywhere shuts whichever was.
    let Some(menu) = chrome.output_menu.or(chrome.send_menu) else {
        return;
    };
    if menu.frame.is_empty() {
        return;
    }
    let p = &theme.palette;
    let m = &theme.metrics;
    fill_rect_rounded(scene, menu.frame, m.corner_radius, p.border);
    fill_rect_rounded(
        scene,
        menu.frame.inset(1.0),
        m.corner_radius,
        p.panel_header,
    );

    // Where the *output* already goes, so the menu says where you are as well
    // as where you could go. A send menu is picking a new destination and has
    // no current row to mark.
    let current = chrome.output_menu.map(|_| match chrome.output {
        Some(track) => crate::canvas::RouteChoice::Track(track),
        None => crate::canvas::RouteChoice::Master,
    });
    for (choice, rect) in &menu.items {
        if rect.is_empty() {
            continue;
        }
        let on = current == Some(*choice);
        if on {
            fill_rect_rounded(scene, *rect, m.corner_radius, p.accent);
        }
        let caption = menu.label(*choice, chrome.route_names);
        let Some(text) = labels.get(&caption) else {
            continue;
        };
        draw_text_clipped(
            scene,
            text,
            *rect,
            rect.x + m.panel_padding.min(rect.width),
            rect.y + (rect.height - text.height) / 2.0,
            if on {
                p.panel
            } else if *choice == crate::canvas::RouteChoice::New {
                p.text_muted
            } else {
                p.text
            },
        );
    }
}

fn draw_mixer_strip(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    chrome: &MixerChrome<'_>,
    layout: &crate::canvas::MixerStripLayout,
    strip: &crate::document::MixerStrip,
    peaks: [f32; 2],
) {
    use crate::canvas::{MixerHit, fader_y_of_db, format_gain_db, format_pan, pan_x_of};

    let p = &theme.palette;
    let m = &theme.metrics;
    let hovering = |what: MixerHit| chrome.hover == Some(what);

    fill_rect_rounded(scene, layout.frame, m.corner_radius, p.panel);

    // The selected strip, ringed. Without it the track-options column beside
    // the master is a panel of controls with nothing saying what they are
    // about — and the report that started this was *"i am not able to click on
    // any of these to select them"*, which is as much about there being no
    // visible answer as about the press.
    if chrome.selected == layout.index {
        stroke_rect_rounded(
            scene,
            layout.frame.inset(0.5),
            m.corner_radius,
            m.border_width.max(1.0) * 2.0,
            p.accent,
        );
    }

    // The track's own colour as a cap along the top, which is what makes a
    // strip findable at a glance on a project with twenty of them.
    let cap = Rect::new(layout.frame.x, layout.frame.y, layout.frame.width, 3.0);
    fill_rect(
        scene,
        cap,
        if strip.is_master {
            p.accent
        } else {
            Color(strip.color)
        },
    );

    // **The chain read-out**: one dot per insert, in chain order, dimmed where
    // one is switched out. No names — the track-options column draws those in
    // full for whichever track is selected, and drawing them twice is what
    // took the height off the fader.
    //
    // What a dot says that the column cannot: this is every strip at once, so
    // "which of these has anything on it, and is anything switched out" is
    // answerable without clicking through sixteen tracks.
    if !layout.chain.is_empty() && !strip.inserts.is_empty() {
        use crate::canvas::{CHAIN_DOT, CHAIN_DOT_GAP};
        let row = layout.chain;
        let pitch = CHAIN_DOT + CHAIN_DOT_GAP;
        // As many as fit, and then one half-height mark to say there are more
        // — a count that ran off the end of the strip would say nothing at all.
        let room = ((row.width + CHAIN_DOT_GAP) / pitch).floor().max(0.0) as usize;
        let shown = strip.inserts.len().min(room);
        let overflowing = strip.inserts.len() > shown;
        let size = CHAIN_DOT.min(row.height);
        let y = row.y + (row.height - size) / 2.0;
        for (slot, insert) in strip.inserts.iter().take(shown).enumerate() {
            let dot = Rect::new(row.x + slot as f32 * pitch, y, size, size);
            // The last one becomes the "and more" mark rather than being drawn
            // as an effect it might not be.
            let more = overflowing && slot + 1 == shown;
            fill_rect_rounded(
                scene,
                if more {
                    Rect::new(dot.x, dot.y + size / 3.0, size, (size / 3.0).max(1.0))
                } else {
                    dot
                },
                size / 2.0,
                if more || insert.bypassed {
                    p.text_muted
                } else {
                    Color(strip.color)
                },
            );
        }
    }

    let renaming = chrome.renaming == Some(layout.index);
    if let Some(text) = labels_get(labels, &strip.name) {
        draw_text_clipped(
            scene,
            text,
            layout.name,
            layout.name.x + 2.0,
            layout.name.y + (layout.name.height - text.height) / 2.0,
            if renaming || hovering(MixerHit::Name(layout.index)) {
                p.text
            } else {
                p.text_muted
            },
        );
    }
    // The same caret the rack's rows and the arrangement's lanes get, so "this
    // is a field you are typing in" looks the same everywhere — and outside
    // the name's own `if let`, so a name backspaced to nothing still has one.
    if renaming {
        let width = labels_get(labels, &strip.name).map_or(0.0, |text| text.width);
        draw_rename_marks(
            scene,
            theme,
            layout.name,
            layout.name.x + 3.0,
            width,
            chrome.rename,
        );
    }

    // --- the pan: a groove, and the distance it has been moved off centre.
    if !layout.pan.is_empty() {
        let groove = Rect::new(
            layout.pan.x,
            layout.pan.y + layout.pan.height / 2.0 - 1.5,
            layout.pan.width,
            3.0,
        );
        // The groove takes the border ink rather than the window's: on this
        // theme the window colour and the panel's are four steps apart, so a
        // control at rest was a track nobody could see there was anything to
        // grab. Found by looking at the frame, not by thinking about it.
        fill_rect(scene, groove, p.border);
        let centre = layout.pan.x + layout.pan.width / 2.0;
        let at = pan_x_of(layout.pan, strip.pan);
        fill_rect(
            scene,
            Rect::new(centre.min(at), groove.y, (at - centre).abs(), groove.height),
            p.accent,
        );
        fill_rect(
            scene,
            Rect::new(at - 2.0, layout.pan.y, 4.0, layout.pan.height),
            if hovering(MixerHit::Pan(layout.index)) {
                p.text
            } else {
                p.text_muted
            },
        );
    }

    // --- the fader: the travel, how much of it is in use, and the handle.
    if !layout.fader.is_empty() {
        let groove = Rect::new(
            layout.fader.x + layout.fader.width / 2.0 - 2.0,
            layout.fader.y,
            4.0,
            layout.fader.height,
        );
        fill_rect(scene, groove, p.border);
        // Unity, marked on the groove — the fader's own zero, and the thing
        // the detent snaps back to.
        let unity = fader_y_of_db(layout.fader, 0.0);
        fill_rect(
            scene,
            Rect::new(layout.fader.x, unity - 0.5, layout.fader.width, 1.0),
            p.grid_line,
        );
        let top = layout.handle.y + layout.handle.height / 2.0;
        fill_rect(
            scene,
            Rect::new(
                groove.x,
                top,
                groove.width,
                (groove.bottom() - top).max(0.0),
            ),
            if strip.mute { p.text_muted } else { p.accent },
        );
        fill_rect_rounded(
            scene,
            layout.handle,
            2.0,
            if hovering(MixerHit::Fader(layout.index)) {
                p.text
            } else {
                p.text_muted
            },
        );
    }

    // --- the meter, one bar per channel.
    if !layout.meter.is_empty() {
        fill_rect(scene, layout.meter, p.border);
        let bar = ((layout.meter.width - 1.0) / 2.0).max(0.0);
        for (channel, peak) in peaks.iter().enumerate() {
            let fill = meter_fill(linear_db(*peak));
            let height = layout.meter.height * fill;
            fill_rect(
                scene,
                Rect::new(
                    layout.meter.x + channel as f32 * (bar + 1.0),
                    layout.meter.bottom() - height,
                    bar,
                    height,
                ),
                if *peak >= 1.0 { p.meter_peak } else { p.meter },
            );
        }
    }

    // --- the two switches.
    for (rect, caption, on, what) in [
        (layout.mute, "M", strip.mute, MixerHit::Mute(layout.index)),
        (layout.solo, "S", strip.solo, MixerHit::Solo(layout.index)),
    ] {
        if rect.is_empty() {
            continue;
        }
        fill_rect_rounded(
            scene,
            rect,
            m.corner_radius,
            if on {
                p.accent
            } else if hovering(what) {
                p.border
            } else {
                p.window
            },
        );
        if let Some(text) = labels_get(labels, caption) {
            draw_text_clipped(
                scene,
                text,
                rect,
                rect.x + ((rect.width - text.width) / 2.0).max(1.0),
                rect.y + (rect.height - text.height) / 2.0,
                if on { p.panel } else { p.text_muted },
            );
        }
    }

    // --- the read-out. The gain, except while the pan is the thing moving:
    // a control being dragged has to say what it is doing, and the pan groove
    // is too short to write a number in.
    let reading = if chrome.active == Some(MixerHit::Pan(layout.index)) {
        format_pan(strip.pan)
    } else {
        format_gain_db(strip.gain_db)
    };
    if let Some(text) = labels_get(labels, &reading)
        && !layout.value.is_empty()
    {
        draw_text_clipped(
            scene,
            text,
            layout.value,
            layout.value.x + ((layout.value.width - text.width) / 2.0).max(1.0),
            layout.value.y + (layout.value.height - text.height) / 2.0,
            p.text,
        );
    }
}

/// A linear peak as decibels, floored where the meters' scale is.
fn linear_db(linear: f32) -> f32 {
    if linear <= 0.0 {
        return crate::transport::METER_FLOOR_DB;
    }
    (20.0 * linear.log10()).max(crate::transport::METER_FLOOR_DB)
}

/// The song end to end, with the loop range shaded and the playhead on top.
fn draw_ruler(scene: &mut Scene, theme: &Theme, ruler: Rect, view: &TransportView, marker: i64) {
    if ruler.is_empty() {
        return;
    }
    let p = &theme.palette;
    // A groove rather than the bar's own colour, so the playhead has something
    // to travel along even at position zero.
    let track = ruler.inset(ruler.height * 0.3);
    fill_rect(scene, track, p.grid_line);

    if view.looping && view.length_samples > 0 {
        let (from, to) = view.loop_range_samples;
        let x0 = playhead_x(track, from, view.length_samples);
        let x1 = playhead_x(track, to, view.length_samples);
        fill_rect(
            scene,
            Rect::new(x0, track.y, (x1 - x0).max(0.0), track.height),
            p.selection,
        );
    }

    if !view.available {
        return;
    }

    // The time marker, under the playhead: where play will start from, and
    // where a stop comes back to. Drawn as a flag rather than a line so it is
    // not mistaken for the playhead when the two are in the same place —
    // which, the moment after a stop, they always are.
    if view.length_samples > 0 {
        let mx = playhead_x(track, marker, view.length_samples);
        fill_rect(scene, Rect::new(mx, ruler.y, 1.0, ruler.height), p.accent);
        fill_rect(
            scene,
            Rect::new(mx, ruler.y, MARKER_FLAG_WIDTH, MARKER_FLAG_HEIGHT).intersection(&ruler),
            p.accent,
        );
    }

    // Two logical pixels wide and drawn over everything: the playhead is the
    // one thing in the bar you look for rather than at.
    let x = playhead_x(track, view.position_sample, view.length_samples);
    fill_rect(
        scene,
        Rect::new(x - 1.0, ruler.y, 2.0, ruler.height),
        p.playhead,
    );
}

/// The little flag on the time marker's stem.
const MARKER_FLAG_WIDTH: f32 = 6.0;
const MARKER_FLAG_HEIGHT: f32 = 5.0;

/// One horizontal bar per channel, with the peak-hold marker over it.
fn draw_meter(
    scene: &mut Scene,
    theme: &Theme,
    meter: Rect,
    view: &TransportView,
    meters: &[Meter; 2],
) {
    if meter.is_empty() {
        return;
    }
    let p = &theme.palette;
    let box_ = meter.inset(meter.height * 0.25);
    fill_rect(scene, box_, p.grid_line);
    if !view.available || box_.is_empty() {
        return;
    }

    let gap = 1.0;
    let lane = ((box_.height - gap) / 2.0).max(0.0);
    for (index, channel) in meters.iter().enumerate() {
        let y = box_.y + index as f32 * (lane + gap);
        let fill = meter_fill(channel.level_db);
        // Green until it is nearly there, then red. The limiter means a peak
        // is not a disaster, but it is still the thing worth seeing.
        let colour = if channel.level_db >= -3.0 {
            p.meter_peak
        } else {
            p.meter
        };
        fill_rect(scene, Rect::new(box_.x, y, box_.width * fill, lane), colour);

        let hold = meter_fill(channel.hold_db);
        if hold > 0.0 {
            fill_rect(
                scene,
                Rect::new(box_.x + box_.width * hold - 1.0, y, 1.0, lane),
                p.meter_peak,
            );
        }
    }
}

/// The piano roll (TDD §16.4).
///
/// Four layers, drawn back to front: the row shading, the grid lines, the
/// notes, the playhead. §16.4 wants those on independent invalidation so a
/// moving playhead does not redirty note geometry; today they share a frame and
/// the split lives in `WidgetTree`'s bounds, which is where it will be applied
/// when the roll is big enough for it to pay.
///
/// **Only the visible window is built.** `visible_ticks` and `visible_keys`
/// bound every loop here. The note scan is still linear in the clip's note
/// count — filtering, not indexing — which is honest for the sizes this opens
/// today and is the first thing to change when it is not.
pub fn draw_piano_roll(scene: &mut Scene, theme: &Theme, labels: &Labels, chrome: &RollChrome<'_>) {
    let p = &theme.palette;
    let l = &chrome.layout;
    let v = &chrome.view;
    let grid = l.grid;

    if grid.is_empty() {
        return;
    }
    fill_rect(scene, grid, p.panel);

    let keys = visible_keys(v, grid);
    let ticks = visible_ticks(v, grid);

    // Row shading: the accidentals sit a shade back, which is what makes an
    // octave countable without a line for every one of them.
    for key in keys.clone() {
        // Snapped to whole pixels, and the same rectangle the keyboard and the
        // notes are drawn in — see `canvas::key_row`. Measuring the three in
        // three places is how they came to disagree by a pixel, which is what
        // "inconsistant sizing on the notes" looked like.
        let row = crate::canvas::key_row(v, grid, key.clamp(0, 127) as u8);
        // A key the instrument cannot play, before the accidental shading and
        // instead of it: "nothing here sounds" is a stronger statement about a
        // row than "this one is a black key", and on a drum kit the dead rows
        // fall on naturals and accidentals alike.
        if !chrome.key_map.plays(key.clamp(0, 127) as u8) {
            fill_rect(scene, row.intersection(&grid), p.row_dead);
        } else if is_accidental(key) && chrome.key_style == crate::canvas::KeyStyle::Piano {
            // Only in the piano view: the list view has no black keys to
            // shade rows for, and striping them there would be a pattern
            // saying something the strip beside it does not.
            fill_rect(scene, row.intersection(&grid), p.row_accidental);
        }
        // A stronger line under every C.
        if key % 12 == 0 {
            fill_rect(
                scene,
                Rect::new(grid.x, row.bottom() - 1.0, grid.width, 1.0).intersection(&grid),
                p.grid_line_strong,
            );
        }
    }

    // Past the clip's end, before the lines so the grid still reads through
    // it: nothing written out here sounds. See `canvas::roll_past_end`.
    if let Some(length) = chrome.clip_length
        && let Some(rect) = crate::canvas::roll_past_end(v, grid, length)
    {
        fill_rect(scene, rect, p.row_dead);
    }

    // Vertical lines, in **three** levels, drawn faintest first so the
    // stronger one wins wherever they land on the same tick.
    //
    // Both halves of "it's hard to tell the time" are here. The levels used to
    // be two — subdivisions and beats shared one ink, so the grid was a comb
    // with nothing to count by — and the finest level used to be the *snap*,
    // so setting the snap to bars emptied the bar. The grid is the ruler you
    // read the time off; the snap is where a note may land. See
    // [`subdivision_unit`](crate::canvas::subdivision_unit).
    let bar = PPQN * Tick::from(chrome.beats_per_bar.max(1));
    let sub = crate::canvas::subdivision_unit(v.snap, chrome.beats_per_bar);
    for (unit, colour) in [
        (sub, p.grid_line_sub),
        (PPQN, p.grid_line),
        (bar, p.grid_line_strong),
    ] {
        if unit <= 0 || (unit as f32) * v.pixels_per_tick < 4.0 {
            // Lines closer together than a few pixels are a grey wash, not a
            // grid.
            continue;
        }
        let mut tick = ticks.start - ticks.start.rem_euclid(unit);
        while tick < ticks.end {
            let x = tick_to_x(v, grid, tick).floor();
            fill_rect(
                scene,
                Rect::new(x, grid.y, 1.0, grid.height).intersection(&grid),
                colour,
            );
            tick += unit;
        }
    }

    // The onion skin: other instruments' notes, behind this one's. Faint and
    // outlined, so a ghost under a real note never reads as the real note —
    // which is the one thing an onion skin must not do.
    for ghost in chrome.ghosts {
        let key = i32::from(ghost.key);
        if !keys.contains(&key) {
            continue;
        }
        if ghost.start + ghost.length < ticks.start || ghost.start > ticks.end {
            continue;
        }
        let x0 = tick_to_x(v, grid, ghost.start);
        let x1 = tick_to_x(v, grid, ghost.start + ghost.length);
        let row = crate::canvas::key_row(v, grid, ghost.key);
        let block = Rect::new(x0, row.y, (x1 - x0).max(1.0), row.height).intersection(&grid);
        if block.is_empty() {
            continue;
        }
        let mut colour = ghost.color;
        colour[3] = GHOST_ALPHA;
        let outline = rounded(block.inset(1.0), 2.0);
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            Color(colour).to_peniko(),
            None,
            &outline,
        );
        colour[3] = GHOST_EDGE_ALPHA;
        scene.stroke(
            &Stroke::new(1.0),
            Affine::IDENTITY,
            Color(colour).to_peniko(),
            None,
            &outline,
        );
    }

    // Notes.
    for (id, note) in chrome.notes.iter() {
        let key = i32::from(note.key);
        if !keys.contains(&key) {
            continue;
        }
        if note.start + note.length < ticks.start || note.start > ticks.end {
            continue;
        }
        let x0 = tick_to_x(v, grid, note.start);
        let x1 = tick_to_x(v, grid, note.start + note.length);
        let row = crate::canvas::key_row(v, grid, note.key);
        let block = Rect::new(
            x0,
            row.y,
            // Always at least a pixel wide: a note too short to see is still a
            // note, and one that vanishes at low zoom cannot be clicked to
            // find out why.
            (x1 - x0).max(1.0),
            row.height,
        )
        .intersection(&grid);
        if block.is_empty() {
            continue;
        }
        let selected = chrome.selection.contains(&id);
        // A note on a key the instrument cannot play is still a note — it can
        // be selected, moved and heard the moment the instrument changes — so
        // it is drawn quietly rather than not at all. Selection wins: what you
        // have hold of has to be visible whether or not it sounds.
        let fill = if selected {
            p.note_selected
        } else if chrome.key_map.plays(note.key) {
            p.note
        } else {
            p.note_silent
        };
        if note.slide {
            // A **slide note** is drawn as the ramp it is: a wedge rising from
            // the bottom of its lane to the top at the pitch it lands on. It
            // has to look different from a note, because it is not one — it
            // starts no voice, it bends whatever is sounding. FL draws the
            // same shape for the same reason.
            let mut wedge = BezPath::new();
            wedge.move_to(Point::new(block.x as f64, block.bottom() as f64 - 0.5));
            wedge.line_to(Point::new(block.right() as f64, block.y as f64 + 0.5));
            wedge.line_to(Point::new(
                block.right() as f64,
                block.bottom() as f64 - 0.5,
            ));
            wedge.close_path();
            scene.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                fill.to_peniko(),
                None,
                &wedge,
            );
            continue;
        }
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            fill.to_peniko(),
            None,
            &rounded(block.inset(0.5), 2.0),
        );
    }

    // The take being recorded, over the notes that are already there and in
    // the record colour, so that what is landing can be told from what has
    // landed — *"so you can be sure it is indeed recording it."* A held key
    // arrives here drawn out to the playhead, and grows with it.
    for take in chrome.recording {
        let key = i32::from(take.key);
        if !keys.contains(&key) {
            continue;
        }
        if take.start + take.length < ticks.start || take.start > ticks.end {
            continue;
        }
        let x0 = tick_to_x(v, grid, take.start);
        let x1 = tick_to_x(v, grid, take.start + take.length);
        let row = crate::canvas::key_row(v, grid, take.key);
        // Never nothing: the instant a key goes down its note is a sliver
        // rather than absent, the rule the arrangement's take band follows.
        let block = Rect::new(x0, row.y, (x1 - x0).max(2.0), row.height).intersection(&grid);
        if block.is_empty() {
            continue;
        }
        let shape = rounded(block.inset(1.0), 2.0);
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            p.meter_peak.to_peniko(),
            None,
            &shape,
        );
        scene.stroke(
            &Stroke::new(1.0),
            Affine::IDENTITY,
            p.accent.to_peniko(),
            None,
            &shape,
        );
    }

    // The time marker first, so the playhead sitting on it is what you see.
    if let Some(tick) = chrome.marker_tick {
        let x = tick_to_x(v, grid, tick);
        if x >= grid.x && x <= grid.right() {
            fill_rect(scene, Rect::new(x, grid.y, 1.0, grid.height), p.accent);
        }
    }

    if let Some(tick) = chrome.playhead_tick {
        let x = tick_to_x(v, grid, tick);
        if x >= grid.x && x <= grid.right() {
            fill_rect(
                scene,
                Rect::new(x - 1.0, grid.y, 2.0, grid.height),
                p.playhead,
            );
        }
    }

    // The selection box, over the notes and under nothing.
    if let Some(box_) = chrome.marquee {
        let box_ = box_.intersection(&grid);
        if !box_.is_empty() {
            fill_rect(scene, box_, p.selection);
            scene.stroke(
                &Stroke::new(1.0),
                Affine::IDENTITY,
                p.accent.to_peniko(),
                None,
                &KRect::new(
                    box_.x as f64,
                    box_.y as f64,
                    box_.right() as f64,
                    box_.bottom() as f64,
                ),
            );
        }
    }

    // The cut tool, over the notes it is about to cut and under the chrome.
    // **Two marks, and they say two different things** — the arrangement's
    // rule, and now the roll's:
    //
    // > *"it would be nice when im cutting in the piano roll it drew the
    // > line that cut the notes i was cutting with the cut tool to visualize
    // > it cleanly."*
    //
    // The stroke is the gesture — where the hand went — drawn faint, because
    // it is feedback that the drag is happening and nothing more. The bright
    // marks are the *cuts*: one across each note the stroke crosses, at the
    // tick `slice_cuts` will divide it on. The roll drew the stroke alone,
    // which on a diagonal through a chord left the reader working out which
    // notes were in it and where each would part.
    if let Some((from, to)) = chrome.slice {
        let mut line = BezPath::new();
        line.move_to(Point::new(from.0 as f64, from.1 as f64));
        line.line_to(Point::new(to.0 as f64, to.1 as f64));
        scene.push_layer(
            Fill::NonZero,
            BlendMode::default(),
            1.0,
            Affine::IDENTITY,
            &KRect::new(
                grid.x as f64,
                grid.y as f64,
                grid.right() as f64,
                grid.bottom() as f64,
            ),
        );
        scene.stroke(
            &Stroke::new(1.0),
            Affine::IDENTITY,
            p.meter_peak.with_alpha(0x55).to_peniko(),
            None,
            &line,
        );
        for mark in crate::canvas::note_marks(v, grid, chrome.notes, from, to) {
            fill_rect(scene, mark, p.meter_peak);
            // A cap at each end, so a mark on a note whose colour is close to
            // the ink still reads as a cut rather than as a stripe.
            for y in [mark.y, mark.bottom() - 2.0] {
                fill_rect(
                    scene,
                    Rect::new(mark.x - 2.0, y, mark.width + 4.0, 2.0),
                    p.meter_peak,
                );
            }
        }
        scene.pop_layer();
    }

    draw_keyboard(
        scene,
        theme,
        labels,
        l,
        v,
        chrome.key_map,
        chrome.live_keys,
        chrome.key_style,
    );
    draw_ruler_strip(scene, theme, labels, l, v, chrome);
    draw_property_lane(scene, theme, labels, chrome);
    draw_roll_toolbar(scene, theme, labels, chrome);
    draw_lane_menu(scene, theme, labels, chrome);
    draw_tools_panel(scene, theme, labels, chrome);
    // Last, over the roll's own chrome, so the mark is not drawn under the
    // keyboard or the ruler — the same place the arrangement's is drawn.
    draw_focus_edge(scene, theme, chrome.layout.frame, chrome.focused);
}

/// One tool's dialog. **Last, over everything**, for the reason the lane menu
/// is: it is not part of the layout it covers.
fn draw_tools_panel(scene: &mut Scene, theme: &Theme, labels: &Labels, chrome: &RollChrome<'_>) {
    let Some(panel) = chrome.tools_panel else {
        return;
    };
    if panel.frame.is_empty() {
        return;
    }
    let p = &theme.palette;
    let m = &theme.metrics;
    fill_rect_rounded(scene, panel.frame, m.corner_radius, p.border);
    fill_rect_rounded(
        scene,
        panel.frame.inset(1.0),
        m.corner_radius,
        p.panel_header,
    );

    // The title, which is what says whose settings these are — and the reason
    // a row inside can be called "Semitones" rather than "Transpose by".
    if !panel.title.is_empty()
        && let Some(text) = labels.get(panel.kind.title())
    {
        draw_text_clipped(
            scene,
            text,
            panel.title,
            panel.title.x + m.panel_padding.min(panel.title.width),
            panel.title.y + (panel.title.height - text.height) / 2.0,
            p.text_muted,
        );
    }

    for (row, rect) in &panel.rows {
        if rect.is_empty() {
            continue;
        }
        let action = chrome.tools.action(*row).is_some();
        // An action row carries a frame so it reads as a button rather than
        // as another read-out — the same reasoning that put a frame on the
        // toolbar's chips.
        if action {
            fill_rect_rounded(scene, rect.inset(1.0), m.corner_radius, p.panel);
        }
        let ink = p.text;
        let inset = m.panel_padding.min(rect.width);
        if let Some(text) = labels.get(&chrome.tools.label(*row)) {
            draw_text_clipped(
                scene,
                text,
                *rect,
                rect.x + inset,
                rect.y + (rect.height - text.height) / 2.0,
                ink,
            );
        }
        // The value is right-aligned against the row's far edge, so a column
        // of them reads down the panel rather than wandering with the names.
        let value = chrome.tools.value(*row);
        if value.is_empty() {
            continue;
        }
        if let Some(text) = labels.get(&value) {
            draw_text_clipped(
                scene,
                text,
                *rect,
                rect.right() - inset - text.width,
                rect.y + (rect.height - text.height) / 2.0,
                p.accent,
            );
        }
    }
}

/// The lane chip's drop-down. **Last, over everything**, which is what a menu
/// is: it is not part of the layout it covers.
fn draw_lane_menu(scene: &mut Scene, theme: &Theme, labels: &Labels, chrome: &RollChrome<'_>) {
    let Some(menu) = chrome.lane_menu else { return };
    if menu.frame.is_empty() {
        return;
    }
    let p = &theme.palette;
    let m = &theme.metrics;
    // A shadow would be better and needs a blur; a border against the panel's
    // own ground is enough to read as "in front of", which is the job.
    fill_rect_rounded(scene, menu.frame, m.corner_radius, p.border);
    fill_rect_rounded(
        scene,
        menu.frame.inset(1.0),
        m.corner_radius,
        p.panel_header,
    );

    for (property, rect) in &menu.items {
        if rect.is_empty() {
            continue;
        }
        let on = *property == chrome.lane_property;
        if on {
            fill_rect_rounded(scene, *rect, m.corner_radius, p.accent);
        }
        let Some(text) = labels.get(property.label()) else {
            continue;
        };
        draw_text_clipped(
            scene,
            text,
            *rect,
            rect.x + m.panel_padding.min(rect.width),
            rect.y + (rect.height - text.height) / 2.0,
            if on { p.panel } else { p.text },
        );
    }
}

/// How solid the caption on a repeat is, against the first one.
///
/// Faint enough that the first pass still reads as the head of the clip,
/// strong enough that the repeats are legibly the same thing again.
const REPEAT_CAPTION_ALPHA: u8 = 0x80;

/// How solid a ghosted note is, and its edge.
///
/// Faint enough that the grid still reads through it, strong enough to see the
/// shape of a chord under a melody.
const GHOST_ALPHA: u8 = 0x33;
const GHOST_EDGE_ALPHA: u8 = 0x88;

/// The property lane under the grid (§16.5's note property lanes).
///
/// One bar per note, at the note's own x, so a phrase's dynamics — or its
/// panning, or its tuning — are read straight down from the notes that make
/// them. Which property is showing is the toolbar's chip; the lane itself only
/// has to know that a *bipolar* property grows from the middle of the strip and
/// a quantity grows from the floor.
fn draw_property_lane(scene: &mut Scene, theme: &Theme, labels: &Labels, chrome: &RollChrome<'_>) {
    let p = &theme.palette;
    let lane = chrome.layout.velocity;
    if lane.is_empty() {
        return;
    }
    let v = &chrome.view;
    let property = chrome.lane_property;
    fill_rect(scene, chrome.layout.velocity_keys, p.panel_header);
    fill_rect(scene, lane, p.row_accidental);
    // A line at the top of the lane, so it reads as its own strip rather than
    // as more grid, and a grip in the middle of it so it is visibly the thing
    // you drag to make the lane taller.
    fill_rect(
        scene,
        Rect::new(
            chrome.layout.velocity_keys.x,
            lane.y,
            chrome.layout.frame.width,
            1.0,
        ),
        p.border,
    );
    let grip = chrome.layout.lane_grip;
    if !grip.is_empty() {
        let width = (grip.width * 0.08).clamp(16.0, 60.0);
        fill_rect_rounded(
            scene,
            Rect::new(
                grip.x + (grip.width - width) / 2.0,
                grip.y + (grip.height - 2.0).max(0.0) / 2.0,
                width,
                2.0,
            ),
            1.0,
            p.text_muted,
        );
    }
    if let Some(text) = labels.get(property.label()) {
        draw_text_clipped(
            scene,
            text,
            chrome.layout.velocity_keys,
            chrome.layout.velocity_keys.x + 4.0,
            chrome.layout.velocity_keys.y + 4.0,
            p.text_muted,
        );
    }

    // The line a bar grows from — half-way for pan and tuning, the floor for a
    // quantity — so an eye can read a value's sign without counting pixels.
    let baseline = lane_baseline_y(property, lane);
    fill_rect(
        scene,
        Rect::new(lane.x, baseline.min(lane.bottom() - 1.0), lane.width, 1.0),
        p.grid_line,
    );

    let ticks = visible_ticks(v, chrome.layout.grid);
    for (id, note) in chrome.notes.iter() {
        if note.start + note.length < ticks.start || note.start > ticks.end {
            continue;
        }
        let x = tick_to_x(v, chrome.layout.grid, note.start);
        let value = lane_y_of_value(property, lane, property.of(note));
        // From the baseline to the value, whichever way round they are: a pan
        // of -40 is a bar hanging below the centre line, not an invisible one.
        let (top, height) = if value <= baseline {
            (value, baseline - value)
        } else {
            (baseline, value - baseline)
        };
        let bar = Rect::new(x, top, 3.0, height.max(2.0)).intersection(&lane);
        if bar.is_empty() {
            continue;
        }
        let selected = chrome.selection.contains(&id);
        fill_rect(scene, bar, if selected { p.note_selected } else { p.note });
    }
}

/// The toolbar: the tools, the snap chip, the zooms.
///
/// The answer to "I cannot see what this can do". Every one of them has a
/// keyboard shortcut too, and the shortcut is what a person ends up using —
/// but only after they have found out it exists.
fn draw_roll_toolbar(scene: &mut Scene, theme: &Theme, labels: &Labels, chrome: &RollChrome<'_>) {
    let p = &theme.palette;
    let m = &theme.metrics;
    if chrome.layout.toolbar.is_empty() {
        return;
    }
    fill_rect(scene, chrome.layout.toolbar, p.panel_header);

    let ghost_caption = chrome.ghost_filter.label();
    let lane_caption = crate::canvas::lane_caption(chrome.lane_property);
    let snap_caption = crate::canvas::snap_caption(chrome.snap);
    let tools_caption = crate::canvas::tools_caption();
    for (control, rect) in &chrome.toolbar.items {
        if rect.is_empty() {
            continue;
        }
        let on = match control {
            RollControl::Tool(tool) => *tool == chrome.tool,
            RollControl::Velocity => !chrome.layout.velocity.is_empty(),
            RollControl::Ghost => chrome.ghost_filter != GhostFilter::Off,
            RollControl::Tools => chrome.tools_panel.is_some(),
            _ => false,
        };
        // The three read-out chips always carry their frame, whether or not
        // anything is hovering them. Reported as *"I don't see snap controls
        // right now"* — the snap chip was there the whole time, sitting in the
        // toolbar as the bare word "1/16" between "Del" and "-", which reads
        // as a label rather than as something to press. This is the same
        // reasoning that put a caret on the lane chip.
        let is_chip = matches!(
            control,
            RollControl::Snap
                | RollControl::Lane
                | RollControl::Ghost
                | RollControl::Keys
                | RollControl::Tools
        );
        if on || is_chip || chrome.hover == Some(*control) {
            fill_rect_rounded(
                scene,
                *rect,
                m.corner_radius,
                if on {
                    p.accent
                } else if chrome.hover == Some(*control) {
                    p.border
                } else {
                    p.panel
                },
            );
        }
        let ink = if on { p.panel } else { p.text };
        // A verb draws its glyph; a read-out draws its value. See
        // `RollControl::icon` for why that is the whole rule.
        if let Some(icon) = control.icon() {
            draw_icon(scene, icon, rect.inset(rect.height * 0.24), ink);
            continue;
        }
        // The snap and lane chips say which division and which property are
        // live rather than their own names: the state is the useful half.
        let caption = match control {
            RollControl::Snap => &snap_caption,
            RollControl::Lane => &lane_caption,
            RollControl::Ghost => &ghost_caption,
            // The chip says which view the strip is in, the way the snap chip
            // says which division is on.
            RollControl::Keys => chrome.key_style.label(),
            RollControl::Tools => &tools_caption,
            other => other.label(),
        };
        let Some(text) = labels.get(caption) else {
            continue;
        };
        draw_text_clipped(
            scene,
            text,
            *rect,
            rect.x + ((rect.width - text.width) / 2.0).max(1.0),
            rect.y + (rect.height - text.height) / 2.0,
            ink,
        );
    }
}

// --------------------------------------------------------- the sidebar ---

/// A panel's ground, header strip and border — the three things every docked
/// panel has and none of them is worth writing twice.
fn draw_panel_frame(scene: &mut Scene, theme: &Theme, panel: &PanelLayout) {
    let p = &theme.palette;
    let m = &theme.metrics;
    if panel.frame.is_empty() {
        return;
    }
    let frame = rounded(panel.frame, m.corner_radius);
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        p.panel.to_peniko(),
        None,
        &frame,
    );
    if !panel.header.is_empty() {
        let h = panel.header;
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            p.panel_header.to_peniko(),
            None,
            &RoundedRect::from_rect(
                KRect::new(h.x as f64, h.y as f64, h.right() as f64, h.bottom() as f64),
                RoundedRectRadii::new(m.corner_radius as f64, m.corner_radius as f64, 0.0, 0.0),
            ),
        );
    }
    if m.border_width > 0.0 {
        scene.stroke(
            &Stroke::new(m.border_width as f64),
            Affine::IDENTITY,
            p.border.to_peniko(),
            None,
            &frame,
        );
    }
}

/// A panel header's own caption.
fn draw_label(
    scene: &mut Scene,
    labels: &Labels,
    caption: &str,
    header: Rect,
    m: &crate::theme::Metrics,
    ink: Color,
) {
    let Some(text) = labels.get(caption) else {
        return;
    };
    draw_text_clipped(
        scene,
        text,
        header,
        header.x + m.panel_padding,
        header.y + (header.height - text.height) / 2.0,
        ink,
    );
}

/// The strip that switches the left-hand panel between its two lists.
///
/// One function, called by both, because the two lists share the strip's
/// geometry (`canvas::tab_strip`) and a strip drawn two ways would be two
/// strips.
fn draw_rack_tabs(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    tabs: &[(crate::document::RackTab, Rect)],
    showing: crate::document::RackTab,
    hover: Option<crate::document::RackTab>,
) {
    let p = &theme.palette;
    for (tab, rect) in tabs {
        if rect.is_empty() {
            continue;
        }
        let on = *tab == showing;
        // The showing tab wears the panel's own ground so it reads as part of
        // the list under it; the other wears the header's, so the strip reads
        // as two tabs rather than as a heading.
        fill_rect(scene, *rect, if on { p.panel } else { p.panel_header });
        if !on && hover == Some(*tab) {
            fill_rect(scene, *rect, p.row_accidental);
        }
        if let Some(text) = labels.get(tab.label()) {
            draw_text_clipped(
                scene,
                text,
                *rect,
                rect.x + ((rect.width - text.width) / 2.0).max(0.0),
                rect.y + (rect.height - text.height) / 2.0,
                if on { p.text } else { p.text_muted },
            );
        }
        // A line under the one that is showing — the tab mark this window
        // already uses on the editor column's tabs.
        if on {
            fill_rect(
                scene,
                Rect::new(rect.x, rect.bottom() - 2.0, rect.width, 2.0),
                p.accent,
            );
        }
    }
}

/// The prefab list (TDD §10.5) — the panel's other tab.
fn draw_prefabs(scene: &mut Scene, theme: &Theme, labels: &Labels, chrome: &PrefabChrome<'_>) {
    use crate::canvas::PrefabHit;
    let p = &theme.palette;
    let l = &chrome.layout;

    draw_rack_tabs(
        scene,
        theme,
        labels,
        &l.tabs,
        crate::document::RackTab::Prefabs,
        match chrome.hover {
            Some(PrefabHit::Tab(tab)) => Some(tab),
            _ => None,
        },
    );

    let hovered = match chrome.hover {
        Some(PrefabHit::Row(index)) => Some(index),
        _ => None,
    };
    for row in &l.rows {
        let Some(prefab) = chrome.prefabs.get(row.index) else {
            continue;
        };
        if prefab.open {
            fill_rect(scene, row.frame, p.selection);
        } else if hovered == Some(row.index) {
            fill_rect(scene, row.frame, p.row_accidental);
        }
        if let Some(text) = labels.get(&prefab.name) {
            draw_text_clipped(
                scene,
                text,
                row.name,
                row.name.x + 6.0,
                row.name.y + (row.name.height - text.height) / 2.0,
                p.text,
            );
            if chrome.renaming == Some(row.index) {
                draw_rename_marks(
                    scene,
                    theme,
                    row.name,
                    row.name.x + 7.0,
                    text.width,
                    chrome.rename,
                );
            }
        }
        // How many places it is drawn in. In the muted ink and with no frame
        // around it, because it is a read-out and not a control — a boxed
        // number beside a boxed number on the rack's rows would read as a
        // second route chip.
        let caption = prefab_uses_label(prefab.uses);
        if let Some(text) = labels.get(&caption) {
            draw_text_clipped(
                scene,
                text,
                row.uses,
                row.uses.x + ((row.uses.width - text.width) / 2.0).max(0.0),
                row.uses.y + (row.uses.height - text.height) / 2.0,
                if prefab.uses == 0 {
                    p.text_muted
                } else {
                    p.text
                },
            );
        }
    }

    fill_rect_rounded(scene, l.add, theme.metrics.corner_radius, p.accent);
    if let Some(text) = labels.get(ADD_PREFAB) {
        draw_text_clipped(
            scene,
            text,
            l.add,
            l.add.x + ((l.add.width - text.width) / 2.0).max(0.0),
            l.add.y + (l.add.height - text.height) / 2.0,
            p.panel,
        );
    }
}

/// What a prefab row's count says. In one place so the window shapes exactly
/// the string the renderer will look for.
///
/// An em dash rather than "0" for a prefab drawn nowhere: a zero in a column
/// of numbers reads as a measurement, and "not used yet" is an absence.
pub fn prefab_uses_label(uses: usize) -> String {
    if uses == 0 {
        "\u{2014}".to_string()
    } else {
        uses.to_string()
    }
}

/// The caption on the prefab list's add button, in one place so the window can
/// shape exactly the string the renderer will look for.
pub const ADD_PREFAB: &str = "+ Make prefab";

fn draw_rack(scene: &mut Scene, theme: &Theme, labels: &Labels, chrome: &RackChrome<'_>) {
    let p = &theme.palette;
    let l = &chrome.layout;

    draw_rack_tabs(
        scene,
        theme,
        labels,
        &l.tabs,
        crate::document::RackTab::Instruments,
        match chrome.hover {
            Some(RackHit::Tab(tab)) => Some(tab),
            _ => None,
        },
    );

    let hovered = match chrome.hover {
        Some(RackHit::Row(index) | RackHit::Mute(index) | RackHit::Solo(index)) => Some(index),
        _ => None,
    };
    for row in &l.rows {
        let Some(channel) = chrome.channels.get(row.index) else {
            continue;
        };
        if row.index == chrome.selected {
            fill_rect(scene, row.frame, p.selection);
        } else if hovered == Some(row.index) {
            // Fainter than the selection: "you are pointing at this" and "this
            // is the one open in the roll" must not look the same.
            fill_rect(scene, row.frame, p.row_accidental);
        }
        // A channel with no soundfont on it is drawn in the muted ink: a
        // channel that cannot make a sound and looks like one that can is the
        // most confusing thing a rack can do.
        let ink = if channel.has_instrument {
            p.text
        } else {
            p.text_muted
        };
        if let Some(text) = labels.get(&channel.name) {
            draw_text_clipped(
                scene,
                text,
                row.name,
                row.name.x + 6.0,
                row.name.y + (row.name.height - text.height) / 2.0,
                if channel.muted { p.text_muted } else { ink },
            );
            if chrome.renaming == Some(row.index) {
                draw_rename_marks(
                    scene,
                    theme,
                    row.name,
                    row.name.x + 7.0,
                    text.width,
                    chrome.rename,
                );
            }
        }
        for (rect, on, caption, colour) in [
            (row.solo, channel.soloed, "S", p.accent),
            (row.mute, channel.muted, "M", p.meter_peak),
        ] {
            fill_rect_rounded(scene, rect, 2.0, if on { colour } else { p.border });
            if let Some(text) = labels.get(caption) {
                draw_text_clipped(
                    scene,
                    text,
                    rect,
                    rect.x + ((rect.width - text.width) / 2.0).max(0.0),
                    rect.y + (rect.height - text.height) / 2.0,
                    if on { p.panel } else { p.text_muted },
                );
            }
        }

        // The button that opens this channel's instrument in a window of its
        // own (TDD §7.2, §7.5).
        //
        // **It was hit-tested and never drawn.** A rectangle you cannot see is
        // not a button, and this one is now the only way to open an instrument
        // at all — the tab it used to switch to is gone. Found the way the
        // other four of its kind were: by opening the window and looking at
        // the row. Drawn as the glyph and not a letter, because "E" beside "S"
        // and "M" reads as a third switch.
        if !row.edit.is_empty() {
            let lit = chrome.hover == Some(RackHit::Edit(row.index));
            fill_rect_rounded(scene, row.edit, 2.0, if lit { p.accent } else { p.border });
            let side = (row.edit.height * 0.62).min(row.edit.width * 0.62);
            draw_icon(
                scene,
                crate::icon::Icon::Sliders,
                Rect::new(
                    row.edit.x + (row.edit.width - side) / 2.0,
                    row.edit.y + (row.edit.height - side) / 2.0,
                    side,
                    side,
                ),
                if lit { p.panel } else { p.text_muted },
            );
        }

        // Where the channel goes, as the mixer's own number. Framed like the
        // switches so it reads as pressable — the lesson the snap chip and the
        // lane chip both taught, twice.
        let caption = crate::canvas::route_label(channel.route, chrome.strips);
        let lit = chrome.hover == Some(RackHit::Route(row.index))
            || chrome.route_menu_open == Some(row.index);
        fill_rect_rounded(scene, row.route, 2.0, if lit { p.accent } else { p.border });
        if let Some(text) = labels.get(&caption) {
            draw_text_clipped(
                scene,
                text,
                row.route,
                row.route.x + ((row.route.width - text.width) / 2.0).max(0.0),
                row.route.y + (row.route.height - text.height) / 2.0,
                if lit { p.panel } else { p.text_muted },
            );
        }
    }

    // The add button, always at the bottom.
    fill_rect_rounded(scene, l.add, theme.metrics.corner_radius, p.accent);
    if let Some(text) = labels.get(ADD_CHANNEL) {
        draw_text_clipped(
            scene,
            text,
            l.add,
            l.add.x + ((l.add.width - text.width) / 2.0).max(0.0),
            l.add.y + (l.add.height - text.height) / 2.0,
            p.panel,
        );
    }

    draw_route_menu(scene, theme, labels, chrome);
}

/// The route chip's drop-down. **Last, over everything in the rack**, which is
/// what a menu is: it is not part of the layout it covers.
fn draw_route_menu(scene: &mut Scene, theme: &Theme, labels: &Labels, chrome: &RackChrome<'_>) {
    let Some(menu) = chrome.route_menu else {
        return;
    };
    if menu.frame.is_empty() {
        return;
    }
    let p = &theme.palette;
    let m = &theme.metrics;
    fill_rect_rounded(scene, menu.frame, m.corner_radius, p.border);
    fill_rect_rounded(
        scene,
        menu.frame.inset(1.0),
        m.corner_radius,
        p.panel_header,
    );

    // Which row the channel is already on, so the menu says where you are as
    // well as where you could go.
    let current = chrome
        .route_menu_open
        .and_then(|index| chrome.channels.get(index))
        .map(|channel| match channel.route {
            Some(track) if track + 1 < chrome.strips => crate::canvas::RouteChoice::Track(track),
            _ => crate::canvas::RouteChoice::Master,
        });

    for (choice, rect) in &menu.items {
        if rect.is_empty() {
            continue;
        }
        let on = current == Some(*choice);
        if on {
            fill_rect_rounded(scene, *rect, m.corner_radius, p.accent);
        }
        let caption = menu.label(*choice, chrome.route_names);
        let Some(text) = labels.get(&caption) else {
            continue;
        };
        draw_text_clipped(
            scene,
            text,
            *rect,
            rect.x + m.panel_padding.min(rect.width),
            rect.y + (rect.height - text.height) / 2.0,
            if on {
                p.panel
            } else if *choice == crate::canvas::RouteChoice::New {
                // The one row that is not a destination.
                p.text_muted
            } else {
                p.text
            },
        );
    }
}

/// The caption on the rack's add button, in one place so the window can shape
/// exactly the string the renderer will look for.
pub const ADD_CHANNEL: &str = "+ Add instrument";

/// The last row of the category chooser: makes one rather than choosing one.
///
/// A category is a folder (§P.3), so a person's categories are free-form and
/// "Save as…" is where a new one comes from — there is no other gesture in the
/// program that makes one, and there does not need to be.
pub const NEW_CATEGORY: &str = "New category\u{2026}";

/// The editor column's tab captions. Two, since the instrument, the effect
/// and the automation curve became windows of their own — see
/// [`draw_editor_window`].
pub const TAB_ROLL: &str = "Piano roll";
pub const TAB_MIXER: &str = "Mixer";

/// The caption on a strip's empty rack row. Short, because the row is the
/// width of a mixer strip and a longer word would be clipped to the same three
/// letters anyway.
pub const ADD_INSERT: &str = "+ fx";

/// What the empty column past the last strip says.
pub const ADD_TRACK: &str = "+";

/// The track-options column's own captions, in one place so the window shapes
/// exactly the strings the renderer looks for.
pub const EFFECTS_HEADING: &str = "Effects";
pub const ADD_EFFECT: &str = "+ Add effect";
pub const SENDS_HEADING: &str = "Sends";
pub const ADD_SEND: &str = "+ Add send";
/// Which side of the fader a send is taken from.
pub const SEND_PRE: &str = "pre";
pub const SEND_POST: &str = "post";
/// The handle a row is picked up by, and the cross that throws it away.
pub const GRIP: &str = "\u{2261}";
pub const REMOVE: &str = "\u{00d7}";

/// What the output row says, given where the track goes.
///
/// Built here rather than in the drawing code so the window can shape exactly
/// the string that will be looked for — `Labels` is keyed by the text itself.
pub fn output_label(name: &str) -> String {
    format!("Out \u{25b8} {name}")
}

/// What the growing block says while a take is being recorded, in one place so
/// the window can shape exactly the string the renderer will look for.
pub const RECORDING: &str = "Recording";

/// What the instrument tab says when the channel is playing nothing.
pub const NO_INSTRUMENT: &str =
    "this channel has no soundfont yet \u{2014} pick one from the browser";

fn draw_editor_tabs(scene: &mut Scene, theme: &Theme, chrome: &Chrome<'_>) {
    let p = &theme.palette;
    let m = &theme.metrics;
    for (which, rect, caption, icon) in [
        (
            EditorTab::Roll,
            chrome.tabs.roll,
            TAB_ROLL,
            crate::icon::Icon::Piano,
        ),
        (
            EditorTab::Mixer,
            chrome.tabs.mixer,
            TAB_MIXER,
            crate::icon::Icon::Sliders,
        ),
    ] {
        if rect.is_empty() {
            continue;
        }
        let on = chrome.tab == which;
        let lit = chrome.hover_tab == Some(which);
        fill_rect_rounded(
            scene,
            rect,
            m.corner_radius,
            if on {
                p.accent
            } else if lit {
                p.border
            } else {
                p.panel
            },
        );
        let ink = if on { p.panel } else { p.text };
        // The glyph, then the caption beside it: a tab is found by its picture
        // and confirmed by its name, and a tab strip of pictures alone is one
        // you have to learn before you can use it.
        let side = rect.height * 0.62;
        let glyph = Rect::new(
            rect.x + 6.0,
            rect.y + (rect.height - side) / 2.0,
            side,
            side,
        );
        let text = labels_get(chrome.labels, caption);
        let width = text.map_or(0.0, |t| t.width);
        let content = side + 4.0 + width;
        let left = rect.x + ((rect.width - content) / 2.0).max(2.0);
        draw_icon(
            scene,
            icon,
            Rect::new(left, glyph.y, side, side).intersection(&rect),
            ink,
        );
        if let Some(text) = text {
            draw_text_clipped(
                scene,
                text,
                rect,
                left + side + 4.0,
                rect.y + (rect.height - text.height) / 2.0,
                ink,
            );
        }
    }
}

/// A glyph and its caption, centred together in `area`.
///
/// Both, not one: a button is found by its picture and confirmed by its name,
/// and a strip of pictures alone is one you have to learn before you can use
/// it. Draws the caption alone if it has not been shaped, and the glyph alone
/// if there is no room for words.
pub fn draw_captioned_icon(
    scene: &mut Scene,
    labels: &Labels,
    icon: crate::icon::Icon,
    caption: &str,
    area: Rect,
    color: Color,
) {
    if area.is_empty() {
        return;
    }
    let side = (area.height * 0.66).min(area.width);
    let text = labels_get(labels, caption);
    let width = text.map_or(0.0, |t| t.width);
    let gap = if width > 0.0 { 4.0 } else { 0.0 };
    let content = side + gap + width;
    let left = area.x + ((area.width - content) / 2.0).max(2.0);
    let y = area.y + (area.height - side) / 2.0;
    draw_icon(
        scene,
        icon,
        Rect::new(left, y, side, side).intersection(&area),
        color,
    );
    if let Some(text) = text
        && left + side + gap + width <= area.right() + 0.5
    {
        draw_text_clipped(
            scene,
            text,
            area,
            left + side + gap,
            area.y + (area.height - text.height) / 2.0,
            color,
        );
    }
}

/// How thick an icon's strokes are, as a fraction of the box it is drawn in.
///
/// One tenth: heavy enough to read at sixteen pixels, light enough that a
/// pencil still looks like a pencil at forty.
const ICON_STROKE: f32 = 0.10;

/// Draws `icon` inside `area`, in `color` (TDD §16.1).
///
/// The unit box maps onto the **largest square** that fits, centred — an icon
/// stretched to a wide button is a wrong shape, not a big one. See
/// [`crate::icon`] for why these are paths rather than files.
pub fn draw_icon(scene: &mut Scene, icon: crate::icon::Icon, area: Rect, color: Color) {
    use crate::icon::Shape;

    let side = area.width.min(area.height);
    if side <= 0.0 {
        return;
    }
    let x0 = area.x + (area.width - side) / 2.0;
    let y0 = area.y + (area.height - side) / 2.0;
    let at = |p: (f32, f32)| Point::new((x0 + p.0 * side) as f64, (y0 + p.1 * side) as f64);
    let stroke = Stroke::new((side * ICON_STROKE).max(0.75) as f64)
        .with_caps(vello::kurbo::Cap::Round)
        .with_join(vello::kurbo::Join::Round);
    let ink = color.to_peniko();

    for shape in crate::icon::shapes(icon) {
        match shape {
            Shape::Line { points, closed } => {
                let mut path = BezPath::new();
                for (index, p) in points.iter().enumerate() {
                    if index == 0 {
                        path.move_to(at(*p));
                    } else {
                        path.line_to(at(*p));
                    }
                }
                if closed {
                    path.close_path();
                }
                scene.stroke(&stroke, Affine::IDENTITY, ink, None, &path);
            }
            Shape::Poly(points) => {
                let mut path = BezPath::new();
                for (index, p) in points.iter().enumerate() {
                    if index == 0 {
                        path.move_to(at(*p));
                    } else {
                        path.line_to(at(*p));
                    }
                }
                path.close_path();
                scene.fill(Fill::NonZero, Affine::IDENTITY, ink, None, &path);
            }
            Shape::Circle { at: c, r, filled } => {
                let circle = vello::kurbo::Circle::new(at(c), (r * side) as f64);
                if filled {
                    scene.fill(Fill::NonZero, Affine::IDENTITY, ink, None, &circle);
                } else {
                    scene.stroke(&stroke, Affine::IDENTITY, ink, None, &circle);
                }
            }
        }
    }
}

fn labels_get<'a>(labels: &'a Labels, caption: &str) -> Option<&'a TextLayout> {
    labels.get(caption)
}

/// The instrument editor: a panel of knobs over one channel's patch.
///
/// Every control is the same shape — a caption, a dial or a chip, and the value
/// underneath in its own units. A knob whose number you cannot read is a knob
/// you cannot set, which is the whole difference between this and a row of
/// anonymous sliders in a host's generic editor.
fn draw_instrument(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    chrome: &InstrumentChrome<'_>,
) {
    let p = &theme.palette;
    let m = &theme.metrics;
    let l = &chrome.layout;
    if l.body.is_empty() {
        return;
    }

    // **The name, as a field.** *"the name section in the instrument window
    // for the soundfont player should be kind of like field instead of a bar
    // and i should be able to drag soundfonts into it."* A field is a thing
    // you can put something *in*, and that is what this one is for — it is the
    // drop target for a soundfont carried out of the browser, so it is drawn
    // sunken and outlined rather than as a filled strip.
    if !l.name.is_empty() {
        fill_rect_rounded(scene, l.name, m.corner_radius, p.window);
        stroke_rect_rounded(scene, l.name, m.corner_radius, 1.0, p.border);
        if let Some(text) = labels.get(&chrome.view.title) {
            draw_text_clipped(
                scene,
                text,
                l.name,
                l.name.x + 6.0,
                l.name.y + (l.name.height - text.height) / 2.0,
                p.text,
            );
        }
    }

    // The key row, above the first heading. A chip rather than a knob, because
    // a key names a track and a track is not a number with a range — and
    // because choosing one is a press rather than a turn.
    //
    // The **chosen** chip is filled rather than outlined. There is always one,
    // because "no key" is one of them.
    //
    // The preset row that used to sit above this one is gone: a preset is a
    // file now and the bar in the window's header is where every device's
    // are chosen (`docs/flopsynth-plan.md` §P.9).
    for (chips, names, chosen, hovered) in [(
        &l.keys,
        &chrome.view.keys,
        chrome.view.key,
        chrome.hover_key,
    )] {
        for (index, rect) in chips {
            let Some(name) = names.get(*index) else {
                continue;
            };
            if rect.is_empty() {
                continue;
            }
            let lit = hovered == Some(*index);
            let picked = chosen == Some(*index);
            fill_rect_rounded(
                scene,
                *rect,
                m.corner_radius,
                match (picked, lit) {
                    (true, _) => p.accent,
                    (false, true) => p.row_accidental,
                    (false, false) => p.panel_header,
                },
            );
            stroke_rect_rounded(scene, *rect, m.corner_radius, 1.0, p.border);
            if let Some(text) = labels.get(name.as_str()) {
                draw_text_clipped(
                    scene,
                    text,
                    *rect,
                    rect.x + 6.0,
                    rect.y + (rect.height - text.height) / 2.0,
                    // The palette has no on-accent colour; the accent is a
                    // fill light enough that the panel's own text reads on it,
                    // which is what every other selected row here uses.
                    p.text,
                );
            }
        }
    }

    for (index, rect) in &l.headings {
        let Some(group) = chrome.view.groups.get(*index) else {
            continue;
        };
        if rect.is_empty() {
            continue;
        }
        fill_rect(
            scene,
            Rect::new(rect.x, rect.bottom() - 1.0, rect.width, 1.0),
            p.border,
        );
        if let Some(text) = labels.get(&group.name) {
            draw_text_clipped(
                scene,
                text,
                *rect,
                rect.x + 2.0,
                rect.y + (rect.height - text.height) / 2.0,
                p.text_muted,
            );
        }
    }

    for (group, index, cell) in &l.cells {
        let Some(param) = chrome.view.param(*group, *index) else {
            continue;
        };
        if cell.is_empty() {
            continue;
        }
        let hot = chrome.active == Some((*group, *index));
        let lit = hot || chrome.hover == Some((*group, *index));
        if lit {
            fill_rect_rounded(scene, *cell, m.corner_radius, p.row_accidental);
        }

        // The caption over the control, the read-out under it.
        // Two lines of text and the rest to the control: a dial squeezed into
        // a third of the cell is a smudge, not a knob.
        let line = m.row_height.min(cell.height * 0.26);
        let caption = Rect::new(cell.x, cell.y, cell.width, line);
        let readout = Rect::new(cell.x, cell.bottom() - line, cell.width, line);
        let control = Rect::new(
            cell.x,
            caption.bottom(),
            cell.width,
            (readout.y - caption.bottom()).max(0.0),
        );

        for (rect, caption_text, ink) in [
            (caption, param.label.as_str(), p.text_muted),
            (
                readout,
                param.display.as_str(),
                if hot { p.accent } else { p.text },
            ),
        ] {
            if let Some(text) = labels.get(caption_text) {
                draw_text_clipped(
                    scene,
                    text,
                    rect,
                    rect.x + ((rect.width - text.width) / 2.0).max(1.0),
                    rect.y + (rect.height - text.height) / 2.0,
                    ink,
                );
            }
        }

        match &param.kind {
            ParamKind::Knob => draw_knob(scene, theme, control, param.value, hot, param.automated),
            ParamKind::Switch => {
                let on = param.value >= 0.5;
                let chip = control.inset((control.height * 0.25).min(control.width * 0.3));
                fill_rect_rounded(
                    scene,
                    chip,
                    chip.height / 2.0,
                    if on { p.accent } else { p.border },
                );
                // A switch has no groove to recolour, so the ring goes round
                // the chip. Same statement as a knob's, in the one shape a
                // two-position control has.
                if param.automated {
                    stroke_rect_rounded(
                        scene,
                        chip.inset(-2.0),
                        (chip.height / 2.0) + 2.0,
                        AUTOMATION_RING_WIDTH,
                        p.param_automated,
                    );
                }
            }
            ParamKind::Choice(options) => {
                // A row of pips saying where in the list you are: a chip that
                // only shows the current option gives no sense of how many
                // there are or how far round you have gone.
                let chosen = choice_index(&param.kind, param.value);
                let count = options.len().max(1);
                let pip = (control.width / (count as f32 * 2.0)).clamp(2.0, 7.0);
                let span = pip * (2 * count - 1) as f32;
                let y = control.y + (control.height - pip) / 2.0;
                for n in 0..count {
                    let x = control.x + (control.width - span) / 2.0 + n as f32 * pip * 2.0;
                    fill_rect_rounded(
                        scene,
                        Rect::new(x, y, pip, pip),
                        pip / 2.0,
                        if n == chosen { p.accent } else { p.border },
                    );
                }
                // Round the whole row of pips, for the reason the switch's
                // goes round its chip.
                if param.automated {
                    let row = Rect::new(
                        control.x + (control.width - span) / 2.0 - 3.0,
                        y - 3.0,
                        span + 6.0,
                        pip + 6.0,
                    );
                    stroke_rect_rounded(
                        scene,
                        row,
                        (pip + 6.0) / 2.0,
                        AUTOMATION_RING_WIDTH,
                        p.param_automated,
                    );
                }
            }
        }
    }
}

/// One dial: a track, and an arc from the bottom-left round to where the value
/// is.
///
/// An arc rather than a filled pie, because a pie reads as a level meter and a
/// dial is a *setting*. Built as a polyline: `vello` would draw a real arc, and
/// twenty-four segments is indistinguishable at this size and one less thing to
/// get wrong.
///
/// The angle runs **clockwise from twelve o'clock**, which is the one thing to
/// be careful of: screen y grows downwards, so the obvious spelling of an arc
/// comes out mirrored and the knob turns anticlockwise. The first version did.
/// How thick the ring round an automated control is drawn.
///
/// Wide enough that its interior samples the colour exactly rather than a
/// blend with what is under it — which is what makes it a claim a test can
/// check as well as one an eye can see.
const AUTOMATION_RING_WIDTH: f32 = 2.0;

fn draw_knob(scene: &mut Scene, theme: &Theme, area: Rect, value: f32, hot: bool, automated: bool) {
    let p = &theme.palette;
    if area.is_empty() {
        return;
    }
    let radius = (area.width.min(area.height) / 2.0 - 1.0).max(2.0);
    let cx = area.x + area.width / 2.0;
    let cy = area.y + area.height / 2.0;
    // The scale round the knob, like the marks printed on a panel; under
    // everything, so the arc and the halo sit over it.
    bridge::draw_knob_ticks(scene, theme, (cx, cy), radius);
    let value = value.clamp(0.0, 1.0);

    // Seven o'clock round to five o'clock — the 270-degree sweep every hardware
    // knob has, so "straight up" is the middle of the range.
    let angle_of = |t: f32| (-0.75 + 1.5 * t) * std::f32::consts::PI;
    let point = |t: f32, r: f32| {
        let a = angle_of(t);
        ((cx + r * a.sin()) as f64, (cy - r * a.cos()) as f64)
    };
    let path_of = |to: f32| {
        let mut path = BezPath::new();
        const STEPS: usize = 24;
        for step in 0..=STEPS {
            let point = point(to * step as f32 / STEPS as f32, radius);
            if step == 0 {
                path.move_to(point);
            } else {
                path.line_to(point);
            }
        }
        path
    };

    // The body, so the dial reads as an object rather than as a stray stroke.
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        p.panel_header.to_peniko(),
        None,
        &vello::kurbo::Circle::new((cx as f64, cy as f64), (radius - 2.0).max(1.0) as f64),
    );

    // The groove. **This is the ring §12.2 asks for**: when a lane owns the
    // control it is drawn in the automation colour rather than the chrome's,
    // so a knob somebody's automation is holding is tellable from one nobody
    // has touched — at a glance, and without a second badge to find room for.
    //
    // The groove rather than the value arc, because the value arc is the part
    // you read the setting off and it has to keep saying what the setting is.
    let width = (radius * 0.28).clamp(2.0, 4.0);
    scene.stroke(
        &Stroke::new(width as f64),
        Affine::IDENTITY,
        if automated {
            p.param_automated
        } else {
            p.grid_line_strong
        }
        .to_peniko(),
        None,
        &path_of(1.0),
    );
    if value > 0.0 {
        scene.stroke(
            &Stroke::new(width as f64),
            Affine::IDENTITY,
            if hot { p.accent } else { p.note }.to_peniko(),
            None,
            &path_of(value),
        );
    }

    // The pointer, so the value is readable at a glance rather than by
    // measuring an arc.
    let mut needle = BezPath::new();
    needle.move_to(point(value, radius * 0.25));
    needle.line_to(point(value, (radius - width).max(radius * 0.5)));
    scene.stroke(
        &Stroke::new((width * 0.6).max(1.5) as f64),
        Affine::IDENTITY,
        if hot { p.accent } else { p.text }.to_peniko(),
        None,
        &needle,
    );
}

/// The arrangement panel's own heading.
pub const ARRANGEMENT: &str = "Arrangement";

/// What is written on a lane that has no clips and no name yet.
pub const EMPTY_LANE: &str = "\u{2014}";

/// The arrangement (TDD §16.4): clips as blocks on lanes.
///
/// Four layers back to front, the same as the roll's: the lane shading, the bar
/// lines, the clips, the playhead. **Only the visible window is built** —
/// `visible_lanes` and `timeline_visible_ticks` bound every loop here.
/// The arrangement's controls.
///
/// The same shape as the roll's toolbar so the two panels read alike, with one
/// difference that matters: a button whose action would do nothing right now —
/// Paste with an empty clipboard — is drawn muted rather than hidden. A
/// control that comes and goes is harder to learn than one that is plainly
/// unavailable.
fn draw_timeline_toolbar(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    chrome: &TimelineChrome<'_>,
) {
    let p = &theme.palette;
    let m = &theme.metrics;
    if chrome.layout.toolbar.is_empty() {
        return;
    }
    let timeline_snap = crate::canvas::snap_caption(chrome.view.snap);
    fill_rect(scene, chrome.layout.toolbar, p.panel_header);

    let selected = !chrome.selection.is_empty();
    for (control, rect) in &chrome.toolbar.items {
        if rect.is_empty() {
            continue;
        }
        // What this button would do, if pressed now.
        let live = match control {
            TimelineControl::Snap
            | TimelineControl::Stretch
            | TimelineControl::ZoomOut
            | TimelineControl::ZoomIn
            | TimelineControl::Draw
            | TimelineControl::Select
            | TimelineControl::Slice => true,
            TimelineControl::Paste => chrome.can_paste,
            _ => selected,
        };
        // The tool that is on is lit, the way the roll's are: a pair of chips
        // where neither says which one you are in is a pair you have to press
        // to find out.
        let on = match control {
            TimelineControl::Draw => chrome.tool == crate::canvas::TimelineTool::Draw,
            TimelineControl::Select => chrome.tool == crate::canvas::TimelineTool::Select,
            TimelineControl::Slice => chrome.tool == crate::canvas::TimelineTool::Slice,
            TimelineControl::Stretch => chrome.stretch,
            _ => false,
        };
        // The snap chip is a read-out and always carries its frame: a bare
        // word in a toolbar is not something anyone reads as a control. That
        // is exactly how the roll's snap chip came to be reported missing.
        // The stretch switch is a word too, and an unlit word beside the snap
        // chip would read as a caption for it rather than as a switch of its own.
        if on
            || matches!(control, TimelineControl::Snap | TimelineControl::Stretch)
            || chrome.hover == Some(*control)
        {
            fill_rect_rounded(
                scene,
                *rect,
                m.corner_radius,
                if on {
                    p.accent
                } else if chrome.hover == Some(*control) {
                    p.border
                } else {
                    p.panel
                },
            );
        }
        let ink = if on {
            p.panel
        } else if live {
            p.text
        } else {
            p.text_muted
        };
        // A verb draws its glyph, a read-out its value — and a button whose
        // action would do nothing right now is drawn muted rather than
        // hidden, which is why `live` reaches the icon too.
        if let Some(icon) = control.icon() {
            draw_icon(scene, icon, rect.inset(rect.height * 0.24), ink);
            continue;
        }
        let caption = match control {
            TimelineControl::Snap => &timeline_snap,
            other => other.label(),
        };
        let Some(text) = labels.get(caption) else {
            continue;
        };
        draw_text_clipped(
            scene,
            text,
            *rect,
            rect.x + ((rect.width - text.width) / 2.0).max(1.0),
            rect.y + (rect.height - text.height) / 2.0,
            ink,
        );
    }
}

fn draw_timeline(scene: &mut Scene, theme: &Theme, labels: &Labels, chrome: &TimelineChrome<'_>) {
    let p = &theme.palette;
    let l = &chrome.layout;
    let v = &chrome.view;
    if l.grid.is_empty() {
        return;
    }
    fill_rect(scene, l.grid, p.panel);
    fill_rect(scene, l.headers, p.panel_header);

    let lanes = visible_lanes(v, l.grid, chrome.lanes.len());
    let ticks = timeline_visible_ticks(v, l.grid);

    // Alternating lane shading, so a block's row can be read off across a wide
    // arrangement without counting.
    for lane in lanes.clone() {
        let y = lane_to_y(v, l.grid, lane);
        let row = Rect::new(l.grid.x, y, l.grid.width, v.lane_height);
        if lane % 2 == 1 {
            fill_rect(scene, row.intersection(&l.grid), p.row_accidental);
        }
        fill_rect(
            scene,
            Rect::new(l.grid.x, y + v.lane_height - 1.0, l.grid.width, 1.0).intersection(&l.grid),
            p.grid_line,
        );

        // The lane's name, and a mute switch that is the whole row's left edge.
        let header =
            Rect::new(l.headers.x, y, l.headers.width, v.lane_height).intersection(&l.headers);
        if header.is_empty() {
            continue;
        }
        let info = chrome.lanes.get(lane);
        let muted = info.is_some_and(|i| i.muted);
        fill_rect(
            scene,
            Rect::new(header.x, header.y, 3.0, header.height),
            if muted { p.meter_peak } else { p.accent },
        );
        let name = info.map_or(EMPTY_LANE, |i| i.name.as_str());
        if let Some(text) = labels.get(name) {
            draw_text_clipped(
                scene,
                text,
                header,
                header.x + 8.0,
                header.y + (header.height - text.height) / 2.0,
                if muted { p.text_muted } else { p.text },
            );
            if chrome.renaming == Some(lane) {
                draw_rename_marks(
                    scene,
                    theme,
                    header,
                    header.x + 9.0,
                    text.width,
                    chrome.rename,
                );
            }
        }
        fill_rect(
            scene,
            Rect::new(header.x, header.bottom() - 1.0, header.width, 1.0),
            p.border,
        );
    }

    // Bar lines, and a stronger one every four bars so a long arrangement can
    // be counted at a glance.
    let bar = PPQN * Tick::from(chrome.beats_per_bar.max(1));
    for (unit, colour) in [(bar, p.grid_line), (bar * 4, p.grid_line_strong)] {
        if unit <= 0 || (unit as f32) * v.pixels_per_tick < 4.0 {
            continue;
        }
        let mut tick = ticks.start - ticks.start.rem_euclid(unit);
        while tick < ticks.end {
            let x = timeline_tick_to_x(v, l.grid, tick).floor();
            fill_rect(
                scene,
                Rect::new(x, l.grid.y, 1.0, l.grid.height).intersection(&l.grid),
                colour,
            );
            tick += unit;
        }
    }

    // The time selection, as a band down the grid: what will loop, where it
    // is edited. Under the clips, so it tints rather than covers them.
    if let Some((from, to)) = chrome.loop_range {
        let x0 = timeline_tick_to_x(v, l.grid, from);
        let x1 = timeline_tick_to_x(v, l.grid, to);
        let band = Rect::new(x0, l.grid.y, (x1 - x0).max(0.0), l.grid.height).intersection(&l.grid);
        if !band.is_empty() {
            fill_rect(scene, band, p.selection);
        }
    }

    // The clips.
    for clip in chrome.clips {
        if !lanes.contains(&clip.lane) {
            continue;
        }
        if clip.start + clip.length < ticks.start || clip.start > ticks.end {
            continue;
        }
        let whole = clip_rect(v, l.grid, clip);
        let block = whole.intersection(&l.grid);
        if block.is_empty() {
            continue;
        }
        let selected = chrome.selection.contains(&clip.id);
        let automation = clip.kind == crate::document::ClipKind::Automation;
        let body = if clip.muted {
            p.grid_line
        } else if selected {
            p.note_selected
        } else {
            Color(clip.color)
        };
        // An automation block is a **ground for a curve**, so it is filled
        // dark rather than in the lane's colour: the shape is the content, and
        // a bright block with a line on it hides the line. A note block is the
        // other way round — the block *is* what there is to see.
        //
        // **Whatever else is true of it**, selection included. Reported from
        // using the window: *"when an automation clip is selected i cannot see
        // the graph at all so i have to unselect it to see how it actually
        // looks."* Selecting used to fill the block in the selection colour and
        // then stroke the curve in that same colour — a line painted onto its
        // own background, so the one moment you most need the shape was the one
        // moment it was gone. A selected block says so with an **edge** and
        // with the colour of its curve instead; see below.
        fill_rect_rounded(
            scene,
            block.inset(1.0),
            3.0,
            if automation { p.panel_header } else { body },
        );
        if automation {
            if selected {
                // The edge is what selection means here. Drawn before the
                // curve so the curve stays the brightest thing in the block.
                scene.stroke(
                    &Stroke::new(1.5),
                    Affine::IDENTITY,
                    p.note_selected.to_peniko(),
                    None,
                    &rounded(block.inset(1.0), 3.0),
                );
            }
            // Against the **whole** block, not the part on screen: the
            // anatomy the pointer is tested against is the whole block's,
            // and a curve measured against the visible part would slide as
            // the arrangement scrolled. Clipped to the grid instead.
            let lit: &[fontelle_types::PointId] = if chrome.point_clip == Some(clip.id) {
                chrome.point_selection
            } else {
                &[]
            };
            draw_automation_curve(scene, theme, l.grid, whole, clip, body, lit);
        } else if clip.kind == crate::document::ClipKind::Audio {
            // *"i should be able to see the waveform of the audio inside the
            // clip."* Same band as the note preview, same reason.
            draw_clip_waveform(scene, theme, l.grid, whole, clip, selected);
            // And its fades over the waveform: the curve the player uses,
            // the part it takes away shaded, and — on the chosen block —
            // the handles that set it (TDD §15.2). See `canvas::fade_anatomy`.
            let hovered = chrome
                .hover_clip
                .filter(|(id, _)| *id == clip.id)
                .map(|(_, part)| part);
            let fading = chrome
                .fading
                .filter(|(id, _)| *id == clip.id)
                .map(|(_, end)| end);
            draw_clip_fades(
                scene, theme, labels, l.grid, whole, clip, selected, hovered, fading,
            );
        } else {
            // And a note clip shows the notes that are in it, for the same
            // reason: what is *in* a clip is the thing you are looking for
            // when you scan an arrangement.
            draw_clip_notes(scene, theme, l.grid, whole, clip, selected);
            // And the ones landing in it right now, while a take is being
            // recorded: the open clip is where the take goes, so the block
            // fills with the notes as they are played, in the record colour.
            if clip.open && !chrome.take_notes.is_empty() {
                draw_take_notes(scene, theme, l.grid, whole, clip, chrome.take_notes);
            }
        }
        // The open clip gets a bright edge: the roll below is showing this one,
        // and nothing else on screen said so.
        if clip.open {
            scene.stroke(
                &Stroke::new(2.0),
                Affine::IDENTITY,
                p.accent.to_peniko(),
                None,
                &rounded(block.inset(1.0), 3.0),
            );
        }
        // The seams between a looped clip's repeats. **Between** them, not
        // around them: a line at each end is a border, and a bordered block is
        // exactly the copy-and-paste picture looping is meant to stop looking
        // like. Drawn before the caption so the caption sits on top.
        let seams = crate::canvas::loop_marks(v, l.grid, clip);
        for x in &seams {
            let seam = Rect::new(*x, block.y + 1.0, 1.0, (block.height - 2.0).max(0.0))
                .intersection(&l.grid);
            if seam.is_empty() {
                continue;
            }
            fill_rect(scene, seam, p.panel);
            // A small notch out of the top edge at each seam, which is what
            // makes the repeats readable at a glance rather than only on
            // close inspection — the block reads as a strip of tiles.
            let notch = Rect::new(*x - 2.0, block.y + 1.0, 5.0, 3.0).intersection(&l.grid);
            if !notch.is_empty() {
                fill_rect(scene, notch, p.panel);
            }
        }
        if let Some(text) = labels.get(&clip.name) {
            // Written once per repeat, faint after the first. A four-bar loop
            // that says its name four times is a loop; one that says it once
            // is a long block.
            //
            // In the **caption band** rather than down the middle of the
            // block: the middle is where the content is drawn now, and a name
            // written across a preview is a name over the thing it names. On
            // a block too short to have a band it goes back to the centre,
            // because there is nothing under it there either.
            let (header, _) = crate::canvas::clip_bands(whole);
            let first = block.x + 5.0;
            let y = if header.height >= text.height {
                header.y + (header.height - text.height) / 2.0
            } else {
                block.y + (block.height - text.height) / 2.0
            };
            let ink = if clip.muted {
                p.text_muted
            } else if automation {
                // Behind the curve, not competing with it: on an automation
                // block the name says *which parameter*, and the shape is what
                // is actually being read.
                p.text_muted
            } else {
                p.panel
            };
            draw_text_clipped(scene, text, block, first, y, ink);
            for x in &seams {
                let repeat =
                    Rect::new(*x, block.y, block.right() - *x, block.height).intersection(&l.grid);
                if repeat.width < text.width + 6.0 {
                    continue;
                }
                draw_text_clipped(
                    scene,
                    text,
                    repeat,
                    x + 5.0,
                    y,
                    Color([ink.0[0], ink.0[1], ink.0[2], REPEAT_CAPTION_ALPHA]),
                );
            }
        }
    }

    // Every block's edge, over everything: *"make it so that the edges of
    // clips are always visible and dont blend into eachother when they get
    // close or even overlapped."* Drawn after all the bodies rather than
    // with each, because a block painted later covers the end of the one
    // before it, and an edge painted with its body is under the next body.
    // One dark line, the ground's own colour, so two blocks of one colour
    // side by side are two blocks.
    for clip in chrome.clips {
        if !lanes.contains(&clip.lane) {
            continue;
        }
        let block = clip_rect(v, l.grid, clip).intersection(&l.grid);
        if block.is_empty() {
            continue;
        }
        scene.stroke(
            &Stroke::new(1.0),
            Affine::IDENTITY,
            p.window.to_peniko(),
            None,
            &rounded(block.inset(1.0), 3.0),
        );
    }
    // And where two blocks on a row lie over each other, stripes across the
    // part they share — see `canvas::clip_overlaps`. Diagonal, so they read
    // as a marking rather than as content: nothing else on the arrangement
    // runs at an angle. Over the stripes, when the two are audio, the
    // crossfade they are actually playing: two curves crossing.
    for shared in crate::canvas::clip_overlaps(v, l.grid, chrome.clips) {
        draw_clip_overlap(scene, theme, &shared);
    }

    if let Some(box_) = chrome.marquee {
        let box_ = box_.intersection(&l.grid);
        if !box_.is_empty() {
            fill_rect(scene, box_, p.selection);
            scene.stroke(
                &Stroke::new(1.0),
                Affine::IDENTITY,
                p.accent.to_peniko(),
                None,
                &KRect::new(
                    box_.x as f64,
                    box_.y as f64,
                    box_.right() as f64,
                    box_.bottom() as f64,
                ),
            );
        }
    }

    // The cut tool. **Two marks, and they say two different things.**
    //
    // The stroke is the gesture — where the hand went — and it is drawn faint,
    // because it is feedback that the drag is happening and nothing more. The
    // bright marks are the *cuts*: one per clip the stroke crosses, at the
    // tick `clip_cuts` will actually divide it on, which is the pointer
    // snapped to the arrangement's grid.
    //
    // > *"instead of actually displaying it rounded to the grid that it's
    // > going to cut the actual clip at."*
    //
    // Drawing only the stroke was showing the question rather than the answer:
    // a line aimed a third of a beat late looked like a cut a third of a beat
    // late, and landed on the beat. See `canvas::slice_marks`, which asks the
    // same function the release will ask.
    if let Some((from, to)) = chrome.slice {
        let mut line = BezPath::new();
        line.move_to(Point::new(from.0 as f64, from.1 as f64));
        line.line_to(Point::new(to.0 as f64, to.1 as f64));
        scene.push_layer(
            Fill::NonZero,
            BlendMode::default(),
            1.0,
            Affine::IDENTITY,
            &KRect::new(
                l.grid.x as f64,
                l.grid.y as f64,
                l.grid.right() as f64,
                l.grid.bottom() as f64,
            ),
        );
        scene.stroke(
            &Stroke::new(1.0),
            Affine::IDENTITY,
            p.meter_peak.with_alpha(0x55).to_peniko(),
            None,
            &line,
        );
        for mark in crate::canvas::slice_marks(
            v,
            l.grid,
            chrome.clips,
            from,
            to,
            v.snap,
            chrome.beats_per_bar,
        ) {
            fill_rect(scene, mark, p.meter_peak);
            // A cap at each end, so a mark on a clip whose colour is close to
            // the ink still reads as a cut rather than as a stripe.
            for y in [mark.y, mark.bottom() - 3.0] {
                fill_rect(
                    scene,
                    Rect::new(mark.x - 2.0, y, mark.width + 4.0, 3.0),
                    p.meter_peak,
                );
            }
        }
        scene.pop_layer();
    }

    draw_timeline_ruler(scene, theme, labels, chrome);
    draw_timeline_toolbar(scene, theme, labels, chrome);

    // A border down the seam between the headers and the grid.
    fill_rect(
        scene,
        Rect::new(l.grid.x - 1.0, l.grid.y, 1.0, l.grid.height),
        p.border,
    );
    draw_focus_edge(scene, theme, chrome.panel.frame, chrome.focused);
}

/// How wide the focused pane's spine is.
const FOCUS_EDGE_PX: f32 = 2.0;

/// **Which canvas the keyboard is talking to**, said quietly.
///
/// > *"we need to make it possible to tell which window is currently focused
/// > that way you don't accidentally do something in the wrong window ... this
/// > needs to be subtle but easy to tell at a glance."*
///
/// A two-point bar down the inside of the focused pane's left edge, in the
/// accent. Chosen over the two louder readings for a reason each: **dimming
/// the unfocused pane's contents** would change the colour of the clips and
/// notes you are judging edits by, and a picture that shifts with focus is a
/// picture you cannot trust; **tinting a header** is quieter still but sits
/// outside the working area, where the eye is not.
///
/// A spine rather than the full outline this used to draw: an outline reads as
/// a selected object, and a pane is not an object you selected — it is where
/// you are. It is on the left because that is where both panes begin, so the
/// mark is always in the same place relative to what it marks.
fn draw_focus_edge(scene: &mut Scene, theme: &Theme, frame: Rect, focused: bool) {
    if !focused || frame.is_empty() {
        return;
    }
    let radius = theme.metrics.corner_radius.min(frame.height / 2.0);
    fill_rect_rounded(
        scene,
        Rect::new(
            frame.x,
            frame.y + radius,
            FOCUS_EDGE_PX,
            (frame.height - radius * 2.0).max(1.0),
        ),
        FOCUS_EDGE_PX / 2.0,
        theme.palette.accent,
    );
}

/// The notes inside a note clip's block (TDD §16.4).
///
/// *"make it so the midi clips in the arrangement arent just blank rectangles
/// but instead actually show a preview of the notes drawn out inside of it
/// like how other daws do."* The geometry is `canvas::clip_notes` — including
/// which passes of a loop are on screen — so this only has to choose an ink
/// and put the rectangles down.
///
/// The ink is the **block's own colour lightened**, not the palette's note
/// colour: a preview has to read as part of the clip it is in rather than as
/// something lying on top of it, and every lane has a different colour to be
/// part of.
fn draw_clip_notes(
    scene: &mut Scene,
    theme: &Theme,
    grid: Rect,
    block: Rect,
    clip: &ClipInfo,
    selected: bool,
) {
    let rects = crate::canvas::clip_notes(block, grid, clip);
    if rects.is_empty() {
        return;
    }
    let p = &theme.palette;
    // On a selected block the body is already the selection colour, so the
    // notes take the panel's dark ink to stay legible against it.
    let ink = if selected || clip.muted {
        p.panel
    } else {
        lighten(Color(clip.color), 0.55)
    };

    scene.push_layer(
        Fill::NonZero,
        BlendMode::default(),
        1.0,
        Affine::IDENTITY,
        &KRect::new(
            grid.x as f64,
            grid.y as f64,
            grid.right() as f64,
            grid.bottom() as f64,
        ),
    );
    for rect in rects {
        // Rounded only when there is room to see a corner; below that the
        // rounding eats the note.
        if rect.height >= 3.0 && rect.width >= 3.0 {
            fill_rect_rounded(scene, rect, 1.0, ink);
        } else {
            fill_rect(scene, rect, ink);
        }
    }
    scene.pop_layer();
}

/// The notes of a take being recorded into `clip`, drawn into its block.
///
/// The same geometry as [`draw_clip_notes`] — `canvas::clip_notes` over the
/// take's notes as if they were the clip's, so a note lands exactly where it
/// will be drawn once it is kept — in the record colour, which is the one ink
/// on this panel that says "not in the document yet".
fn draw_take_notes(
    scene: &mut Scene,
    theme: &Theme,
    grid: Rect,
    block: Rect,
    clip: &ClipInfo,
    takes: &[crate::document::NotePreview],
) {
    let mut as_if = clip.clone();
    as_if.notes = takes.to_vec();
    let rects = crate::canvas::clip_notes(block, grid, &as_if);
    if rects.is_empty() {
        return;
    }
    let p = &theme.palette;
    scene.push_layer(
        Fill::NonZero,
        BlendMode::default(),
        1.0,
        Affine::IDENTITY,
        &KRect::new(
            grid.x as f64,
            grid.y as f64,
            grid.right() as f64,
            grid.bottom() as f64,
        ),
    );
    for rect in rects {
        // A key that has only just gone down is a sliver, never nothing.
        let rect = Rect::new(rect.x, rect.y, rect.width.max(2.0), rect.height);
        if rect.height >= 3.0 && rect.width >= 3.0 {
            fill_rect_rounded(scene, rect, 1.0, p.meter_peak);
        } else {
            fill_rect(scene, rect, p.meter_peak);
        }
    }
    scene.pop_layer();
}

/// A colour moved `amount` of the way towards white.
///
/// For the note preview: a clip's own colour at full strength is the block it
/// is drawn on, so the notes have to be a step away from it, and a step
/// towards white keeps the hue the lane is identified by.
fn lighten(colour: Color, amount: f32) -> Color {
    let mix = |c: u8| (f32::from(c) + (255.0 - f32::from(c)) * amount) as u8;
    Color([
        mix(colour.0[0]),
        mix(colour.0[1]),
        mix(colour.0[2]),
        colour.0[3],
    ])
}

/// How far towards white a waveform is drawn against its block.
///
/// A step further than the note preview's, because a waveform is a thin line
/// where a note is a slab: the same distance from the ground reads fainter.
const WAVEFORM_LIGHTEN: f32 = 0.7;

/// How solid the waveform's **outline** is, against the core drawn solid
/// inside it — see `canvas::clip_waveform_core`. Enough that the extremes
/// are read as the take's reach, not so much that the core is lost in them.
const WAVEFORM_OUTLINE_ALPHA: u8 = 0x78;

/// The waveform inside an audio clip's block (TDD §15.3).
///
/// *"i should be able to see the waveform of the audio inside the clip."* The
/// geometry is `canvas::clip_waveform` — including which columns are on screen
/// — so this only has to choose an ink and put the rectangles down.
///
/// The ink is the block's own colour **lightened**, exactly as the note
/// preview's is and for the same reason: what is inside a clip has to read as
/// part of it rather than as something lying on top.
///
/// **Two tones.** The extremes are the outline, in a translucent ink; the
/// loudness (`canvas::clip_waveform_core`) is drawn solid inside it. The
/// outline says how far the take swung and the core how loud it was, and
/// for a voice the two are far apart — drawn as one shape, speech was a
/// fuzz of peaks; drawn as two, it reads as syllables. A preview without a
/// core yet draws the outline solid, so nothing is fainter for being older.
fn draw_clip_waveform(
    scene: &mut Scene,
    theme: &Theme,
    grid: Rect,
    block: Rect,
    clip: &ClipInfo,
    selected: bool,
) {
    let columns = crate::canvas::clip_waveform(block, grid, clip);
    if columns.is_empty() {
        return;
    }
    let p = &theme.palette;
    let ink = if clip.muted {
        p.text_muted
    } else if selected {
        p.panel
    } else {
        lighten(Color(clip.color), WAVEFORM_LIGHTEN)
    };
    let core = crate::canvas::clip_waveform_core(block, grid, clip);
    let outline = if core.is_empty() {
        ink
    } else {
        ink.with_alpha(WAVEFORM_OUTLINE_ALPHA)
    };
    for column in columns {
        let column = column.intersection(&grid);
        if column.is_empty() {
            continue;
        }
        fill_rect(scene, column, outline);
    }
    for column in core {
        let column = column.intersection(&grid);
        if column.is_empty() {
            continue;
        }
        fill_rect(scene, column, ink);
    }

    // **Where the take runs out**, when the block is longer than the sound in
    // it. A rule at the end and a dimmed band past it, so the empty part of a
    // block says *"the file stops here"* rather than saying nothing at all —
    // which is what it said before, and what somebody trimming against it read
    // as a broken picture. See `canvas::content_end`.
    if let Some(at) = crate::canvas::content_end(block, clip) {
        let (_, content) = crate::canvas::clip_bands(block);
        let past = Rect::new(at, content.y, (block.right() - at).max(0.0), content.height)
            .intersection(&grid);
        if !past.is_empty() {
            // The dim wash first, then the rule over its left edge.
            fill_rect(scene, past, p.window.with_alpha(0x40));
            // A centre line through the empty part: this is silence the block
            // is holding, and a line through the middle is how this program
            // draws silence everywhere else.
            fill_rect(
                scene,
                Rect::new(past.x, content.y + content.height / 2.0, past.width, 1.0)
                    .intersection(&grid),
                ink.with_alpha(0x40),
            );
        }
        let rule = Rect::new(at - 0.5, content.y, 1.0, content.height).intersection(&grid);
        if !rule.is_empty() {
            fill_rect(scene, rule, ink.with_alpha(0xb0));
        }
    }
}

/// An audio block's fades: each curve, the region above it shaded, and
/// the handles and nodes when the block is selected **or under the
/// pointer** — with the one the pointer is on lit, and a caption saying
/// how long a fade is while its handle is being dragged.
///
/// The curve is `canvas::fade_curve` — the same bend the player applies —
/// and the handles are `canvas::fade_anatomy`'s, the same rectangles the
/// pointer is tested against, so what you see is what you can grab. Drawn
/// clipped to the grid, like everything else on the block.
#[allow(clippy::too_many_arguments)]
fn draw_clip_fades(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    grid: Rect,
    block: Rect,
    clip: &ClipInfo,
    selected: bool,
    hovered: Option<crate::canvas::ClipPart>,
    fading: Option<crate::canvas::FadeEnd>,
) {
    use crate::canvas::{ClipPart, FadeEnd};
    let Some(anatomy) = crate::canvas::fade_anatomy(block, clip) else {
        return;
    };
    let p = &theme.palette;
    let (_, content) = crate::canvas::clip_bands(block);
    let visible = block.intersection(&grid);
    if visible.is_empty() {
        return;
    }
    scene.push_layer(
        Fill::NonZero,
        BlendMode::default(),
        1.0,
        Affine::IDENTITY,
        &KRect::new(
            visible.x as f64,
            visible.y as f64,
            visible.right() as f64,
            visible.bottom() as f64,
        ),
    );
    let ink = if selected { p.panel } else { p.text };
    let shade = Color([p.window.0[0], p.window.0[1], p.window.0[2], 0x70]);
    for end in [crate::canvas::FadeEnd::In, crate::canvas::FadeEnd::Out] {
        let points = crate::canvas::fade_curve(block, clip, end);
        if points.len() < 2 {
            continue;
        }
        // The part the fade takes away: everything above the curve, from
        // the corner the fade starts at to the top of the band where it
        // ends. Shaded rather than cut out, so the waveform under it is
        // still there to be read.
        let mut region = BezPath::new();
        let (first, last) = (points[0], points[points.len() - 1]);
        region.move_to(Point::new(f64::from(first.0), f64::from(content.y)));
        for (x, y) in &points {
            region.line_to(Point::new(f64::from(*x), f64::from(*y)));
        }
        region.line_to(Point::new(f64::from(last.0), f64::from(content.y)));
        region.close_path();
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            shade.to_peniko(),
            None,
            &region,
        );
        let mut line = BezPath::new();
        line.move_to(Point::new(f64::from(first.0), f64::from(first.1)));
        for (x, y) in &points[1..] {
            line.line_to(Point::new(f64::from(*x), f64::from(*y)));
        }
        scene.stroke(
            &Stroke::new(1.5),
            Affine::IDENTITY,
            ink.to_peniko(),
            None,
            &line,
        );
    }
    if selected || hovered.is_some() {
        // The handles, on the block in hand and on the one under the
        // pointer: a corner mark on every take would be a mark nobody asked
        // to read, and a corner with no mark until the block is chosen is a
        // handle nobody finds — FL shows its fade handles on the clip under
        // the pointer, and so does this now. The one the pointer is on is
        // lit, so it reads as a thing to take hold of.
        let lit = |part: ClipPart| {
            hovered == Some(part) || fading.is_some_and(|end| part == ClipPart::FadeHandle(end))
        };
        for (handle, part) in [
            (anatomy.handle_in, ClipPart::FadeHandle(FadeEnd::In)),
            (anatomy.handle_out, ClipPart::FadeHandle(FadeEnd::Out)),
        ] {
            let mark = Rect::new(
                handle.x + 1.0,
                handle.y + 1.0,
                (handle.width - 2.0).max(1.0),
                (handle.height - 2.0).max(1.0),
            );
            fill_rect_rounded(scene, mark, 2.0, if lit(part) { p.accent } else { ink });
        }
        for (node, part) in [
            (anatomy.node_in, ClipPart::FadeNode(FadeEnd::In)),
            (anatomy.node_out, ClipPart::FadeNode(FadeEnd::Out)),
        ] {
            let Some(node) = node else { continue };
            let (fill, edge) = if lit(part) {
                (p.accent, ink)
            } else {
                (ink, p.accent)
            };
            fill_rect_rounded(scene, node, node.width / 2.0, fill);
            stroke_rect_rounded(scene, node, node.width / 2.0, 1.0, edge);
        }
    }
    scene.pop_layer();

    // **How long, while it is being set.** A caption beside the handle in
    // hand, outside the block's clip so it is readable on a short lane, in
    // a small plate so it reads over whatever is under it — the number a
    // fade drag is really about, which the block cannot otherwise say.
    if let Some(end) = fading {
        let caption = crate::canvas::fade_caption(clip, end);
        if let Some(text) = labels.get(&caption) {
            let handle = match end {
                FadeEnd::In => anatomy.handle_in,
                FadeEnd::Out => anatomy.handle_out,
            };
            let pad = 4.0;
            let width = text.width + pad * 2.0;
            let height = text.height + pad;
            // Above the block, and on the side of the handle the fade is
            // on, so the plate never covers the curve being shaped. Kept
            // inside the grid whichever way it is pushed.
            let x = match end {
                FadeEnd::In => handle.x,
                FadeEnd::Out => handle.right() - width,
            }
            .clamp(grid.x, (grid.right() - width).max(grid.x));
            let y = (block.y - height - 2.0).max(grid.y);
            let plate = Rect::new(x, y, width, height);
            fill_rect_rounded(scene, plate, 3.0, p.panel);
            stroke_rect_rounded(scene, plate, 3.0, 1.0, p.accent);
            draw_text_clipped(scene, text, plate, x + pad, y + pad / 2.0, p.text);
        }
    }
}

/// How far apart the overlap stripes are, in points.
const OVERLAP_STRIPE_STEP: f32 = 6.0;

/// Diagonal stripes across `shared`, the part of a row two clips both
/// claim. Clipped to the rectangle, so a stripe never leaves it.
fn draw_clip_overlap(scene: &mut Scene, theme: &Theme, overlap: &crate::canvas::ClipOverlap) {
    let shared = overlap.area;
    if shared.is_empty() {
        return;
    }
    let p = &theme.palette;
    let ink = Color([p.text.0[0], p.text.0[1], p.text.0[2], 0x60]);
    scene.push_layer(
        Fill::NonZero,
        BlendMode::default(),
        1.0,
        Affine::IDENTITY,
        &KRect::new(
            shared.x as f64,
            shared.y as f64,
            shared.right() as f64,
            shared.bottom() as f64,
        ),
    );
    // Each stripe climbs one height across one height, from a start far
    // enough left that the first one still crosses the top-left corner.
    let mut x = shared.x - shared.height;
    while x < shared.right() {
        scene.stroke(
            &Stroke::new(1.5),
            Affine::IDENTITY,
            ink.to_peniko(),
            None,
            &vello::kurbo::Line::new(
                (x as f64, shared.bottom() as f64),
                ((x + shared.height) as f64, shared.y as f64),
            ),
        );
        x += OVERLAP_STRIPE_STEP;
    }
    // The crossfade, over the stripes: *"so it resembles the other fades but
    // just with them crossing through eachother."* The same ink and the same
    // weight as a clip's own fade curve, because it is the same kind of
    // statement — this is the envelope, and here is its shape.
    for curve in [&overlap.fade_in, &overlap.fade_out] {
        if curve.len() < 2 {
            continue;
        }
        let mut line = BezPath::new();
        line.move_to(Point::new(f64::from(curve[0].0), f64::from(curve[0].1)));
        for (x, y) in &curve[1..] {
            line.line_to(Point::new(f64::from(*x), f64::from(*y)));
        }
        scene.stroke(
            &Stroke::new(1.5),
            Affine::IDENTITY,
            p.text.to_peniko(),
            None,
            &line,
        );
    }
    scene.pop_layer();
}

/// One automation block's curve (TDD §12.1), and the handles it is edited by.
///
/// The picture is the editor: *"i want the automation graph to be a literal
/// graph drawn inside the clip."* The line goes through
/// `canvas::automation_polyline`, which evaluates the same function the
/// audio thread's values come from, and the handles are
/// `canvas::automation_block`'s — the same rectangles the pointer is tested
/// against, so what you see is what you can grab.
fn draw_automation_curve(
    scene: &mut Scene,
    theme: &Theme,
    grid: Rect,
    block: Rect,
    clip: &ClipInfo,
    ink: Color,
    lit: &[fontelle_types::PointId],
) {
    let points = crate::canvas::automation_polyline(block, clip.length, &clip.curve);
    if points.is_empty() {
        return;
    }
    let p = &theme.palette;
    let anatomy = crate::canvas::automation_block(block, clip);

    scene.push_layer(
        Fill::NonZero,
        BlendMode::default(),
        1.0,
        Affine::IDENTITY,
        &KRect::new(
            grid.x as f64,
            grid.y as f64,
            grid.right() as f64,
            grid.bottom() as f64,
        ),
    );

    // A guide at half, so a value can be read off the shape rather than
    // guessed. Faint: it is a ruling, not a curve.
    if anatomy.area.height >= 10.0 {
        let mid = crate::canvas::block_y_of_value(anatomy.area, 0.5);
        fill_rect(
            scene,
            Rect::new(anatomy.area.x, mid, anatomy.area.width, 1.0),
            p.grid_line,
        );
    }

    // A flat clip is a single horizontal run and still has to be visible: two
    // points at the same value make a line of zero height, which strokes to
    // nothing without a width.
    let mut path = BezPath::new();
    path.move_to((points[0].0 as f64, points[0].1 as f64));
    for (x, y) in points.iter().skip(1) {
        path.line_to((*x as f64, *y as f64));
    }
    scene.stroke(
        &Stroke::new(1.5),
        Affine::IDENTITY,
        ink.to_peniko(),
        None,
        &path,
    );

    // The points themselves, so a block with two of them reads as a segment
    // you could grab rather than as a rule drawn across the clip. Skipped on a
    // block too small for them to be anything but noise. A selected point is
    // in the accent, as a selected note is.
    if block.height >= 12.0 {
        for (id, rect) in &anatomy.handles {
            let selected = lit.contains(id);
            let dot = if selected {
                rect.inset(1.0)
            } else {
                rect.inset(2.0)
            };
            fill_rect_rounded(
                scene,
                dot,
                dot.width / 2.0,
                if selected { p.accent } else { p.text },
            );
        }
    }
    scene.pop_layer();
}

/// The arrangement's bar ruler, with the marker and the playhead on it.
fn draw_timeline_ruler(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    chrome: &TimelineChrome<'_>,
) {
    let p = &theme.palette;
    let l = &chrome.layout;
    let v = &chrome.view;
    if l.ruler.is_empty() {
        return;
    }
    fill_rect(scene, l.ruler, p.panel_header);

    let bar = PPQN * Tick::from(chrome.beats_per_bar.max(1));
    let ticks = timeline_visible_ticks(v, l.grid);
    let label_every = label_stride(bar as f32 * v.pixels_per_tick);
    let mut tick = ticks.start - ticks.start.rem_euclid(bar);
    while tick < ticks.end {
        let x = timeline_tick_to_x(v, l.grid, tick).floor();
        if x >= l.grid.x {
            fill_rect(
                scene,
                Rect::new(x, l.ruler.y, 1.0, l.ruler.height).intersection(&l.ruler),
                p.grid_line_strong,
            );
            let number = tick / bar + 1;
            if let Some(stride) = label_every
                && labelled_bar(number, stride)
                && let Some(text) = labels.get(&number.to_string())
            {
                draw_text_clipped(
                    scene,
                    text,
                    l.ruler,
                    x + 3.0,
                    l.ruler.y + (l.ruler.height - text.height) / 2.0,
                    p.text_muted,
                );
            }
        }
        tick += bar;
    }

    // The selection on the ruler itself, where the drag that made it was:
    // a solid strip, so it reads as a thing you can take hold of again.
    if let Some((from, to)) = chrome.loop_range {
        let x0 = timeline_tick_to_x(v, l.grid, from).max(l.grid.x);
        let x1 = timeline_tick_to_x(v, l.grid, to).min(l.grid.right());
        let strip = Rect::new(
            x0,
            l.ruler.y + 2.0,
            (x1 - x0).max(0.0),
            (l.ruler.height - 4.0).max(0.0),
        )
        .intersection(&l.ruler);
        if !strip.is_empty() {
            fill_rect_rounded(scene, strip, 2.0, p.selection);
            fill_rect(
                scene,
                Rect::new(strip.x, strip.y, 2.0, strip.height),
                p.accent,
            );
            fill_rect(
                scene,
                Rect::new(strip.right() - 2.0, strip.y, 2.0, strip.height),
                p.accent,
            );
        }
    }

    // The take as it is being recorded — *"i can see the clip being recorded
    // in the arrangement as im recording"*. A band one row tall at the foot of
    // the grid, which is where the row it becomes will be added, and drawn in
    // the record colour so that it cannot be mistaken for a clip that is
    // already there.
    if let Some((from, to)) = chrome.recording {
        let x0 = timeline_tick_to_x(v, l.grid, from).max(l.grid.x);
        let x1 = timeline_tick_to_x(v, l.grid, to).min(l.grid.right());
        let height = v.lane_height.min(l.grid.height);
        let band = Rect::new(
            x0,
            l.grid.bottom() - height,
            // Never nothing: the instant recording starts, the band is a
            // sliver rather than absent, or the first thing you see is the
            // window not reacting.
            (x1 - x0).max(2.0),
            height,
        )
        .intersection(&l.grid);
        if !band.is_empty() {
            fill_rect_rounded(scene, band, theme.metrics.corner_radius, p.meter_peak);
            stroke_rect_rounded(
                scene,
                band.inset(0.5),
                theme.metrics.corner_radius,
                1.0,
                p.accent,
            );
            if let Some(text) = labels.get(RECORDING) {
                draw_text_clipped(
                    scene,
                    text,
                    band,
                    band.x + 4.0,
                    band.y + (band.height - text.height) / 2.0,
                    p.panel,
                );
            }
        }
    }

    for (at, colour, flag) in [
        (chrome.marker_tick, p.accent, true),
        (chrome.playhead_tick, p.playhead, false),
    ] {
        let x = timeline_tick_to_x(v, l.grid, at);
        if x < l.grid.x || x > l.grid.right() {
            continue;
        }
        fill_rect(
            scene,
            Rect::new(
                x - if flag { 0.0 } else { 1.0 },
                l.ruler.y,
                if flag { 1.0 } else { 2.0 },
                l.ruler.height,
            ),
            colour,
        );
        if flag {
            fill_rect(
                scene,
                Rect::new(x, l.ruler.y, MARKER_FLAG_WIDTH, MARKER_FLAG_HEIGHT)
                    .intersection(&l.ruler),
                colour,
            );
        }
        // And down the grid, so a block's position against the playhead is
        // readable without looking back up at the ruler.
        fill_rect(
            scene,
            Rect::new(
                x - if flag { 0.0 } else { 1.0 },
                l.grid.y,
                if flag { 1.0 } else { 2.0 },
                l.grid.height,
            )
            .intersection(&l.grid),
            colour,
        );
    }

    fill_rect(
        scene,
        Rect::new(l.ruler.x, l.ruler.bottom() - 1.0, l.ruler.width, 1.0),
        p.border,
    );
}

/// A glyph centred in `rect`, or a glyph with `word` after it, the pair
/// centred — a chip that reads as one thing rather than an icon with a
/// caption bolted on. The glyph is the height of the text, so the row stays
/// the row's height.
fn draw_glyph_and_word(
    scene: &mut Scene,
    icon: crate::icon::Icon,
    word: Option<&TextLayout>,
    rect: Rect,
    ink: Color,
) {
    const GAP: f32 = 5.0;
    let side = (rect.height - 10.0).clamp(8.0, 16.0);
    let word_width = word.map_or(0.0, |text| text.width);
    let whole = side
        + if word.is_some() {
            GAP + word_width
        } else {
            0.0
        };
    let x = rect.x + ((rect.width - whole) / 2.0).max(2.0);
    draw_icon(
        scene,
        icon,
        Rect::new(x, rect.y + (rect.height - side) / 2.0, side, side),
        ink,
    );
    if let Some(text) = word {
        draw_text_clipped(
            scene,
            text,
            rect,
            x + side + GAP,
            rect.y + (rect.height - text.height) / 2.0,
            ink,
        );
    }
}

fn draw_browser(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    chrome: &BrowserChrome<'_>,
    status: &str,
) {
    let p = &theme.palette;
    let m = &theme.metrics;
    let l = &chrome.layout;

    // The mode switch, above everything: the search filters whichever list is
    // showing, so this is the thing that has to be read first.
    for (mode, rect) in l.tabs.iter().map(|(mode, rect)| (*mode, *rect)) {
        if rect.is_empty() {
            continue;
        }
        let on = chrome.mode == mode;
        let lit = chrome.hover == Some(BrowserHit::Mode(mode));
        fill_rect_rounded(
            scene,
            rect,
            m.corner_radius,
            if on {
                p.accent
            } else if lit {
                p.border
            } else {
                p.window
            },
        );
        // The glyph, and the word beside it only when the tab can hold
        // both (`tab_shows_word`): five words across the sidebar were five
        // clipped words. The tooltip says the word the rest of the time.
        let ink = if on { p.panel } else { p.text };
        let word = crate::canvas::tab_shows_word(rect.width)
            .then(|| labels.get(mode.label()))
            .flatten();
        draw_glyph_and_word(scene, mode.icon(), word, rect, ink);
    }

    // Which kind of file the Import tab is showing. A button each rather than
    // one that cycles, because every answer is worth being able to see and
    // press directly — a chip saying "MIDI" leaves "and what else?" unasked.
    // Read off the layout, which reads off `FolderKind::ALL`.
    for (kind, rect) in l.kinds.iter().map(|(kind, rect)| (*kind, *rect)) {
        if rect.is_empty() {
            continue;
        }
        let on = chrome.import_kind == kind;
        let lit = chrome.hover == Some(BrowserHit::Kind(kind));
        fill_rect_rounded(
            scene,
            rect,
            m.corner_radius,
            if on {
                p.accent
            } else if lit {
                p.border
            } else {
                p.panel
            },
        );
        let ink = if on { p.panel } else { p.text };
        draw_glyph_and_word(
            scene,
            crate::canvas::kind_icon(kind),
            labels.get(kind.tab_label()),
            rect,
            ink,
        );
    }

    // The two project-level actions, in the mode that has them.
    for (rect, caption, what, icon) in [
        (
            l.new_project,
            NEW_PROJECT,
            BrowserHit::NewProject,
            crate::icon::Icon::NewFile,
        ),
        (
            l.export,
            EXPORT,
            BrowserHit::Export,
            crate::icon::Icon::ArrowDown,
        ),
    ] {
        if rect.is_empty() {
            continue;
        }
        let lit = chrome.hover == Some(what);
        fill_rect_rounded(
            scene,
            rect,
            m.corner_radius,
            if lit { p.accent } else { p.border },
        );
        draw_captioned_icon(
            scene,
            labels,
            icon,
            caption,
            rect,
            if lit { p.panel } else { p.text },
        );
    }

    // The search field.
    if !l.search.is_empty() {
        fill_rect_rounded(scene, l.search, 3.0, p.window);
        if chrome.searching {
            scene.stroke(
                &Stroke::new(1.0),
                Affine::IDENTITY,
                p.accent.to_peniko(),
                None,
                &rounded(l.search, 3.0),
            );
        }
        let shown = if chrome.query.is_empty() {
            search_hint(chrome.mode)
        } else {
            chrome.query
        };
        if let Some(text) = labels.get(shown) {
            draw_text_clipped(
                scene,
                text,
                l.search,
                l.search.x + 6.0,
                l.search.y + (l.search.height - text.height) / 2.0,
                if chrome.query.is_empty() {
                    p.text_muted
                } else {
                    p.text
                },
            );
            if chrome.searching && !chrome.query.is_empty() {
                fill_rect(
                    scene,
                    Rect::new(
                        l.search.x + 7.0 + text.width,
                        l.search.y + 3.0,
                        1.0,
                        (l.search.height - 6.0).max(0.0),
                    )
                    .intersection(&l.search),
                    p.accent,
                );
            }
        }
    }

    let hover_file = match chrome.hover {
        Some(BrowserHit::File(index)) => Some(index),
        _ => None,
    };
    let hover_preset = match chrome.hover {
        Some(BrowserHit::Preset(index)) => Some(index),
        _ => None,
    };

    // Parallel to the settings list, empty in every other mode — so a
    // soundfont row draws no control.
    let settings_controls = chrome.settings_controls;
    let mut list = |area: Rect,
                    rows: &[(usize, Rect)],
                    entries: &[LibraryEntry],
                    selected: Option<usize>,
                    hovered: Option<usize>,
                    focused: Option<usize>| {
        if area.is_empty() {
            return;
        }
        fill_rect(scene, area, p.window);
        for (index, rect) in rows {
            let Some(entry) = entries.get(*index) else {
                continue;
            };
            if Some(*index) == selected {
                fill_rect(scene, *rect, p.selection);
                // A bar down the left edge as well as the wash, because the
                // selection colour alone is easy to miss on a list of forty.
                fill_rect(scene, Rect::new(rect.x, rect.y, 2.0, rect.height), p.accent);
            } else if hovered == Some(*index) {
                fill_rect(scene, *rect, p.row_accidental);
            }
            // Where the arrow keys are, drawn as an outline so it can sit on
            // top of the row that is playing without either mark hiding the
            // other — *"go through the selected instruments with arrow keys
            // after it being clicked on to focus it"*.
            if focused == Some(*index) {
                stroke_rect_rounded(scene, rect.inset(1.0), 2.0, 1.0, p.accent);
            }
            let detail = labels.get(&entry.detail);
            let detail_width = detail.map_or(0.0, |d| d.width);
            // A folder gets a glyph, and the row's text starts after it. A
            // list where a folder and a soundfont look the same is a list you
            // have to click to find out what you are looking at — which is the
            // report this whole feature answers.
            let glyph = match entry.kind {
                LibraryKind::Folder => Some(crate::icon::Icon::Folder),
                LibraryKind::Up => Some(crate::icon::Icon::ArrowUp),
                // A heading over a run of search hits: which soundfont they
                // are in. Given the folder glyph, because that is what it is
                // saying — "these came from in here".
                LibraryKind::Group => Some(crate::icon::Icon::Folder),
                LibraryKind::File => None,
            };
            // And a band behind it, so a list of hits from four soundfonts
            // reads as four groups rather than as one long list with some
            // grey rows in it.
            if entry.kind == LibraryKind::Group {
                fill_rect(scene, *rect, p.panel_header);
            }
            let indent = match glyph {
                Some(icon) => {
                    let side = (rect.height * 0.55).min(rect.width);
                    draw_icon(
                        scene,
                        icon,
                        Rect::new(
                            rect.x + 4.0,
                            rect.y + (rect.height - side) / 2.0,
                            side,
                            side,
                        ),
                        p.text_muted,
                    );
                    side + 6.0
                }
                None => 0.0,
            };
            if let Some(text) = labels.get(&entry.name) {
                // Clipped to the room left beside the size, not to the whole
                // row: a long soundfont name has to stop, not overprint.
                let column = name_column(rect.inset(0.0), detail_width);
                draw_text_clipped(
                    scene,
                    text,
                    column,
                    column.x + 5.0 + indent,
                    rect.y + (rect.height - text.height) / 2.0,
                    // A folder is a place, not a thing you can put on a
                    // channel; the muted ink says so without a second column.
                    if entry.kind == LibraryKind::File {
                        p.text
                    } else {
                        p.text_muted
                    },
                );
            }
            if let Some(detail) = detail {
                let x = rect.right() - detail.width - 5.0;
                draw_text_clipped(
                    scene,
                    detail,
                    *rect,
                    x,
                    rect.y + (rect.height - detail.height) / 2.0,
                    p.text_muted,
                );
            }
            // A settings row's control, over the right of the row: the groove a
            // number is dragged along, the pill a switch is, the caret a choice
            // drops from. The value text above sits in the gutter clear of it.
            if let Some(control) = settings_controls.get(*index) {
                draw_setting_control(scene, p, m, control, *rect);
            }
        }
    };
    list(
        l.files,
        &l.file_rows,
        chrome.files,
        chrome.selected_file,
        hover_file,
        chrome.focus_setting,
    );
    list(
        l.presets,
        &l.preset_rows,
        chrome.presets,
        chrome.selected_preset,
        hover_preset,
        chrome.focus_preset,
    );

    // The status line: where the bank is, or what just went wrong. Its own row
    // rather than a strip drawn over the bottom of the preset list, because a
    // folder path on top of a list of presets is two things at once.
    if !status.is_empty()
        && let Some(text) = labels.get(status)
    {
        draw_text_clipped(
            scene,
            text,
            l.status,
            l.status.x + 4.0,
            l.status.y + (l.status.height - text.height) / 2.0,
            p.text_muted,
        );
    }

    // And the two things a person needs to do with a folder: look in it, and
    // change which one it is. Pinned to the bottom of the panel, because on a
    // first run the bank is empty, the lists above are empty, and these are the
    // only things worth clicking.
    for (rect, caption, what, icon) in [
        (
            l.open_folder,
            if l.mode == crate::canvas::BrowserMode::Settings {
                OPEN_CONFIG_FOLDER
            } else {
                OPEN_FOLDER
            },
            BrowserHit::OpenFolder(l.mode),
            crate::icon::Icon::Folder,
        ),
        (
            l.choose_folder,
            CHOOSE_FOLDER,
            BrowserHit::ChooseFolder(l.mode),
            crate::icon::Icon::Route,
        ),
    ] {
        if rect.is_empty() {
            continue;
        }
        let lit = chrome.hover == Some(what);
        fill_rect_rounded(
            scene,
            rect,
            theme.metrics.corner_radius,
            if lit { p.accent } else { p.border },
        );
        draw_captioned_icon(
            scene,
            labels,
            icon,
            caption,
            rect,
            if lit { p.panel } else { p.text },
        );
    }
}

/// A settings row's control, drawn over the right of the row.
///
/// The value text (the row's `detail`) is already drawn in the gutter to the
/// right; this adds the thing you *touch* — so a number reads as a slider, a
/// choice as a drop-down, a switch as a switch, rather than as a value you
/// click to step. A heading and a button draw nothing extra: a button's whole
/// row is the target, and its caption is its `detail`.
fn draw_setting_control(
    scene: &mut Scene,
    p: &crate::theme::Palette,
    m: &crate::theme::Metrics,
    control: &crate::canvas::SettingControl,
    row: Rect,
) {
    use crate::canvas::SettingControl;
    let area = crate::canvas::setting_control_rect(row, m);
    if area.is_empty() {
        return;
    }
    match control {
        SettingControl::Heading | SettingControl::Button => {}
        SettingControl::Slider { fraction } => {
            let groove = crate::canvas::setting_slider_groove(area, m);
            if groove.is_empty() {
                return;
            }
            let radius = groove.height / 2.0;
            // The unfilled groove, then the fill up to the handle — the same
            // read as a fader: how far along says the value at a glance.
            fill_rect_rounded(scene, groove, radius, p.border);
            let handle_x = crate::canvas::setting_slider_x_of(area, *fraction);
            let filled = Rect::new(
                groove.x,
                groove.y,
                (handle_x - groove.x).max(0.0),
                groove.height,
            );
            if !filled.is_empty() {
                fill_rect_rounded(scene, filled, radius, p.accent);
            }
            // A grip at the handle, tall enough to aim at — a groove alone is a
            // reading, and this says it is a thing you drag.
            let grip_w = 4.0_f32.min(area.width);
            let grip_h = (row.height * 0.5).min(row.height);
            let grip = Rect::new(
                (handle_x - grip_w / 2.0).clamp(area.x, area.right() - grip_w),
                row.y + (row.height - grip_h) / 2.0,
                grip_w,
                grip_h,
            );
            fill_rect_rounded(scene, grip, grip_w / 2.0, p.text);
        }
        SettingControl::Switch { on } => {
            // A pill with the knob at one end or the other, lit when on. Just
            // left of the value gutter, where the "On"/"Off" is written, so the
            // two do not overlap. The groove's right edge is that divider.
            let divider = crate::canvas::setting_slider_groove(area, m).right();
            let track_w = (m.row_height * 1.4).min(area.width);
            let track_h = (m.row_height * 0.5).clamp(2.0, row.height);
            let track = Rect::new(
                (divider - track_w).max(area.x),
                row.y + (row.height - track_h) / 2.0,
                track_w,
                track_h,
            );
            let radius = track_h / 2.0;
            fill_rect_rounded(scene, track, radius, if *on { p.accent } else { p.border });
            let knob = track_h - 2.0;
            let knob_x = if *on {
                track.right() - knob - 1.0
            } else {
                track.x + 1.0
            };
            fill_rect_rounded(
                scene,
                Rect::new(knob_x, track.y + 1.0, knob, knob),
                knob / 2.0,
                if *on { p.panel } else { p.text_muted },
            );
        }
        SettingControl::Choice { .. } => {
            // A caret just left of the value gutter, saying the value beside it
            // drops down — the same mark the other drop-down rows here wear.
            let divider = crate::canvas::setting_slider_groove(area, m).right();
            let side = (row.height * 0.4).min(area.width);
            let caret = Rect::new(
                divider - side,
                row.y + (row.height - side) / 2.0,
                side,
                side,
            );
            draw_icon(scene, crate::icon::Icon::Chevron, caret, p.text_muted);
        }
    }
}

/// The placeholder in the empty search box, in one place for the same reason
/// [`ADD_CHANNEL`] is.
/// What the empty search box says, per mode.
///
/// **It said "search soundfonts" over the projects list**, which is the same
/// conflation the folder buttons and the panel heading had: three lists in one
/// panel, and one of them naming all three. The box does filter projects, so
/// the placeholder was not only wrong, it talked somebody out of using a
/// control that worked.
pub fn search_hint(mode: crate::canvas::BrowserMode) -> &'static str {
    match mode {
        crate::canvas::BrowserMode::Sounds => SEARCH_HINT,
        crate::canvas::BrowserMode::Projects => "search projects\u{2026}",
        crate::canvas::BrowserMode::Import => "search this folder\u{2026}",
        // There is no box in this mode — see `browser_layout_for` — but a
        // function over an enum answers for every case of it.
        crate::canvas::BrowserMode::Settings => "search settings\u{2026}",
        crate::canvas::BrowserMode::Presets => "search presets\u{2026}",
    }
}

pub const SEARCH_HINT: &str = "search soundfonts\u{2026}";

/// The browser footer's captions, in one place for the same reason.
pub const OPEN_FOLDER: &str = "Open folder";
/// The same button in the settings tab, where the folder it opens is the one
/// the settings file itself lives in — worth naming, since it is the only
/// folder in that mode and "Open folder" would be a question rather than a
/// caption.
pub const OPEN_CONFIG_FOLDER: &str = "Settings folder";
/// And on the button that makes a project, in the mode that has one.
pub const NEW_PROJECT: &str = "New";
/// And on the one that bounces it to a WAV.
pub const EXPORT: &str = "Export";
pub const CHOOSE_FOLDER: &str = "Change\u{2026}";

/// The strip down the left-hand side: a keyboard, or a list of names.
///
/// **Every key gets a band of the same height**, in both views. That is the fix
/// for *"single white keys wont be as tall as other white keys... the e key is
/// smaller"*: the first draft drew a real keyboard, with the naturals running
/// the full width and the accidentals sitting short on top of them, so the
/// white left over beside C# read as part of C's key and C looked half again as
/// tall as E. A side view of a piano does look like that. A grid whose rows are
/// all the same height does not, and the strip is a label for the grid.
///
/// The two views differ in what is *in* the band. The piano keeps the black
/// and white of a keyboard, which is how you find your place on a melodic
/// instrument; the list drops it for the name of whatever is on the key, which
/// is the only thing that helps on a drum kit — see [`KeyStyle`].
///
/// [`KeyStyle`]: crate::canvas::KeyStyle
#[allow(clippy::too_many_arguments)]
fn draw_keyboard(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    l: &RollLayout,
    v: &RollView,
    map: &crate::document::KeyMap,
    live: u128,
    style: crate::canvas::KeyStyle,
) {
    use crate::canvas::KeyStyle;
    let p = &theme.palette;
    if l.keys.is_empty() {
        return;
    }
    fill_rect(
        scene,
        l.keys,
        if style == KeyStyle::Piano {
            p.key_white
        } else {
            p.panel
        },
    );

    for key in visible_keys(v, l.grid) {
        // The same snapped rectangle the grid's rows and the notes use, so a
        // key lines up with the row it names at every zoom.
        let row = crate::canvas::key_row(v, l.grid, key.clamp(0, 127) as u8);
        let row = Rect::new(l.keys.x, row.y, l.keys.width, row.height).intersection(&l.keys);
        if row.is_empty() {
            continue;
        }
        let plays = map.plays(key.clamp(0, 127) as u8);
        // A key a MIDI keyboard is holding down. Lit in the accent, whether or
        // not the instrument plays it: a key that is down is a fact about the
        // hands, and a dead key that lights is how you find out it is dead.
        let down = (0..=127).contains(&key) && live & (1u128 << key) != 0;
        let accidental = is_accidental(key);

        // The band itself.
        let ink = match (down, plays, style, accidental) {
            (true, _, _, _) => p.accent,
            // The whole key, not a shade of it. A drum kit is mostly dead
            // keys, and the four that work have to be the thing you see.
            (_, false, _, _) => p.key_dead,
            (_, _, KeyStyle::Piano, true) => p.key_black,
            (_, _, KeyStyle::Piano, false) => p.key_white,
            // The list has no black keys: every row is a name on the same
            // ground, and the octave stripe below is what counts them.
            (_, _, KeyStyle::Names, _) => p.panel_header,
        };
        fill_rect(scene, row, ink);
        if style == KeyStyle::Piano && accidental && !down && plays {
            // The black key is still short, which keeps the picture a
            // keyboard — but the band it sits in is the same height as every
            // other band, which is the whole point.
            fill_rect(
                scene,
                Rect::new(row.x, row.y, row.width * 0.62, row.height).intersection(&l.keys),
                p.key_black,
            );
        }
        // A hairline under **every** key, not only the naturals: it is what
        // makes the rows read as even bands rather than as a keyboard.
        fill_rect(
            scene,
            Rect::new(row.x, row.bottom() - 1.0, row.width, 1.0).intersection(&l.keys),
            p.border,
        );

        // The name of the thing on this key, when the instrument has one —
        // `Snare` rather than `D1`, which is the whole reason a kit is
        // writable without playing every row to find out what it does. It
        // takes precedence over the octave name: on a key that has both,
        // "Snare" is the useful half. In the list view every row is named,
        // falling back to the note when the instrument has nothing to say.
        let named = map.name(key.clamp(0, 127) as u8);
        let octave = (key % 12 == 0).then(|| key_name(key));
        if key % 12 == 0 {
            fill_rect(
                scene,
                Rect::new(row.x, row.y, 3.0, row.height).intersection(&l.keys),
                p.accent,
            );
        }
        let caption = match style {
            KeyStyle::Piano => named.map(str::to_string).or(octave),
            KeyStyle::Names => named.map(str::to_string).or_else(|| Some(key_name(key))),
        };
        let Some(caption) = caption else {
            continue;
        };
        // A line box is a little taller than the ink in it, so a name is
        // allowed to be marginally taller than its row before it is dropped —
        // otherwise the keyboard is never labelled at any zoom anyone actually
        // uses.
        if let Some(text) = labels.get(&caption)
            && row.height >= text.height - 3.0
        {
            draw_text_clipped(
                scene,
                text,
                row,
                row.x + 6.0,
                row.y + (row.height - text.height) / 2.0,
                match (plays, style, accidental) {
                    (false, _, _) => p.text_muted,
                    (_, KeyStyle::Names, _) => p.text,
                    // On a black key the ink has to be the light one.
                    (_, KeyStyle::Piano, true) => p.key_white,
                    (_, KeyStyle::Piano, false) => p.key_black,
                },
            );
        }
    }
    // A border between the strip and the grid, so they read as two things.
    fill_rect(
        scene,
        Rect::new(l.keys.right() - 1.0, l.keys.y, 1.0, l.keys.height),
        p.border,
    );
}

/// The bar ruler across the top.
fn draw_ruler_strip(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    l: &RollLayout,
    v: &RollView,
    chrome: &RollChrome<'_>,
) {
    let p = &theme.palette;
    if l.ruler.is_empty() {
        return;
    }
    let beats_per_bar = chrome.beats_per_bar;
    fill_rect(scene, l.ruler, p.panel_header);

    let bar = PPQN * Tick::from(beats_per_bar.max(1));
    let ticks = visible_ticks(v, l.grid);
    // Numbered only when the numbers would not collide — at a zoom where two
    // bars are eight pixels apart, a wall of digits is less legible than none.
    let label_every = label_stride(bar as f32 * v.pixels_per_tick);
    let mut tick = ticks.start - ticks.start.rem_euclid(bar);
    while tick < ticks.end {
        let x = tick_to_x(v, l.grid, tick).floor();
        if x >= l.grid.x {
            fill_rect(
                scene,
                Rect::new(x, l.ruler.y, 1.0, l.ruler.height).intersection(&l.ruler),
                p.grid_line_strong,
            );
            // One-based, because bar 1 is where a musician says a song starts.
            let number = tick / bar + 1;
            if let Some(stride) = label_every
                && labelled_bar(number, stride)
                && let Some(text) = labels.get(&number.to_string())
            {
                draw_text_clipped(
                    scene,
                    text,
                    l.ruler,
                    x + 3.0,
                    l.ruler.y + (l.ruler.height - text.height) / 2.0,
                    p.text_muted,
                );
            }
        }
        tick += bar;
    }

    // The time selection, the same strip the arrangement's ruler draws, in
    // this clip's own ticks. Clipped to the grid: a selection that runs past
    // the clip's end is drawn to the edge and no further.
    if let Some((from, to)) = chrome.loop_range {
        let x0 = tick_to_x(v, l.grid, from).max(l.grid.x);
        let x1 = tick_to_x(v, l.grid, to).min(l.grid.right());
        let strip = Rect::new(
            x0,
            l.ruler.y + 2.0,
            (x1 - x0).max(0.0),
            (l.ruler.height - 4.0).max(0.0),
        )
        .intersection(&l.ruler);
        if !strip.is_empty() {
            fill_rect_rounded(scene, strip, 2.0, p.selection);
            fill_rect(
                scene,
                Rect::new(strip.x, strip.y, 2.0, strip.height),
                p.accent,
            );
            fill_rect(
                scene,
                Rect::new(strip.right() - 2.0, strip.y, 2.0, strip.height),
                p.accent,
            );
        }
        let band = Rect::new(x0, l.grid.y, (x1 - x0).max(0.0), l.grid.height).intersection(&l.grid);
        if !band.is_empty() {
            fill_rect(scene, band, p.selection);
        }
    }

    // The time marker's flag, and the playhead's, in the ruler where a person
    // looks for them. The flag is what makes the ruler visibly a thing you
    // click rather than a row of numbers.
    if let Some(tick) = chrome.marker_tick {
        let x = tick_to_x(v, l.grid, tick);
        if x >= l.grid.x && x <= l.grid.right() {
            fill_rect(
                scene,
                Rect::new(x, l.ruler.y, 1.0, l.ruler.height),
                p.accent,
            );
            fill_rect(
                scene,
                Rect::new(x, l.ruler.y, MARKER_FLAG_WIDTH, MARKER_FLAG_HEIGHT)
                    .intersection(&l.ruler),
                p.accent,
            );
        }
    }
    if let Some(tick) = chrome.playhead_tick {
        let x = tick_to_x(v, l.grid, tick);
        if x >= l.grid.x && x <= l.grid.right() {
            fill_rect(
                scene,
                Rect::new(x - 1.0, l.ruler.y, 2.0, l.ruler.height),
                p.playhead,
            );
        }
    }

    fill_rect(
        scene,
        Rect::new(l.ruler.x, l.ruler.bottom() - 1.0, l.ruler.width, 1.0),
        p.border,
    );
}

/// How many bars apart the ruler's numbers should be, or `None` when even the
/// widest spacing would crowd them.
///
/// Middle C is key 60 and is called C4, so the octave number is `key / 12 - 1`.
pub fn key_name(key: i32) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!(
        "{}{}",
        NAMES[key.rem_euclid(12) as usize],
        key.div_euclid(12) - 1
    )
}

/// Whether bar `number` (one-based) gets written on the ruler at `stride`.
///
/// Its own function because the obvious spelling is wrong: `number % stride ==
/// 1` reads correctly and is `0 == 1` for a stride of one, so the common case —
/// every bar numbered — numbered none of them. Found by looking at the window.
pub fn labelled_bar(number: i64, stride: i64) -> bool {
    stride > 0 && (number - 1).rem_euclid(stride) == 0
}

/// The room a row's first column has, given how wide its second one is.
///
/// A soundfont called `Bread Breads Distortion Guitar v2.1` in a 248-pixel
/// panel is the normal case, and clipping its name to the whole row draws it
/// straight through the size at the other end.
pub fn name_column(row: Rect, detail_width: f32) -> Rect {
    Rect::new(row.x, row.y, row.width - detail_width - ROW_GAP, row.height).clamped()
}

/// Between a row's two columns.
const ROW_GAP: f32 = 10.0;

/// The bar numbers to write, given how many pixels one bar is.
///
/// About thirty pixels is the narrowest a two- or three-digit number can be
/// written in without touching the next one.
pub fn label_stride(bar_px: f32) -> Option<i64> {
    if !bar_px.is_finite() || bar_px <= 0.0 {
        return None;
    }
    [1i64, 2, 4, 8, 16, 32]
        .into_iter()
        .find(|&stride| bar_px * stride as f32 >= 30.0)
}

/// Whether `key` is a black note. The pattern repeats every octave and C is 0.
fn is_accidental(key: i32) -> bool {
    matches!(key.rem_euclid(12), 1 | 3 | 6 | 8 | 10)
}

/// Draws a laid-out string with its top-left at `(x, y)`.
pub fn draw_text(scene: &mut Scene, text: &TextLayout, x: f32, y: f32, color: Color) {
    for run in &text.runs {
        scene
            .draw_glyphs(&run.font)
            .font_size(run.font_size)
            .brush(color.to_peniko())
            .transform(Affine::translate((x as f64, y as f64)))
            .draw(Fill::NonZero, run.glyphs.iter().copied());
    }
}

/// [`fill_rect`] with rounded corners — every chip, switch and button here.
/// An outline around a rounded rectangle, drawn *inside* its own edge.
///
/// The `+` column and the options panel both need to read as "a thing with a
/// boundary" rather than as a patch of a slightly different background — a
/// panel with no border on this theme sits four steps from the window colour
/// and disappears.
/// The text cursor after a name being typed into.
///
/// A one-pixel bar, which is what the search box has drawn since it was
/// written — one shape for "the keyboard is going here", wherever it is.
/// The caret and the selection of an inline rename, over the name in
/// `field` whose text starts at `text_x`.
///
/// With no marks — a caller that knows a rename is on but not where the
/// caret is — the caret goes at the end of the text, which is where it was
/// always drawn before the marks existed. The selection is a wash over the
/// text rather than an inversion, the same as the name prompt's, and the
/// caret is not drawn while there is one: a caret inside a highlighted range
/// is two claims about where typing goes.
fn draw_rename_marks(
    scene: &mut Scene,
    theme: &Theme,
    field: Rect,
    text_x: f32,
    text_width: f32,
    marks: Option<RenameMarks>,
) {
    let p = &theme.palette;
    let Some(marks) = marks else {
        draw_caret(scene, p.accent, field, text_x + text_width);
        return;
    };
    if let Some((from, to)) = marks.selection {
        let wash = Rect::new(
            text_x + from,
            field.y + 2.0,
            (to - from).max(0.0),
            (field.height - 4.0).max(0.0),
        )
        .intersection(&field);
        fill_rect(scene, wash, p.selection);
        return;
    }
    if marks.caret_on {
        draw_caret(scene, p.accent, field, text_x + marks.caret_x);
    }
}

fn draw_caret(scene: &mut Scene, color: Color, field: Rect, x: f32) {
    // On the pixel grid: a one-pixel line at a measured, fractional x is
    // two half-covered columns on every rasteriser, and *which* two differs
    // between them — Metal's caret failed the headless test Vulkan's passed.
    // A caret is a pixel column, so it is put on one.
    let x = x.round();
    fill_rect(
        scene,
        Rect::new(x, field.y + 3.0, 1.0, (field.height - 6.0).max(0.0)).intersection(&field),
        color,
    );
}

fn stroke_rect_rounded(scene: &mut Scene, r: Rect, radius: f32, width: f32, color: Color) {
    if r.is_empty() || width <= 0.0 {
        return;
    }
    scene.stroke(
        &Stroke::new(width as f64),
        Affine::IDENTITY,
        color.to_peniko(),
        None,
        &rounded(r, radius),
    );
}

fn fill_rect_rounded(scene: &mut Scene, r: Rect, radius: f32, color: Color) {
    if r.is_empty() {
        return;
    }
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        color.to_peniko(),
        None,
        &rounded(r, radius),
    );
}

/// Draws text that must not run out of the rectangle it belongs to.
///
/// A soundfont called `Orchestral Strings Ensemble Legato.sf2` in a 248-pixel
/// panel is the normal case, not the edge case, and text spilling over the
/// panel next to it is the difference between a tool and a mock-up.
fn draw_text_clipped(
    scene: &mut Scene,
    text: &TextLayout,
    clip: Rect,
    x: f32,
    y: f32,
    color: Color,
) {
    if clip.is_empty() || text.is_empty() {
        return;
    }
    // Both axes. Only the horizontal one was checked, which is why a row
    // clipped to a few pixels tall still drew its caption at full height, on
    // top of the row above it and down into the panel below.
    let needs_clip = x < clip.x
        || x + text.width > clip.right()
        || y < clip.y
        || y + text.height > clip.bottom();
    if needs_clip {
        scene.push_layer(
            Fill::NonZero,
            BlendMode::default(),
            1.0,
            Affine::IDENTITY,
            &KRect::new(
                clip.x as f64,
                clip.y as f64,
                clip.right() as f64,
                clip.bottom() as f64,
            ),
        );
    }
    draw_text(scene, text, x, y, color);
    if needs_clip {
        scene.pop_layer();
    }
}

fn fill_rect(scene: &mut Scene, r: Rect, color: Color) {
    if r.is_empty() {
        return;
    }
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        color.to_peniko(),
        None,
        &KRect::new(r.x as f64, r.y as f64, r.right() as f64, r.bottom() as f64),
    );
}

fn rounded(r: Rect, radius: f32) -> RoundedRect {
    // A radius bigger than half the shorter side is not a rounder rectangle,
    // it is an undefined path.
    let radius = radius.min(r.width / 2.0).min(r.height / 2.0).max(0.0);
    RoundedRect::new(
        r.x as f64,
        r.y as f64,
        r.right() as f64,
        r.bottom() as f64,
        radius as f64,
    )
}

/// The size and colour space every target here agrees on.
const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

#[derive(Debug)]
pub enum RenderError {
    /// No GPU we can use. On a developer's machine this is a problem; in a
    /// container it is a reason to skip, which is why it is a value and not a
    /// panic.
    NoAdapter(String),
    Device(String),
    Render(String),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoAdapter(why) => write!(f, "no usable GPU adapter: {why}"),
            Self::Device(why) => write!(f, "the GPU device could not be opened: {why}"),
            Self::Render(why) => write!(f, "the frame could not be rendered: {why}"),
        }
    }
}

impl std::error::Error for RenderError {}

/// Renders scenes into memory, with no window and no surface.
///
/// This is to the window what `fontelle_app::render_offline` is to the audio
/// device: the same pipeline, driven without hardware attached, so the result
/// can be asserted on. It is the answer to the plan's §2.5 problem for the one
/// part of a GUI that genuinely is pixels.
pub struct Headless {
    context: RenderContext,
    dev_id: usize,
    renderer: Renderer,
}

impl Headless {
    pub fn new() -> Result<Self, RenderError> {
        // vello's own `RenderContext` rather than a hand-built instance: it
        // owns the wgpu version this crate is pinned to, and picking an adapter
        // is exactly the part worth not reimplementing.
        let mut context = RenderContext::new();
        let dev_id = block_on(context.device(None)).ok_or_else(|| {
            RenderError::NoAdapter("no adapter matched the default options".to_string())
        })?;

        let renderer = Renderer::new(
            &context.devices[dev_id].device,
            RendererOptions {
                use_cpu: false,
                antialiasing_support: vello::AaSupport::area_only(),
                num_init_threads: None,
                pipeline_cache: None,
            },
        )
        .map_err(|e| RenderError::Render(e.to_string()))?;

        Ok(Self {
            context,
            dev_id,
            renderer,
        })
    }

    /// Renders `scene` and returns tightly packed RGBA8, row-major from the
    /// top-left.
    pub fn render(
        &mut self,
        scene: &Scene,
        width: u32,
        height: u32,
        base: Color,
    ) -> Result<Vec<u8>, RenderError> {
        let device = &self.context.devices[self.dev_id].device;
        let queue = &self.context.devices[self.dev_id].queue;

        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fontelle-ui headless target"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());

        self.renderer
            .render_to_texture(
                device,
                queue,
                scene,
                &view,
                &RenderParams {
                    base_color: base.to_peniko(),
                    width,
                    height,
                    antialiasing_method: AaConfig::Area,
                },
            )
            .map_err(|e| RenderError::Render(e.to_string()))?;

        // Copies out of a texture want rows aligned to 256 bytes, so the
        // staging buffer is padded and the padding is dropped on the way out.
        let unpadded = width as usize * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
        let padded = unpadded.div_ceil(align) * align;

        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fontelle-ui headless readback"),
            size: (padded * height as usize) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("fontelle-ui headless copy"),
        });
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded as u32),
                    rows_per_image: None,
                },
            },
            size,
        );
        queue.submit([encoder.finish()]);

        let slice = staging.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| RenderError::Render(e.to_string()))?;

        let mapped = slice.get_mapped_range();
        let mut pixels = Vec::with_capacity(unpadded * height as usize);
        for row in 0..height as usize {
            pixels.extend_from_slice(&mapped[row * padded..row * padded + unpadded]);
        }
        drop(mapped);
        staging.unmap();

        Ok(pixels)
    }
}

/// Runs a future to completion on this thread.
///
/// `wgpu`'s setup calls are the only futures this crate has, and they resolve
/// on the first poll on native targets. A parking waker rather than a spin so
/// this is correct even where they do not, and hand-rolled rather than adding
/// an async runtime for three call sites — the same trade `fontelle_model`'s
/// hand-rolled date formatting makes.
pub(crate) fn block_on<F: std::future::Future>(future: F) -> F::Output {
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake, Waker};

    struct Unpark(std::thread::Thread);
    impl Wake for Unpark {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    // Safe: `future` lives on this stack frame for the whole loop and is never
    // moved after the first poll.
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::park(),
        }
    }
}

/// Flopsynth's window (`docs/flopsynth-plan.md` §8).
pub struct FlopsynthChrome<'a> {
    pub layout: crate::canvas::FlopsynthLayout,
    pub view: &'a crate::canvas::FlopsynthView,
    /// Which control the pointer is over, as `(card, param)`.
    pub hover: Option<(usize, usize)>,
    /// Which control is being dragged.
    pub active: Option<(usize, usize)>,
    /// How deep the newest route to each control is, for the controls
    /// something modulates: `(card, param)` to a bipolar depth. What draws the
    /// arc (§8.1 rule 6).
    pub modulated: Vec<((usize, usize), f32)>,
    /// A source badge being carried: which source, and where the pointer is.
    /// While one is in flight every control that could take it is lit.
    pub assigning: Option<(usize, (f32, f32))>,
    /// An effect card carried by its header (§8.5): the card, and the card
    /// it would land on if let go now — whose header is lit to say so.
    pub carrying_slot: Option<(usize, Option<usize>)>,
    /// Which controls could take it — the same list, worked out once by the
    /// host rather than asked per knob while a drag is running.
    pub destinations: Vec<(usize, usize)>,
    /// The Presets page's About column, line by line (`canvas::preset_about`).
    pub about: Vec<String>,
    /// Where the pointer is, for the rows of the Presets page that light up
    /// under it — the shelves and the presets, which the hit test already
    /// names and the renderer only has to ask about.
    pub hover_at: (f32, f32),
    /// Whether the Presets search box has the keyboard, so it can be drawn
    /// with the lit outline and a caret the way a focused field is.
    pub searching: bool,
    /// This frame of the sky through the canopy — see [`SkyFrame`]. `None`
    /// draws a still one.
    pub sky: Option<&'a SkyFrame>,
    /// Textures dropped into the skin folder (`skin.rs`); every surface is
    /// procedural without them.
    pub skin: Option<&'a crate::skin::Skin>,
}

/// What the Presets page's search box says.
///
/// A function rather than a `format!` at the call site, for
/// [`voice_count_label`]'s reason: the window shapes its text ahead of
/// drawing it and the two have to ask for the same string.
pub fn search_caption(query: &str) -> String {
    if query.is_empty() {
        PRESET_SEARCH_HINT.to_string()
    } else {
        format!("{query}{}", crate::canvas::NAME_CARET)
    }
}

/// The search box while it has the keyboard: always the caret, even on an empty
/// query, so a focused-but-empty box is not drawn as the dead hint the way an
/// unfocused one is.
pub fn focused_search_caption(query: &str) -> String {
    format!("{query}{}", crate::canvas::NAME_CARET)
}

/// The search box with nothing typed in it.
pub const PRESET_SEARCH_HINT: &str = "Search presets \u{2014} type to filter";
/// What the list says when the search leaves nothing.
pub const NO_PRESETS_MATCH: &str = "nothing matches";
/// The tag on a row that is the user's own preset.
pub const MINE_TAG: &str = "mine";

/// A polyline, clipped to the rectangle it belongs in.
///
/// Clipped rather than trusted: a curve is computed from numbers a person is
/// dragging, and one that ran outside its own card would draw over the card
/// next to it.
fn stroke_polyline(
    scene: &mut Scene,
    points: &[(f32, f32)],
    clip: Rect,
    width: f32,
    colour: crate::theme::Color,
) {
    if points.len() < 2 || clip.is_empty() {
        return;
    }
    let mut path = BezPath::new();
    path.move_to(Point::new(points[0].0 as f64, points[0].1 as f64));
    for (x, y) in &points[1..] {
        path.line_to(Point::new(*x as f64, *y as f64));
    }
    scene.push_layer(
        Fill::NonZero,
        BlendMode::default(),
        1.0,
        Affine::IDENTITY,
        &KRect::new(
            clip.x as f64,
            clip.y as f64,
            clip.right() as f64,
            clip.bottom() as f64,
        ),
    );
    scene.stroke(
        &Stroke::new(width as f64),
        Affine::IDENTITY,
        colour.to_peniko(),
        None,
        &path,
    );
    scene.pop_layer();
}

/// A polyline with a glow under it: the same line twice, wide and faint and
/// then thin and bright, which is what makes a curve read as *lit* rather
/// than drawn.
fn glow_polyline(scene: &mut Scene, points: &[(f32, f32)], clip: Rect, colour: Color) {
    stroke_polyline(scene, points, clip, 5.0, colour.with_alpha(0x38));
    stroke_polyline(scene, points, clip, 1.5, lighten(colour, 0.25));
}

/// The area under a polyline, down to the bottom of `clip`.
fn fill_under_polyline(scene: &mut Scene, points: &[(f32, f32)], clip: Rect, colour: Color) {
    if points.len() < 2 || clip.is_empty() {
        return;
    }
    let mut path = BezPath::new();
    path.move_to(Point::new(points[0].0 as f64, clip.bottom() as f64));
    for (x, y) in points {
        path.line_to(Point::new(*x as f64, *y as f64));
    }
    path.line_to(Point::new(
        points[points.len() - 1].0 as f64,
        clip.bottom() as f64,
    ));
    path.close_path();
    scene.push_layer(
        Fill::NonZero,
        BlendMode::default(),
        1.0,
        Affine::IDENTITY,
        &KRect::new(
            clip.x as f64,
            clip.y as f64,
            clip.right() as f64,
            clip.bottom() as f64,
        ),
    );
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        colour.to_peniko(),
        None,
        &path,
    );
    scene.pop_layer();
}

/// One colour part of the way to another.
fn mix(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color([
        mix(a.0[0], b.0[0]),
        mix(a.0[1], b.0[1]),
        mix(a.0[2], b.0[2]),
        mix(a.0[3], b.0[3]),
    ])
}

/// A rounded rectangle filled top to bottom from one colour to another.
fn fill_rect_vertical(scene: &mut Scene, r: Rect, radius: f32, top: Color, bottom: Color) {
    use vello::peniko::{Brush, Gradient};
    if r.is_empty() {
        return;
    }
    let gradient = Gradient::new_linear(
        Point::new(r.x as f64, r.y as f64),
        Point::new(r.x as f64, r.bottom() as f64),
    )
    .with_stops([(0.0, top.to_peniko()), (1.0, bottom.to_peniko())]);
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        &Brush::Gradient(gradient),
        None,
        &rounded(r, radius),
    );
}

/// A soft disc of colour fading to nothing at its edge — what a nebula is
/// made of, and the halo round a lit knob.
fn fill_glow(scene: &mut Scene, centre: (f32, f32), radius: f32, colour: Color, alpha: u8) {
    use vello::peniko::{Brush, Gradient};
    if radius <= 0.0 {
        return;
    }
    let gradient = Gradient::new_radial(Point::new(centre.0 as f64, centre.1 as f64), radius)
        .with_stops([
            (0.0, colour.with_alpha(alpha).to_peniko()),
            (1.0, colour.with_alpha(0).to_peniko()),
        ]);
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        &Brush::Gradient(gradient),
        None,
        &vello::kurbo::Circle::new((centre.0 as f64, centre.1 as f64), radius as f64),
    );
}

/// One card's picture.
///
/// Every one is drawn from numbers the *voice* reads — the oscillator's
/// current frame, the filter's own response, the envelope's own stages — for
/// the reason §8.1 rule 5 gives: a picture that lies is believed.
fn draw_flopsynth_picture(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    rect: Rect,
    picture: &crate::canvas::FlopsynthPicture,
    ink: Color,
) {
    use crate::canvas::{
        FlopsynthPicture, env_curve_points, response_curve_points, wave_curve_points,
    };
    let p = &theme.palette;
    if rect.is_empty() {
        return;
    }
    // The screen the picture is shown on: an inset display with a faint
    // raster (`bridge::draw_screen`) and a grid behind the curve, so it reads
    // as a scope rather than as another control.
    bridge::draw_screen(scene, theme, rect, ink);
    let inner = rect.inset(2.0);
    if inner.is_empty() {
        return;
    }
    let grid = p.border.with_alpha(0x60);
    for step in 1..4 {
        let x = inner.x + inner.width * step as f32 / 4.0;
        fill_rect(scene, Rect::new(x, inner.y, 1.0, inner.height), grid);
    }
    for step in [0.25, 0.75] {
        let y = inner.y + inner.height * step;
        fill_rect(scene, Rect::new(inner.x, y, inner.width, 1.0), grid);
    }

    match picture {
        FlopsynthPicture::None => {}
        FlopsynthPicture::Wave { points, position } => {
            // The zero line, so a wave's asymmetry is visible rather than
            // merely present.
            fill_rect(
                scene,
                Rect::new(inner.x, inner.y + inner.height * 0.5, inner.width, 1.0),
                p.border,
            );
            glow_polyline(scene, &wave_curve_points(inner, points), inner, p.accent);
            // Where the position knob sits across the table's frames, so the
            // picture says *which* frame it is showing.
            let x = inner.x + inner.width * position.clamp(0.0, 1.0);
            fill_rect(
                scene,
                Rect::new(x - 0.5, inner.y, 1.0, inner.height),
                p.playhead,
            );
        }
        FlopsynthPicture::Response {
            points,
            cutoff,
            resonance,
        } => {
            // Unity, so "this filter is doing nothing here" is readable.
            let unity = crate::canvas::RESPONSE_TOP_DB
                / (crate::canvas::RESPONSE_TOP_DB - crate::canvas::RESPONSE_BOTTOM_DB);
            fill_rect(
                scene,
                Rect::new(
                    inner.x,
                    inner.bottom() - inner.height * (1.0 - unity),
                    inner.width,
                    1.0,
                ),
                p.border,
            );
            let curve = response_curve_points(inner, points);
            fill_under_polyline(scene, &curve, inner, p.accent.with_alpha(0x2a));
            glow_polyline(scene, &curve, inner, p.accent);
            // The handle: where a drag on this picture is holding.
            let x = inner.x + inner.width * cutoff.clamp(0.0, 1.0);
            let y = inner.bottom() - inner.height * resonance.clamp(0.0, 1.0);
            fill_rect_rounded(
                scene,
                Rect::new(x - 3.0, y - 3.0, 6.0, 6.0),
                3.0,
                p.playhead,
            );
        }
        FlopsynthPicture::Envelope {
            attack,
            decay,
            sustain,
            release,
        } => {
            let points = env_curve_points(inner, *attack, *decay, *sustain, *release);
            fill_under_polyline(scene, &points, inner, p.modulation.with_alpha(0x2a));
            glow_polyline(scene, &points, inner, p.modulation);
            // A node at each corner, so the shape reads as four stages rather
            // than as one line.
            for (x, y) in points.iter().skip(1).take(points.len().saturating_sub(2)) {
                fill_rect_rounded(
                    scene,
                    Rect::new(x - 2.0, y - 2.0, 4.0, 4.0),
                    2.0,
                    p.playhead,
                );
            }
        }
        FlopsynthPicture::Sound {
            peaks,
            start,
            loop_region,
            name,
        } => {
            fill_rect(
                scene,
                Rect::new(inner.x, inner.y + inner.height * 0.5, inner.width, 1.0),
                p.border,
            );
            // The loop, shaded, under the shape.
            if let Some((from, to)) = loop_region {
                let x0 = inner.x + inner.width * from.clamp(0.0, 1.0);
                let x1 = inner.x + inner.width * to.clamp(0.0, 1.0);
                if x1 > x0 {
                    fill_rect(
                        scene,
                        Rect::new(x0, inner.y, x1 - x0, inner.height),
                        p.modulation.with_alpha(0x28),
                    );
                    for x in [x0, x1] {
                        fill_rect(
                            scene,
                            Rect::new(x - 0.5, inner.y, 1.0, inner.height),
                            p.modulation.with_alpha(0xa0),
                        );
                    }
                }
            }
            // The recording's shape: one column per peak pair, filled, the
            // way the arrangement draws a clip.
            if !peaks.is_empty() {
                let (top, bottom) = crate::canvas::sound_outline_points(inner, peaks);
                let mut path = BezPath::new();
                path.move_to(Point::new(top[0].0 as f64, top[0].1 as f64));
                for (x, y) in &top[1..] {
                    path.line_to(Point::new(*x as f64, *y as f64));
                }
                for (x, y) in bottom.iter().rev() {
                    path.line_to(Point::new(*x as f64, *y as f64));
                }
                path.close_path();
                scene.fill(
                    Fill::NonZero,
                    Affine::IDENTITY,
                    p.accent.with_alpha(0x70).to_peniko(),
                    None,
                    &path,
                );
                stroke_polyline(scene, &top, inner, 1.0, lighten(p.accent, 0.2));
                stroke_polyline(scene, &bottom, inner, 1.0, lighten(p.accent, 0.2));
                // Where the note starts.
                let x = inner.x + inner.width * start.clamp(0.0, 1.0);
                fill_rect(
                    scene,
                    Rect::new(x - 0.5, inner.y, 1.0, inner.height),
                    p.playhead,
                );
            }
            // Its name — or, with nothing dropped yet, what to do.
            if let Some(label) = labels.get_small(name) {
                draw_text_clipped(
                    scene,
                    label,
                    inner,
                    inner.x + 4.0,
                    inner.y + 2.0,
                    if peaks.is_empty() {
                        p.text_muted
                    } else {
                        p.text
                    },
                );
            }
        }
        FlopsynthPicture::Partials { bars, harmonics } => {
            // The harmonic grid: where a *table's* partials would be. What
            // the bars stand sharp of.
            let grid_ink = p.border.with_alpha(0x90);
            for h in 1..=*harmonics {
                let x = inner.x + inner.width * h as f32 / (*harmonics as f32 + 0.5);
                fill_rect(
                    scene,
                    Rect::new(x - 0.5, inner.y, 1.0, inner.height),
                    grid_ink,
                );
            }
            for (at, height) in bars {
                let x = inner.x + inner.width * at / (*harmonics as f32 + 0.5);
                if x < inner.x || x > inner.right() {
                    continue;
                }
                let h = inner.height * height.clamp(0.0, 1.0);
                fill_glow(scene, (x, inner.bottom() - h), 4.0, p.accent, 0x60);
                fill_rect(
                    scene,
                    Rect::new(x - 1.0, inner.bottom() - h, 2.0, h),
                    lighten(p.accent, 0.2),
                );
            }
        }
        FlopsynthPicture::Lfo { points, phase } => {
            fill_rect(
                scene,
                Rect::new(inner.x, inner.y + inner.height * 0.5, inner.width, 1.0),
                p.border,
            );
            let curve = wave_curve_points(inner, points);
            glow_polyline(scene, &curve, inner, p.modulation);
            // The dot: where the newest voice is in the cycle. The shape says
            // what the LFO is; this says what it is doing (§11, phase 6).
            //
            // Read off the drawn curve rather than computed a second way, so
            // it cannot sit off the line it is meant to be on — the same rule
            // the envelope's nodes follow.
            if !curve.is_empty() {
                let at = (phase.clamp(0.0, 1.0) * (curve.len() - 1) as f32).round() as usize;
                let (x, y) = curve[at.min(curve.len() - 1)];
                fill_rect_rounded(
                    scene,
                    Rect::new(x - 2.5, y - 2.5, 5.0, 5.0),
                    2.5,
                    p.playhead,
                );
            }
        }
    }
}

/// Flopsynth's window: cards laid out as the signal flows.
///
/// The drawing has almost nothing to decide — every rectangle and every
/// polyline comes from `canvas/flopsynth.rs`, which is pure and tested (§8.1
/// rule 10). What is left here is colour.
/// The page tabs, the badges and the matrix — everything on Flopsynth's window
/// that is not a card.
fn draw_flopsynth_chrome(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    chrome: &FlopsynthChrome<'_>,
) {
    let p = &theme.palette;
    let m = &theme.metrics;
    let l = &chrome.layout;

    // The tab strip: a head-up display floating on the sky, the page you
    // are on lit (`bridge::draw_hud_tab`).
    for (page, rect) in &l.tabs {
        bridge::draw_hud_tab(
            scene,
            theme,
            labels,
            *rect,
            page.label(),
            *page == chrome.view.page,
        );
    }

    // How many voices are sounding, at the right-hand end of the tab strip.
    //
    // A read-out and not a control, so it is text rather than a box:
    // polyphony is a number somebody *sets*, and the only way to know whether
    // sixteen is enough for what they are playing is to watch this (§11,
    // phase 6). Off the audio thread's own state — see `VoiceMeter`.
    if let Some((_, first)) = l.tabs.first()
        && let Some(text) = labels.get(&voice_count_label(chrome.view.voices))
    {
        let strip = Rect::new(l.body.x, first.y, l.body.width, first.height);
        draw_text_clipped(
            scene,
            text,
            strip,
            strip.right() - text.width - m.panel_padding,
            strip.y + (strip.height - text.height) / 2.0,
            match chrome.view.voices {
                0 => p.text_muted,
                _ => p.accent,
            },
        );
    }

    // The source badges. Violet, because they are the thing the violet arcs
    // come from — one ink for one idea (§8.1 rule 6).
    for (index, rect) in l.badges.iter().enumerate() {
        let Some(name) = chrome.view.sources.get(index) else {
            continue;
        };
        if rect.is_empty() {
            continue;
        }
        let carried = chrome.assigning.map(|(which, _)| which) == Some(index);
        fill_rect_rounded(
            scene,
            *rect,
            m.corner_radius,
            if carried { p.modulation } else { p.panel },
        );
        stroke_rect_rounded(scene, *rect, m.corner_radius, 1.0, p.modulation);
        if let Some(text) = labels.get(name) {
            draw_text_clipped(
                scene,
                text,
                *rect,
                rect.x + 5.0,
                rect.y + (rect.height - text.height) / 2.0,
                if carried { p.window } else { p.text },
            );
        }
    }

    // The matrix.
    if !l.matrix.is_empty() {
        fill_rect_rounded(scene, l.matrix, m.corner_radius, p.panel);
        stroke_rect_rounded(scene, l.matrix, m.corner_radius, 1.0, p.border);
        let header = Rect::new(
            l.matrix.x,
            l.matrix.y,
            l.matrix.width,
            crate::canvas::CARD_HEADER.min(l.matrix.height),
        );
        fill_rect(scene, header, p.panel_header);
        if let Some(text) = labels.get(MATRIX_HEADING) {
            draw_text_clipped(
                scene,
                text,
                header,
                header.x + 6.0,
                header.y + (header.height - text.height) / 2.0,
                p.text_muted,
            );
        }
        if chrome.view.routes.is_empty()
            && let Some(text) = labels.get(NO_ROUTES)
        {
            let body = Rect::new(
                l.matrix.x,
                header.bottom(),
                l.matrix.width,
                (l.matrix.height - header.height).max(0.0),
            );
            draw_text_clipped(scene, text, body, body.x + 8.0, body.y + 6.0, p.text_muted);
        }
        for (index, row) in l.routes.iter().enumerate() {
            let Some(route) = chrome.view.routes.get(index) else {
                continue;
            };
            for (rect, text) in [
                (row.source, route.source.as_str()),
                (row.destination, route.destination.as_str()),
            ] {
                if let Some(label) = labels.get(text) {
                    draw_text_clipped(
                        scene,
                        label,
                        rect,
                        rect.x + 2.0,
                        rect.y + (rect.height - label.height) / 2.0,
                        p.text,
                    );
                }
            }
            // The depth slider: a groove, a centre mark, and a bar growing
            // from the middle — bipolar, because a depth is.
            if !row.depth.is_empty() {
                fill_rect_rounded(scene, row.depth, 2.0, p.window);
                let middle = row.depth.x + row.depth.width / 2.0;
                fill_rect(
                    scene,
                    Rect::new(middle - 0.5, row.depth.y, 1.0, row.depth.height),
                    p.border,
                );
                let reach = row.depth.width / 2.0 * route.depth.clamp(-1.0, 1.0);
                let bar = Rect::new(
                    middle.min(middle + reach),
                    row.depth.y + 2.0,
                    reach.abs(),
                    (row.depth.height - 4.0).max(0.0),
                );
                fill_rect_rounded(scene, bar, 2.0, p.modulation);
            }
            if !row.remove.is_empty() {
                draw_icon(
                    scene,
                    crate::icon::Icon::Trash,
                    row.remove.inset(3.0),
                    p.text_muted,
                );
            }
        }
        // The thumb, when rows are hidden: the same mark the menus and the
        // preset list use for the same fact.
        if !l.matrix_scrollbar.is_empty() {
            fill_rect_rounded(scene, l.matrix_scrollbar, 2.0, p.text_muted);
        }
    }

    // The badge under the pointer while it is being carried, so a drag has
    // something in hand rather than only a cursor.
    if let Some((index, (x, y))) = chrome.assigning
        && let Some(name) = chrome.view.sources.get(index)
    {
        let rect = Rect::new(
            x + 8.0,
            y - crate::canvas::BADGE_H / 2.0,
            crate::canvas::BADGE_W,
            crate::canvas::BADGE_H,
        );
        fill_rect_rounded(scene, rect, m.corner_radius, p.modulation);
        if let Some(text) = labels.get(name) {
            draw_text_clipped(
                scene,
                text,
                rect,
                rect.x + 5.0,
                rect.y + (rect.height - text.height) / 2.0,
                p.window,
            );
        }
    }
}

/// What the voice read-out says.
///
/// A function rather than a `format!` at the call site, because the window
/// shapes its text ahead of drawing it and the two have to ask for the same
/// string — a caption shaped under one spelling and drawn under another is a
/// blank space.
pub fn voice_count_label(voices: usize) -> String {
    match voices {
        0 => "silent".to_string(),
        1 => "1 voice".to_string(),
        n => format!("{n} voices"),
    }
}

/// The matrix panel's heading, and what it says when it is empty.
pub const MATRIX_HEADING: &str = "Modulation matrix";
pub const NO_ROUTES: &str = "no routes \u{2014} drag a source onto a knob";

/// The arc round a control something modulates (§8.1 rule 6).
///
/// **Outside the groove**, in the band `canvas::ring_hit` answers to, so what
/// is drawn and what can be grabbed are the same ring.
///
/// It follows the knob's own 270° sweep — seven o'clock round to five o'clock
/// — rather than a full circle, for two reasons. It reads as *this* knob's
/// arc, and it never reaches the top of the cell: a cell is sixty pixels tall
/// and the caption sits in its first fourteen, so a ring that went over the
/// top struck through the word naming the knob it belonged to. A bipolar depth
/// grows from straight up, which is the middle of that sweep.
fn draw_modulation_ring(scene: &mut Scene, theme: &Theme, cell: Rect, depth: f32, lit: bool) {
    use vello::kurbo::{BezPath, Stroke};

    let knob = crate::canvas::flop_knob_rect(cell);
    if knob.is_empty() {
        return;
    }
    let radius = knob.width / 2.0 + crate::canvas::RING_GAP + crate::canvas::RING_BAND / 2.0;
    let (cx, cy) = (knob.x + knob.width / 2.0, knob.y + knob.height / 2.0);
    // `draw_knob`'s own angles, so the two are one control: `t` runs 0..1 over
    // the sweep and `0.5` is straight up.
    let point = |t: f32| {
        let a = (-0.75 + 1.5 * t) * std::f32::consts::PI;
        (
            (cx + radius * a.sin()) as f64,
            (cy - radius * a.cos()) as f64,
        )
    };
    let arc = |from: f32, to: f32| {
        let mut path = BezPath::new();
        const STEPS: usize = 32;
        for step in 0..=STEPS {
            let t = from + (to - from) * step as f32 / STEPS as f32;
            let at = point(t);
            if step == 0 {
                path.move_to(at);
            } else {
                path.line_to(at);
            }
        }
        path
    };

    // From straight up, out to the depth: right for a positive route and left
    // for a negative one, which is what bipolar means on a dial.
    let reach = 0.5 + depth.clamp(-1.0, 1.0) * 0.5;
    scene.stroke(
        &Stroke::new(2.0),
        vello::kurbo::Affine::IDENTITY,
        theme.palette.modulation.to_peniko(),
        None,
        &arc(0.5, reach),
    );
    if lit {
        // The whole sweep while a badge is over it: "this one will take it".
        scene.stroke(
            &Stroke::new(1.0),
            vello::kurbo::Affine::IDENTITY,
            theme.palette.modulation.to_peniko(),
            None,
            &arc(0.0, 1.0),
        );
    }
}

fn draw_flopsynth(scene: &mut Scene, theme: &Theme, labels: &Labels, chrome: &FlopsynthChrome<'_>) {
    let p = &theme.palette;
    let m = &theme.metrics;
    let l = &chrome.layout;
    if l.body.is_empty() {
        return;
    }

    // The bridge (`bridge.rs`): the hull first, the ground under everything,
    // then the canopy — the window onto the sky — with the page tabs floating
    // on it, then the consoles set into the hull. Nothing else in this
    // program draws a gradient, and this window is the one place the brief
    // asked for a *room* rather than a panel.
    let ground = Rect::new(
        l.whole.x - m.panel_margin,
        l.whole.y - m.panel_margin,
        l.whole.width + m.panel_margin * 2.0,
        l.whole.height + m.panel_margin * 2.0,
    );
    // The sky fills from the top of the window down to the canopy's foot:
    // the tab strip is drawn over it as a head-up display.
    let opening = Rect::new(
        ground.x + 2.0,
        ground.y + 2.0,
        ground.width - 4.0,
        (l.canopy.bottom() - ground.y - 2.0).max(0.0),
    );
    bridge::draw_hull(scene, theme, ground, opening.bottom() + 4.0, chrome.skin);
    bridge::draw_canopy(scene, theme, opening, chrome.sky, chrome.skin);
    draw_flopsynth_chrome(scene, theme, labels, chrome);
    if chrome.view.page == crate::canvas::FlopsynthPage::Presets {
        draw_flop_presets(scene, theme, labels, chrome);
        return;
    }
    // The `+ effect` button (§8.5): a chip in the accent, because it is the
    // one thing on a bare Effects page and has to read as the thing to press.
    if !l.add_effect.is_empty() {
        let button = l.add_effect;
        let lit = button.contains(chrome.hover_at.0, chrome.hover_at.1);
        fill_glow(
            scene,
            (
                button.x + button.width / 2.0,
                button.y + button.height / 2.0,
            ),
            button.width * 0.6,
            p.accent,
            if lit { 0x50 } else { 0x28 },
        );
        fill_rect_vertical(
            scene,
            button,
            button.height / 2.0,
            p.panel.with_alpha(0xe8),
            mix(p.panel, p.window, 0.35).with_alpha(0xe8),
        );
        stroke_rect_rounded(
            scene,
            button,
            button.height / 2.0,
            1.0,
            if lit {
                lighten(p.accent, 0.3)
            } else {
                p.accent
            },
        );
        if let Some(text) = labels.get(crate::canvas::ADD_EFFECT) {
            draw_text_clipped(
                scene,
                text,
                button,
                button.x + (button.width - text.width).max(0.0) / 2.0,
                button.y + (button.height - text.height) / 2.0,
                if lit { p.text } else { p.accent },
            );
        }
    }

    for (index, placed) in l.cards.iter().enumerate() {
        let Some(card) = chrome.view.cards.get(index) else {
            continue;
        };
        if placed.frame.is_empty() {
            continue;
        }
        let ink = card_ink(&card.group.name, p);
        // The console's lamp is on while a control in it is held or under
        // the pointer.
        let hot_card = chrome
            .active
            .or(chrome.hover)
            .is_some_and(|(which, _)| which == index);
        bridge::draw_console(
            scene,
            theme,
            labels,
            placed.frame,
            placed.header,
            &card.group.name,
            ink,
            hot_card,
            chrome.skin,
        );
        // A slot being carried: its own header dimmed, and the header of the
        // slot it would land on lit in its ink — the landing is said before
        // the button comes up, the way a carried badge lights the knobs.
        if let Some((carried, landing)) = chrome.carrying_slot {
            if carried == index {
                fill_rect_rounded(
                    scene,
                    placed.header,
                    m.corner_radius,
                    p.window.with_alpha(128),
                );
            } else if landing == Some(index) {
                stroke_rect_rounded(scene, placed.frame.inset(0.5), m.corner_radius, 2.0, ink);
                fill_rect_rounded(scene, placed.header, m.corner_radius, ink.with_alpha(90));
            }
        }
        if !placed.remove.is_empty() {
            let lit = placed.remove.contains(chrome.hover_at.0, chrome.hover_at.1);
            draw_icon(
                scene,
                crate::icon::Icon::Trash,
                placed.remove.inset(1.0),
                if lit { p.meter_peak } else { p.text_muted },
            );
        }
        draw_flopsynth_picture(scene, theme, labels, placed.picture, &card.picture, ink);

        for (param_index, cell) in &placed.cells {
            let Some(param) = card.group.params.get(*param_index) else {
                continue;
            };
            if cell.is_empty() {
                continue;
            }
            let hot = chrome.active == Some((index, *param_index));
            let lit = hot || chrome.hover == Some((index, *param_index));
            // The nameplate's chooser: a chip filling its cell, with no
            // caption — the nameplate is its caption.
            if crate::canvas::is_nameplate_control(param) {
                let chip = Rect::new(
                    cell.x,
                    cell.y + 2.0,
                    cell.width,
                    (cell.height - 4.0).max(0.0),
                );
                draw_flop_chip(scene, theme, labels, chip, &param.display, lit);
                continue;
            }
            if lit {
                fill_rect_rounded(scene, *cell, m.corner_radius, p.text.with_alpha(0x10));
            }
            // The arc **first**, under everything: it reaches a few pixels
            // past the knob (see `ring_hit`, which is the band it is drawn
            // in), and drawn last it struck through the caption above the
            // knob it belongs to.
            let depth = chrome
                .modulated
                .iter()
                .find(|(which, _)| *which == (index, *param_index))
                .map(|(_, depth)| *depth);
            let takes =
                chrome.assigning.is_some() && chrome.destinations.contains(&(index, *param_index));
            if depth.is_some() || takes {
                draw_modulation_ring(scene, theme, *cell, depth.unwrap_or(0.0), takes);
            }

            // The caption above, the control between, the read-out below —
            // the same three-band cell the general grid uses, at the small
            // size. A chooser carries its value inside its chip and has no
            // read-out under it.
            let control = crate::canvas::flop_knob_rect(*cell);
            let caption = Rect::new(cell.x, cell.y, cell.width, control.y - cell.y);
            let readout = Rect::new(
                cell.x,
                control.bottom(),
                cell.width,
                (cell.bottom() - control.bottom()).max(0.0),
            );
            if let Some(label) = labels.get_small(&param.label) {
                draw_text_clipped(
                    scene,
                    label,
                    caption,
                    caption.x + ((caption.width - label.width) / 2.0).max(1.0),
                    caption.y + (caption.height - label.height) / 2.0,
                    if lit { p.text } else { p.text_muted },
                );
            }
            match &param.kind {
                ParamKind::Knob => {
                    draw_flop_knob(
                        scene,
                        theme,
                        control,
                        param.value,
                        hot,
                        lit,
                        param.automated,
                        chrome.skin.and_then(|skin| skin.knob.as_ref()),
                    );
                    if let Some(label) = labels.get_small(&param.display) {
                        draw_text_clipped(
                            scene,
                            label,
                            readout,
                            readout.x + ((readout.width - label.width) / 2.0).max(1.0),
                            readout.y + (readout.height - label.height) / 2.0,
                            if hot { p.accent } else { p.text },
                        );
                    }
                }
                ParamKind::Switch => {
                    draw_flop_switch(scene, theme, control, cell.width, param.value >= 0.5, lit);
                    if let Some(label) = labels.get_small(&param.display) {
                        draw_text_clipped(
                            scene,
                            label,
                            readout,
                            readout.x + ((readout.width - label.width) / 2.0).max(1.0),
                            readout.y + (readout.height - label.height) / 2.0,
                            p.text,
                        );
                    }
                }
                ParamKind::Choice(_) => {
                    let inset = crate::canvas::CHIP_INSET;
                    let chip = Rect::new(
                        cell.x + inset,
                        control.y,
                        cell.width - inset * 2.0,
                        control.height,
                    );
                    draw_flop_chip(scene, theme, labels, chip, &param.display, lit);
                }
            }
        }
    }
}

/// The ink a card's family is drawn in — the rule under its name, and the
/// glow on its knobs' arcs. The three oscillators take the theme's three
/// ramps (§8.1 rule 2), so a route's source badge and its oscillator share a
/// colour without a new token; the filters the accent; everything that
/// *moves* something the modulation violet.
fn card_ink(name: &str, p: &crate::theme::Palette) -> Color {
    match name {
        "OSC A" => p.accent,
        "OSC B" => p.playhead,
        "OSC C" => p.note,
        n if n.starts_with("Filter") => p.accent,
        n if n.starts_with("ENV") || n.starts_with("LFO") || n.starts_with("Modulation") => {
            p.modulation
        }
        n if n.starts_with("FX") => p.meter,
        _ => p.text_muted,
    }
}

/// A knob on Flopsynth's window.
///
/// `draw_knob`'s geometry — the same 270-degree sweep, the same needle — with
/// a domed body and a glow under the value arc, so the arc reads as lit.
#[allow(clippy::too_many_arguments)]
fn draw_flop_knob(
    scene: &mut Scene,
    theme: &Theme,
    area: Rect,
    value: f32,
    hot: bool,
    lit: bool,
    automated: bool,
    cap: Option<&vello::peniko::ImageData>,
) {
    use vello::peniko::{Brush, Gradient};
    let p = &theme.palette;
    if area.is_empty() {
        return;
    }
    let radius = (area.width.min(area.height) / 2.0 - 1.0).max(2.0);
    let cx = area.x + area.width / 2.0;
    let cy = area.y + area.height / 2.0;
    let value = value.clamp(0.0, 1.0);

    let angle_of = |t: f32| (-0.75 + 1.5 * t) * std::f32::consts::PI;
    let point = |t: f32, r: f32| {
        let a = angle_of(t);
        ((cx + r * a.sin()) as f64, (cy - r * a.cos()) as f64)
    };
    let path_of = |from: f32, to: f32, r: f32| {
        let mut path = BezPath::new();
        const STEPS: usize = 24;
        for step in 0..=STEPS {
            let t = from + (to - from) * step as f32 / STEPS as f32;
            let at = point(t, r);
            if step == 0 {
                path.move_to(at);
            } else {
                path.line_to(at);
            }
        }
        path
    };

    if lit {
        fill_glow(scene, (cx, cy), radius * 2.2, p.accent, 0x48);
    }
    // The body: a dome, lit from above.
    let body = vello::kurbo::Circle::new((cx as f64, cy as f64), (radius - 2.0).max(1.0) as f64);
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        p.window.with_alpha(0xa0).to_peniko(),
        None,
        &vello::kurbo::Circle::new((cx as f64, cy as f64), (radius + 0.5) as f64),
    );
    let dome = Gradient::new_radial(
        Point::new(cx as f64, (cy - radius * 0.35) as f64),
        radius * 1.3,
    )
    .with_stops([
        (0.0, lighten(p.panel_header, 0.22).to_peniko()),
        (1.0, mix(p.panel_header, p.window, 0.5).to_peniko()),
    ]);
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        &Brush::Gradient(dome),
        None,
        &body,
    );
    // A cap from the skin folder, if there is one, scaled onto the dome and
    // clipped to it; the dome stays under it as the shading.
    if let Some(cap) = cap
        && cap.width > 0
        && cap.height > 0
    {
        let r = (radius - 2.0).max(1.0) as f64;
        scene.push_layer(
            Fill::NonZero,
            BlendMode::default(),
            1.0,
            Affine::IDENTITY,
            &body,
        );
        scene.draw_image(
            &vello::peniko::ImageBrush {
                image: cap.clone(),
                sampler: vello::peniko::ImageSampler::new()
                    .with_quality(vello::peniko::ImageQuality::Medium),
            },
            Affine::translate((cx as f64 - r, cy as f64 - r))
                * Affine::scale_non_uniform(
                    r * 2.0 / cap.width as f64,
                    r * 2.0 / cap.height as f64,
                ),
        );
        scene.pop_layer();
    }

    // The groove — the automation ring when a lane owns it, §12.2.
    let width = (radius * 0.24).clamp(1.5, 3.0);
    scene.stroke(
        &Stroke::new(width as f64),
        Affine::IDENTITY,
        if automated {
            p.param_automated
        } else {
            p.grid_line_strong
        }
        .to_peniko(),
        None,
        &path_of(0.0, 1.0, radius),
    );
    // The value arc, with its glow under it.
    if value > 0.0 {
        let ink = if hot {
            lighten(p.accent, 0.2)
        } else {
            p.accent
        };
        scene.stroke(
            &Stroke::new((width * 2.6) as f64),
            Affine::IDENTITY,
            ink.with_alpha(0x40).to_peniko(),
            None,
            &path_of(0.0, value, radius),
        );
        scene.stroke(
            &Stroke::new(width as f64),
            Affine::IDENTITY,
            ink.to_peniko(),
            None,
            &path_of(0.0, value, radius),
        );
    }
    // The pointer, so the value is readable at a glance rather than by
    // measuring an arc.
    let mut needle = BezPath::new();
    needle.move_to(point(value, radius * 0.3));
    needle.line_to(point(value, (radius - width - 1.0).max(radius * 0.5)));
    scene.stroke(
        &Stroke::new((width * 0.7).max(1.5) as f64),
        Affine::IDENTITY,
        if hot { lighten(p.accent, 0.4) } else { p.text }.to_peniko(),
        None,
        &needle,
    );
}

/// A chooser: a chip carrying its value, with a wedge that says it opens.
fn draw_flop_chip(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    chip: Rect,
    value: &str,
    lit: bool,
) {
    let p = &theme.palette;
    let m = &theme.metrics;
    if chip.is_empty() {
        return;
    }
    fill_rect_vertical(
        scene,
        chip,
        m.corner_radius,
        lighten(p.panel_header, 0.06),
        mix(p.panel_header, p.window, 0.4),
    );
    stroke_rect_rounded(
        scene,
        chip,
        m.corner_radius,
        1.0,
        if lit { p.accent } else { p.border },
    );
    // The text's room and the chevron's are the canvas's numbers, so what
    // `cell_span_measured` promises fits is what is drawn.
    use crate::canvas::{CHIP_CHEVRON, CHIP_TEXT_INDENT};
    if let Some(text) = labels.get_small(value) {
        draw_text_clipped(
            scene,
            text,
            Rect::new(
                chip.x,
                chip.y,
                (chip.width - CHIP_CHEVRON).max(0.0),
                chip.height,
            ),
            chip.x + CHIP_TEXT_INDENT,
            chip.y + (chip.height - text.height) / 2.0,
            p.text,
        );
    }
    // The wedge, two pixels in from the chip's edge.
    if chip.width >= CHIP_CHEVRON + CHIP_TEXT_INDENT {
        let colour = if lit { p.accent } else { p.text_muted };
        let x = chip.right() - 2.0 - CHEVRON_PX;
        let middle = chip.y + chip.height / 2.0;
        for step in 0..3 {
            let step = step as f32;
            fill_rect(
                scene,
                Rect::new(x + step, middle - 1.0 + step, 1.0, 1.0),
                colour,
            );
            fill_rect(
                scene,
                Rect::new(x + CHEVRON_PX - 1.0 - step, middle - 1.0 + step, 1.0, 1.0),
                colour,
            );
        }
    }
}

/// A switch: a pill with its dot at one end or the other.
fn draw_flop_switch(
    scene: &mut Scene,
    theme: &Theme,
    band: Rect,
    cell_width: f32,
    on: bool,
    lit: bool,
) {
    let p = &theme.palette;
    if band.is_empty() {
        return;
    }
    let width = (cell_width - 12.0).clamp(18.0, 34.0);
    let height = (band.height * 0.55).clamp(10.0, 16.0);
    let cx = band.x + band.width / 2.0;
    let pill = Rect::new(
        cx - width / 2.0,
        band.y + (band.height - height) / 2.0,
        width,
        height,
    );
    if on {
        fill_glow(
            scene,
            (cx, pill.y + height / 2.0),
            width * 0.9,
            p.accent,
            0x40,
        );
    }
    fill_rect_rounded(
        scene,
        pill,
        height / 2.0,
        if on {
            p.accent
        } else {
            mix(p.panel_header, p.window, 0.4)
        },
    );
    stroke_rect_rounded(
        scene,
        pill,
        height / 2.0,
        1.0,
        if lit {
            lighten(p.accent, 0.3)
        } else {
            p.border
        },
    );
    let dot = height - 4.0;
    let x = if on {
        pill.right() - 2.0 - dot
    } else {
        pill.x + 2.0
    };
    fill_rect_rounded(
        scene,
        Rect::new(x, pill.y + 2.0, dot, dot),
        dot / 2.0,
        if on { p.window } else { p.text_muted },
    );
}

/// The Presets page (§8.6): shelves, the list, and the loaded preset described.
fn draw_flop_presets(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    chrome: &FlopsynthChrome<'_>,
) {
    use crate::canvas::PresetsHit;
    let p = &theme.palette;
    let m = &theme.metrics;
    let page = &chrome.layout.presets;
    let view = chrome.view;
    let hover = crate::canvas::presets_hit(&chrome.layout, chrome.hover_at.0, chrome.hover_at.1);

    let pane = |scene: &mut Scene, rect: Rect| {
        if rect.is_empty() {
            return;
        }
        fill_rect_vertical(
            scene,
            rect,
            m.corner_radius + 2.0,
            p.panel.with_alpha(0xe8),
            mix(p.panel, p.window, 0.35).with_alpha(0xe8),
        );
        stroke_rect_rounded(scene, rect, m.corner_radius + 2.0, 1.0, p.border);
    };

    // The shelves.
    pane(scene, page.column);
    for (index, (shelf, rect)) in page.shelves.iter().enumerate() {
        if rect.is_empty() {
            continue;
        }
        let here = *shelf == view.browse.shelf;
        if here {
            fill_rect_rounded(scene, *rect, m.corner_radius, p.accent.with_alpha(0x40));
            fill_rect(
                scene,
                Rect::new(rect.x, rect.y + 3.0, 2.0, rect.height - 6.0),
                p.accent,
            );
        } else if hover == Some(PresetsHit::Shelf(index)) {
            fill_rect_rounded(scene, *rect, m.corner_radius, p.text.with_alpha(0x10));
        }
        if let Some(text) = labels.get(&shelf.label()) {
            draw_text_clipped(
                scene,
                text,
                *rect,
                rect.x + 8.0,
                rect.y + (rect.height - text.height) / 2.0,
                if here { p.text } else { p.text_muted },
            );
        }
    }

    // The list, with its search across the top.
    pane(
        scene,
        Rect::new(
            page.list.x,
            page.search.y - 4.0,
            page.list.width,
            page.list.bottom() - page.search.y + 4.0,
        ),
    );
    if !page.search.is_empty() {
        let typing = !view.browse.query.is_empty();
        // Lit whenever it has the keyboard, not only once something is typed —
        // that is what says a click landed and this is where keys now go.
        let lit = typing || chrome.searching;
        fill_rect_rounded(
            scene,
            page.search,
            m.corner_radius,
            p.window.with_alpha(0xd8),
        );
        stroke_rect_rounded(
            scene,
            page.search,
            m.corner_radius,
            1.0,
            if lit { p.accent } else { p.border },
        );
        // The hint when empty and unfocused; a caret when it has the keyboard,
        // so a focused-but-empty box is not indistinguishable from a dead one.
        let caption = if chrome.searching {
            focused_search_caption(&view.browse.query)
        } else {
            search_caption(&view.browse.query)
        };
        if let Some(text) = labels.get(&caption) {
            draw_text_clipped(
                scene,
                text,
                page.search,
                page.search.x + 8.0,
                page.search.y + (page.search.height - text.height) / 2.0,
                if lit { p.text } else { p.text_muted },
            );
        }
    }
    let current = view.bank.iter().position(|preset| {
        chrome
            .about
            .first()
            .is_some_and(|name| name.trim_end_matches('*') == preset.name)
    });
    for (which, rect) in &page.rows {
        if rect.is_empty() {
            continue;
        }
        let Some(preset) = view.bank.get(*which) else {
            continue;
        };
        let loaded = current == Some(*which);
        if preset.favourite {
            fill_rect_rounded(
                scene,
                *rect,
                m.corner_radius,
                p.accent.with_alpha(FAVORITE_WASH),
            );
        }
        if loaded {
            stroke_rect_rounded(scene, *rect, m.corner_radius, 1.0, p.accent);
        } else if matches!(hover, Some(PresetsHit::Row(w) | PresetsHit::Star(w)) if w == *which) {
            fill_rect_rounded(scene, *rect, m.corner_radius, p.text.with_alpha(0x10));
        }
        let star = Rect::new(
            rect.right() - crate::canvas::STAR_WIDTH,
            rect.y,
            crate::canvas::STAR_WIDTH,
            rect.height,
        );
        let (icon, ink) = if preset.favourite {
            (crate::icon::Icon::StarFilled, p.accent)
        } else {
            (crate::icon::Icon::Star, p.text_muted)
        };
        draw_icon(scene, icon, star.inset(STAR_INSET), ink);
        let mut right = star.x;
        // On a shelf that mixes categories, the row says which it is from —
        // quietly, at the right, where the eye goes after the name.
        if matches!(
            view.browse.shelf,
            crate::canvas::PresetShelf::All
                | crate::canvas::PresetShelf::Favourites
                | crate::canvas::PresetShelf::Mine
        ) && let Some(tag) = labels.get_small(&preset.category)
        {
            right -= tag.width + 10.0;
            draw_text_clipped(
                scene,
                tag,
                *rect,
                right,
                rect.y + (rect.height - tag.height) / 2.0,
                p.text_muted,
            );
        }
        if preset.origin == fontelle_types::PresetOrigin::User
            && let Some(tag) = labels.get_small(MINE_TAG)
        {
            right -= tag.width + 6.0;
            draw_text_clipped(
                scene,
                tag,
                *rect,
                right,
                rect.y + (rect.height - tag.height) / 2.0,
                p.text_muted,
            );
        }
        if let Some(text) = labels.get(&preset.name) {
            draw_text_clipped(
                scene,
                text,
                Rect::new(rect.x, rect.y, (right - rect.x - 4.0).max(0.0), rect.height),
                rect.x + 8.0,
                rect.y + (rect.height - text.height) / 2.0,
                if loaded { p.accent } else { p.text },
            );
        }
    }
    if page.rows.is_empty()
        && let Some(text) = labels.get_small(NO_PRESETS_MATCH)
    {
        draw_text_clipped(
            scene,
            text,
            page.list,
            page.list.x + 12.0,
            page.list.y + 8.0,
            p.text_muted,
        );
    }
    // The thumb, when there is more list than panel.
    let max = page.max_scroll();
    if max > 0.0 && page.content_height > 0.0 {
        let track = Rect::new(
            page.list.right() - 6.0,
            page.list.y + 4.0,
            4.0,
            page.list.height - 8.0,
        );
        let height =
            (track.height * (track.height / page.content_height).clamp(0.0, 1.0)).max(18.0);
        let y =
            track.y + (track.height - height).max(0.0) * (view.browse.scroll.clamp(0.0, max) / max);
        fill_rect_rounded(
            scene,
            Rect::new(track.x, y, track.width, height),
            2.0,
            p.text_muted.with_alpha(0xa0),
        );
    }

    // The loaded preset, described.
    pane(scene, page.about);
    if !page.about.is_empty() {
        let mut y = page.about.y + 10.0;
        for (index, line) in chrome.about.iter().enumerate() {
            if let Some(text) = labels.get(line) {
                draw_text_clipped(
                    scene,
                    text,
                    page.about,
                    page.about.x + 12.0,
                    y,
                    if index == 0 { p.text } else { p.text_muted },
                );
            }
            y += m.row_height;
            if index == 0 {
                fill_rect(
                    scene,
                    Rect::new(page.about.x + 12.0, y - 4.0, 28.0, 2.0),
                    p.accent,
                );
                y += 4.0;
            }
        }
    }
}

// ---------------------------------------------------------------- the tuner
//
// `docs/tune-plan.md` §7.1: "robotic and futuristic like the interior of a
// sci-fi spaceship". A builder cannot draw that from the adjective, so what
// follows is that adjective in primitives this file already has — a lattice
// instead of a sky, chamfered consoles instead of glass panes, scanlines over
// every display, and one big instrument in the middle.
//
// **No new palette tokens.** Flopsynth added one and paid a theme-format bump
// for it; this window is drawn from the inks the palette already has, so a
// person's own theme recolours it.

/// What the corrector's window draws.
pub struct TuneChrome<'a> {
    pub layout: crate::canvas::TuneLayout,
    pub view: &'a crate::canvas::TuneView,
    /// Which control the pointer is over, as `(card, param)`.
    pub hover: Option<(usize, usize)>,
    /// Which control is being dragged.
    pub active: Option<(usize, usize)>,
    /// Where the pointer is, for the key it is over and the frame under it.
    pub hover_at: (f32, f32),
}

/// How wide one cell of the lattice is.
const LATTICE: f32 = 28.0;
/// Where the horizon sits, as a share of the ground's height.
const HORIZON: f32 = 0.60;
/// The chamfer on a console's two cut corners.
const CHAMFER: f32 = 6.0;

/// The ink a card's family is drawn in — [`card_ink`]'s sibling, and the same
/// rule: the ones that *move* something take the modulation violet, the scale
/// and the correction take the accent, and the plumbing takes the muted text.
fn tune_ink(name: &str, p: &crate::theme::Palette) -> Color {
    match name {
        "Scale" | "Correction" => p.accent,
        "Vibrato" | "Voice" => p.modulation,
        "MIDI" => p.playhead,
        // The one card that is not about pitch at all: it colours what has
        // already been corrected, so it takes the ink this program uses for
        // level and drive rather than either of the pitch inks.
        "Character" => p.meter_peak,
        _ => p.text_muted,
    }
}

/// The ground: a near-black grade with a lattice over it and one horizon.
///
/// No stars — that is Flopsynth's sky, and the two windows should be tellable
/// apart at a glance.
fn draw_tune_ground(scene: &mut Scene, theme: &Theme, body: Rect) {
    let p = &theme.palette;
    let ground = Rect::new(
        body.x - theme.metrics.panel_margin,
        body.y - theme.metrics.panel_margin,
        body.width + theme.metrics.panel_margin * 2.0,
        body.height + theme.metrics.panel_margin * 2.0,
    );
    fill_rect_vertical(scene, ground, 0.0, mix(p.window, p.accent, 0.06), p.window);
    // The lattice: a grid of cells stroked faintly, with every other row
    // offset by half a cell so it reads as a honeycomb rather than as graph
    // paper.
    let ink = p.text.with_alpha(0x10);
    let mut y = ground.y;
    let mut row = 0usize;
    while y < ground.bottom() {
        fill_rect(scene, Rect::new(ground.x, y, ground.width, 1.0), ink);
        let offset = if row.is_multiple_of(2) {
            0.0
        } else {
            LATTICE / 2.0
        };
        let mut x = ground.x + offset;
        while x < ground.right() {
            fill_rect(scene, Rect::new(x, y, 1.0, LATTICE), ink);
            x += LATTICE;
        }
        y += LATTICE;
        row += 1;
    }
    // One horizon, lit from under.
    let horizon = ground.y + ground.height * HORIZON;
    fill_glow(
        scene,
        (ground.x + ground.width / 2.0, horizon + 24.0),
        ground.width * 0.7,
        p.accent,
        0x22,
    );
    fill_rect(
        scene,
        Rect::new(ground.x, horizon, ground.width, 1.0),
        p.accent.with_alpha(0x50),
    );
}

/// Scanlines over a display area: every third row, one pixel, in the window's
/// own colour. What makes a rectangle read as a screen.
fn draw_scanlines(scene: &mut Scene, theme: &Theme, area: Rect) {
    if area.is_empty() {
        return;
    }
    let ink = theme.palette.window.with_alpha(0x30);
    let mut y = area.y;
    while y < area.bottom() {
        fill_rect(scene, Rect::new(area.x, y, area.width, 1.0), ink);
        y += 3.0;
    }
}

/// One console: a chamfered pane with an edge light along its top and down
/// its left.
///
/// **Chamfered rather than rounded**, and cut at two corners rather than four:
/// a bevel that goes all the way round is a rounded rectangle with corners,
/// and the asymmetry is what makes a panel read as machined.
fn draw_tune_card(
    scene: &mut Scene,
    theme: &Theme,
    labels: &Labels,
    frame: Rect,
    header: Rect,
    name: &str,
) {
    let p = &theme.palette;
    let ink = tune_ink(name, p);
    let mut path = BezPath::new();
    let (x, y, r, b) = (
        frame.x as f64,
        frame.y as f64,
        frame.right() as f64,
        frame.bottom() as f64,
    );
    let c = CHAMFER as f64;
    path.move_to(Point::new(x + c, y));
    path.line_to(Point::new(r, y));
    path.line_to(Point::new(r, b - c));
    path.line_to(Point::new(r - c, b));
    path.line_to(Point::new(x, b));
    path.line_to(Point::new(x, y + c));
    path.close_path();
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        p.panel.with_alpha(0xd8).to_peniko(),
        None,
        &path,
    );
    scene.stroke(
        &Stroke::new(1.0),
        Affine::IDENTITY,
        p.border.to_peniko(),
        None,
        &path,
    );
    // The edge light: the top and the left, with a glow behind them.
    fill_glow(
        scene,
        (frame.x + 6.0, frame.y + 6.0),
        frame.width.min(frame.height) * 0.5,
        ink,
        0x1c,
    );
    fill_rect(
        scene,
        Rect::new(
            frame.x + CHAMFER,
            frame.y,
            (frame.width - CHAMFER).max(0.0),
            1.0,
        ),
        ink.with_alpha(0xb0),
    );
    fill_rect(
        scene,
        Rect::new(
            frame.x,
            frame.y + CHAMFER,
            1.0,
            (frame.height - CHAMFER * 2.0).max(0.0),
        ),
        ink.with_alpha(0x70),
    );
    if let Some(text) = labels.get_small(&name.to_uppercase()) {
        draw_text_clipped(
            scene,
            text,
            header,
            header.x + CHAMFER + 4.0,
            header.y + (header.height - text.height) / 2.0,
            ink,
        );
    }
}

/// The keyboard: two octaves, the enabled keys lit, the root ringed, the held
/// keys pulsing and the target brightest.
fn draw_tune_keyboard(scene: &mut Scene, theme: &Theme, labels: &Labels, chrome: &TuneChrome<'_>) {
    let p = &theme.palette;
    let view = chrome.view;
    let band = chrome.layout.keyboard;
    if band.is_empty() {
        return;
    }
    // Read once for the whole keyboard rather than per key: both are answers
    // about the newest frame, and asking twenty-four times would be twenty-four
    // chances to read a different one.
    let target = crate::canvas::target_class(view);
    let sung = crate::canvas::sung_class(view);
    fill_rect(scene, band, p.window.with_alpha(0x80));
    // The naturals first and the accidentals over them, which is the order a
    // keyboard is built in and the reverse of the order it is hit-tested in.
    for pass in [false, true] {
        for (index, key) in chrome.layout.keys.iter().enumerate() {
            let class = (index % 12) as u8;
            let accidental = key.height < band.height - 0.5;
            if accidental != pass || key.is_empty() {
                continue;
            }
            let enabled = view.mask & (1 << class) != 0;
            let held = view.held & (1 << class) != 0;
            let fill = if held {
                p.playhead.with_alpha(0xc0)
            } else if enabled {
                p.accent.with_alpha(if accidental { 0x40 } else { 0x60 })
            } else {
                p.panel.with_alpha(0x80)
            };
            fill_rect(scene, *key, fill);
            stroke_rect_rounded(scene, *key, 1.0, 1.0, p.border);
            if enabled {
                fill_rect(
                    scene,
                    Rect::new(key.x, key.y, key.width, 1.0),
                    p.accent.with_alpha(0xa0),
                );
            }
            // The root gets a ring inside it: the one key on the picture that
            // says what the song is in.
            if class == view.root % 12 {
                stroke_rect_rounded(scene, key.inset(3.0), 1.0, 1.0, p.accent);
            }
            // The note being **forced**, brightest of all (§7.4). Drawn as an
            // edge rather than a fill so it reads over whatever the key
            // already is — a target that is also held would otherwise be
            // indistinguishable from one that is merely held.
            if target == Some(class) {
                stroke_rect_rounded(scene, key.inset(1.0), 1.0, 2.0, p.meter_peak);
            }
            // And a dot where the singer actually is, which is the whole
            // point of putting a keyboard under a pitch trace: the distance
            // between the dot and the bright key *is* the correction.
            if sung == Some(class) {
                let r = (key.width * 0.16).clamp(2.0, 4.0);
                fill_rect_rounded(
                    scene,
                    Rect::new(
                        key.x + key.width / 2.0 - r,
                        key.bottom() - r * 3.0,
                        r * 2.0,
                        r * 2.0,
                    ),
                    r,
                    p.text_muted,
                );
            }
            // The note's name on the naturals, which is what turns a row of
            // boxes into something a person can pick a scale off without
            // counting from the left.
            if !accidental
                && let Some(text) = labels.get_small(fontelle_types::TUNE_ROOTS[class as usize])
                && text.width + 2.0 < key.width
            {
                draw_text_clipped(
                    scene,
                    text,
                    *key,
                    key.x + (key.width - text.width) / 2.0,
                    key.bottom() - text.height - 2.0,
                    if enabled {
                        p.text.with_alpha(0xc0)
                    } else {
                        p.text_muted.with_alpha(0x70)
                    },
                );
            }
            if key.contains(chrome.hover_at.0, chrome.hover_at.1) {
                fill_rect(scene, *key, p.text.with_alpha(0x18));
            }
        }
    }
    draw_scanlines(scene, theme, band);
}

/// The pitch trace: the rails, the sung line, the corrected line over it, and
/// the reticle at the right-hand edge.
/// One unbroken run of the trace: the sung line and the corrected line over
/// the same hops, which are drawn as a pair and must break as a pair.
type TraceRun = (Vec<(f32, f32)>, Vec<(f32, f32)>);

fn draw_tune_viewport(scene: &mut Scene, theme: &Theme, labels: &Labels, chrome: &TuneChrome<'_>) {
    let p = &theme.palette;
    let view = chrome.view;
    let area = chrome.layout.viewport;
    if area.is_empty() {
        return;
    }
    fill_rect(scene, area, p.window.with_alpha(0xa0));

    // The rails, one per enabled pitch class, the root's brighter.
    for (y, root) in crate::canvas::viewport_rails(area, view) {
        fill_rect(
            scene,
            Rect::new(area.x, y, area.width, 1.0),
            if root {
                p.accent.with_alpha(0x90)
            } else {
                p.accent.with_alpha(0x40)
            },
        );
    }

    let points = crate::canvas::viewport_points(area, view);

    // **MIDI**, under everything: the hops whose target came from a held key,
    // as a band in the playhead ink for as long as the key was down (§7.3).
    // Drawn first so the two pitch lines stay legible over it — the bars are
    // context for the trace, not a thing to read on their own.
    {
        let mut run: Option<(f32, f32)> = None;
        for point in points
            .iter()
            .chain(std::iter::once(&crate::canvas::TracePoint {
                x: area.right(),
                sung: None,
                corrected: None,
                locked: false,
                from_midi: false,
            }))
        {
            match (point.from_midi, run) {
                (true, None) => run = Some((point.x, point.x)),
                (true, Some((from, _))) => run = Some((from, point.x)),
                (false, Some((from, to))) => {
                    if to > from {
                        fill_rect(
                            scene,
                            Rect::new(from, area.y, to - from, area.height),
                            p.playhead.with_alpha(0x1e),
                        );
                    }
                    run = None;
                }
                (false, None) => {}
            }
        }
    }

    // The two lines. A gap in the trace is a gap in the line: a run of voiced
    // hops is one polyline and the next run is another, because joining them
    // would draw a note through the silence between two words.
    let mut sung: Vec<(f32, f32)> = Vec::new();
    let mut corrected: Vec<(f32, f32)> = Vec::new();
    let mut runs: Vec<TraceRun> = Vec::new();
    for point in &points {
        match (point.sung, point.corrected) {
            (Some(a), Some(b)) => {
                sung.push((point.x, a));
                corrected.push((point.x, b));
            }
            _ => {
                if sung.len() > 1 {
                    runs.push((std::mem::take(&mut sung), std::mem::take(&mut corrected)));
                } else {
                    sung.clear();
                    corrected.clear();
                }
            }
        }
    }
    if sung.len() > 1 {
        runs.push((sung, corrected));
    }
    for (sung, corrected) in &runs {
        stroke_polyline(scene, sung, area, 1.0, p.text_muted.with_alpha(0xa0));
        glow_polyline(scene, corrected, area, p.accent);
    }

    // The reticle: a bracket pair at the right-hand edge on the corrected
    // pitch, closed when the note is locked.
    if let Some(last) = points.last()
        && let Some(y) = last.corrected
    {
        let gap = if last.locked { 3.0 } else { 9.0 };
        let ink = p.meter_peak;
        for side in [-1.0f32, 1.0] {
            let x = area.right() - 14.0 + side * gap;
            fill_rect(scene, Rect::new(x, y - 6.0, 1.0, 12.0), ink);
            fill_rect(
                scene,
                Rect::new(x.min(x + side * 4.0), y - 6.0, 4.0, 1.0),
                ink,
            );
            fill_rect(
                scene,
                Rect::new(x.min(x + side * 4.0), y + 5.0, 4.0, 1.0),
                ink,
            );
        }
        // **How far off, and onto what** — "−23 ¢ → A3" (§7.3). Beside the
        // brackets, and to their left, because the brackets sit at the right
        // edge and there is nothing to the right of them. It is the one number
        // on the console somebody reads while singing.
        if let Some(caption) = crate::canvas::tune_readout(view)
            && let Some(text) = labels.get_small(&caption)
        {
            let x = (area.right() - 34.0 - text.width).max(area.x + 4.0);
            let y = (y - text.height / 2.0).clamp(area.y + 2.0, area.bottom() - text.height - 2.0);
            fill_rect_rounded(
                scene,
                Rect::new(x - 4.0, y - 2.0, text.width + 8.0, text.height + 4.0),
                3.0,
                p.window.with_alpha(0xb0),
            );
            draw_text_clipped(
                scene,
                text,
                area,
                x,
                y,
                if last.locked { p.meter_peak } else { p.text },
            );
        }
    }

    draw_scanlines(scene, theme, area);
    // The frame: a two-pixel inset rule in the accent, with a glow at each
    // corner — the console treatment, hardest, on the one thing that is an
    // instrument rather than a control.
    for corner in [
        (area.x, area.y),
        (area.right(), area.y),
        (area.x, area.bottom()),
        (area.right(), area.bottom()),
    ] {
        fill_glow(scene, corner, 40.0, p.accent, 0x2a);
    }
    stroke_rect_rounded(scene, area, 2.0, 2.0, p.accent.with_alpha(0x80));

    // What it costs and what is laying the grains, in the frame's top-right.
    let caption = crate::canvas::tune_caption(view);
    if let Some(text) = labels.get_small(&caption) {
        draw_text_clipped(
            scene,
            text,
            area,
            area.right() - text.width - 12.0,
            area.y + 8.0,
            p.text_muted,
        );
    }
}

/// The whole console.
fn draw_tune(scene: &mut Scene, theme: &Theme, labels: &Labels, chrome: &TuneChrome<'_>) {
    let p = &theme.palette;
    let m = &theme.metrics;
    let l = &chrome.layout;
    if l.body.is_empty() {
        return;
    }
    draw_tune_ground(scene, theme, l.body);
    draw_tune_viewport(scene, theme, labels, chrome);
    draw_tune_keyboard(scene, theme, labels, chrome);

    for (index, placed) in l.cards.iter().enumerate() {
        let Some(card) = chrome.view.cards.get(index) else {
            continue;
        };
        if placed.frame.is_empty() {
            continue;
        }
        draw_tune_card(
            scene,
            theme,
            labels,
            placed.frame,
            placed.header,
            &card.group.name,
        );
        for (param_index, cell) in &placed.cells {
            let Some(param) = card.group.params.get(*param_index) else {
                continue;
            };
            if cell.is_empty() {
                continue;
            }
            let hot = chrome.active == Some((index, *param_index));
            let lit = hot || chrome.hover == Some((index, *param_index));
            // The nameplate's chooser: a chip filling its cell, with no
            // caption — the nameplate is its caption.
            if crate::canvas::is_nameplate_control(param) {
                let chip = Rect::new(
                    cell.x,
                    cell.y + 2.0,
                    cell.width,
                    (cell.height - 4.0).max(0.0),
                );
                draw_flop_chip(scene, theme, labels, chip, &param.display, lit);
                continue;
            }
            if lit {
                fill_rect_rounded(scene, *cell, m.corner_radius, p.text.with_alpha(0x10));
            }
            let control = crate::canvas::flop_knob_rect(*cell);
            let caption = Rect::new(cell.x, cell.y, cell.width, control.y - cell.y);
            let readout = Rect::new(
                cell.x,
                control.bottom(),
                cell.width,
                (cell.bottom() - control.bottom()).max(0.0),
            );
            if let Some(label) = labels.get_small(&param.label) {
                draw_text_clipped(
                    scene,
                    label,
                    caption,
                    caption.x + ((caption.width - label.width) / 2.0).max(1.0),
                    caption.y + (caption.height - label.height) / 2.0,
                    if lit { p.text } else { p.text_muted },
                );
            }
            match &param.kind {
                ParamKind::Knob => {
                    draw_flop_knob(
                        scene,
                        theme,
                        control,
                        param.value,
                        hot,
                        lit,
                        param.automated,
                        None,
                    );
                    if let Some(label) = labels.get_small(&param.display) {
                        draw_text_clipped(
                            scene,
                            label,
                            readout,
                            readout.x + ((readout.width - label.width) / 2.0).max(1.0),
                            readout.y + (readout.height - label.height) / 2.0,
                            if hot { p.accent } else { p.text },
                        );
                    }
                }
                ParamKind::Switch => {
                    draw_flop_switch(scene, theme, control, cell.width, param.value >= 0.5, lit);
                    if let Some(label) = labels.get_small(&param.display) {
                        draw_text_clipped(
                            scene,
                            label,
                            readout,
                            readout.x + ((readout.width - label.width) / 2.0).max(1.0),
                            readout.y + (readout.height - label.height) / 2.0,
                            p.text,
                        );
                    }
                }
                ParamKind::Choice(_) => {
                    let inset = crate::canvas::CHIP_INSET;
                    let chip = Rect::new(
                        cell.x + inset,
                        control.y,
                        cell.width - inset * 2.0,
                        control.height,
                    );
                    draw_flop_chip(scene, theme, labels, chip, &param.display, lit);
                }
            }
        }
    }
}

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
    /// the window at all.
    pub rack: Option<RackChrome<'a>>,
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
    /// The hover tip, once the pointer has sat still long enough — what it
    /// says and where it goes. `None` for the great majority of frames.
    pub tooltip: Option<(&'a str, Rect)>,
    /// The right-click menu, while one is open. Drawn **last**, over
    /// everything, because that is what a menu is.
    pub menu: Option<&'a crate::canvas::ContextMenu>,
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
    /// Whether the search box has the keyboard, so the caret is drawn.
    pub searching: bool,
    /// What the pointer is over, so a button can light up.
    pub hover: Option<BrowserHit>,
    /// Which of the panel's two lists is showing.
    pub mode: crate::canvas::BrowserMode,
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
    /// Delete and Ctrl+B visibly belong to one canvas rather than to both.
    pub focused: bool,
    /// The controls across the top.
    pub toolbar: TimelineToolbar,
    /// Which tool is on, so its chip is lit.
    pub tool: crate::canvas::TimelineTool,
    /// Which one the pointer is over, so it lights before it is pressed.
    pub hover: Option<TimelineControl>,
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
}

/// Everything the piano roll draws from. All of it is read-only: the roll is a
/// view and never a mutator (INVARIANT 2).
pub struct RollChrome<'a> {
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
    /// Where the **time marker** is within this clip, or `None` when it is
    /// somewhere the clip does not cover. Play starts here.
    pub marker_tick: Option<Tick>,
    /// The selection box being dragged, if one is.
    pub marquee: Option<Rect>,
    pub hover: Option<RollControl>,
    /// The lane chip's menu, while it is open. Drawn last, over everything.
    pub lane_menu: Option<&'a crate::canvas::LaneMenu>,
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
    /// What the options column's output row says — worked out where the route
    /// names are, rather than in the drawing code.
    pub output_label: String,
    /// An insert being dragged up or down the chain: the slot it started in
    /// and the slot it is over.
    pub insert_drag: Option<(usize, usize)>,
    /// The output row's menu, while it is open, and the names its rows read.
    pub output_menu: Option<&'a crate::canvas::RouteMenu>,
    /// The send menu, likewise — a separate field because the two are over the
    /// same list and mean different things, and only one is ever open.
    pub send_menu: Option<&'a crate::canvas::RouteMenu>,
    /// And the menu of effects a track can be given, which is over a different
    /// list again.
    pub effect_menu: Option<&'a crate::canvas::EffectMenu>,
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

/// One automation clip, as the editor draws it.
pub struct AutomationChrome {
    pub layout: crate::canvas::AutomationLayout,
    pub view: crate::canvas::AutomationView,
    /// The curve, already turned into points.
    pub curve: Vec<(f32, f32)>,
    pub hover: Option<fontelle_types::PointId>,
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
    /// What the pointer is over, so the control under it can light up.
    pub hover: Option<TransportHit>,
    /// The **time marker**: where play starts and where a stop comes back to.
    pub marker_sample: i64,
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

    draw_transport_bar(scene, theme, &chrome.transport);

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
    draw_context_menu(scene, theme, chrome.labels, chrome.menu);
    draw_tooltip(scene, theme, chrome);
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
pub fn draw_editor_window(
    scene: &mut Scene,
    theme: &Theme,
    layout: &PanelLayout,
    labels: &Labels,
    title: &TextLayout,
    chrome: &EditorWindowChrome<'_>,
    // The right-click menu, when the one that is open belongs to *this*
    // window — a knob's, opened on the instrument editor.
    menu: Option<&crate::canvas::ContextMenu>,
) {
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

    match chrome {
        EditorWindowChrome::Instrument(Some(instrument)) => {
            draw_instrument(scene, theme, labels, instrument)
        }
        // A channel with no soundfont on it: say so, rather than leaving an
        // empty window that looks broken.
        EditorWindowChrome::Instrument(None) => {
            draw_label(scene, labels, NO_INSTRUMENT, layout.body, m, p.text_muted)
        }
        EditorWindowChrome::Effect(effect) => draw_effect(scene, theme, labels, effect),
        EditorWindowChrome::Insert(insert) => draw_instrument(scene, theme, labels, insert),
        EditorWindowChrome::Automation(automation) => {
            draw_automation(scene, theme, labels, automation)
        }
    }

    draw_context_menu(scene, theme, labels, menu);
}

/// Which panel a floating editor window is drawing, and what it needs.
pub enum EditorWindowChrome<'a> {
    Instrument(Option<InstrumentChrome<'a>>),
    /// An EQ, which draws a curve because a curve is what an EQ is.
    Effect(EffectChrome),
    /// Every other effect: a grid of knobs read off its own parameter list —
    /// see `fontelle_app::effect_panel`. The same chrome the instrument panel
    /// uses, because a grid of knobs is a grid of knobs.
    Insert(InstrumentChrome<'a>),
    Automation(AutomationChrome),
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
    fill_rect_rounded(scene, menu.frame.inset(1.0), m.corner_radius, p.panel_header);

    for (row, entry) in menu.rows.iter().zip(menu.entries.iter()) {
        if row.is_empty() {
            continue;
        }
        if entry.separator && row.y > menu.frame.y + 1.0 {
            fill_rect(
                scene,
                Rect::new(row.x, row.y, row.width, m.border_width.max(1.0)),
                p.border,
            );
        }
        let Some(text) = labels_get(labels, &entry.label) else {
            continue;
        };
        draw_text_clipped(
            scene,
            text,
            *row,
            row.x + crate::canvas::MENU_TEXT_INSET,
            row.y + (row.height - text.height) / 2.0,
            // A greyed entry is drawn in the same ink as a panel's border,
            // which is this theme's "there, and not for you".
            if entry.enabled { p.text } else { p.border },
        );
    }
}

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

/// The transport bar (item 7 of `docs/first-usable-plan.md`).
///
/// Drawn whether or not there is an engine behind it — a window that changes
/// shape when the sound card goes away is worse than one that says so — but
/// everything in it is muted and the playhead is absent when `view.available`
/// is false.
pub fn draw_transport_bar(scene: &mut Scene, theme: &Theme, chrome: &TransportChrome<'_>) {
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
            // Neither the ruler nor either of the document's boxes is a
            // glyph button, and each is drawn by its own code below.
            TransportHit::Scrub(_) | TransportHit::Tempo | TransportHit::Signature => {}
        }
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
    for (rect, text, what) in [
        (l.tempo, chrome.tempo, TransportHit::Tempo),
        (l.signature, chrome.signature, TransportHit::Signature),
    ] {
        if rect.is_empty() {
            continue;
        }
        let box_rect = rect.inset(2.0);
        fill_rect_rounded(scene, box_rect, m.corner_radius, p.window);
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
            ink,
        );
    }

    draw_ruler(scene, theme, l.ruler, view, chrome.marker_sample);
    draw_meter(scene, theme, l.meter, view, &chrome.meters);
}

/// The mixer: a fader, a pan, two switches and a meter per track (TDD §13).
/// The automation editor: a grid, the curve, and a handle per point.
fn draw_automation(scene: &mut Scene, theme: &Theme, labels: &Labels, chrome: &AutomationChrome) {
    let p = &theme.palette;
    let m = &theme.metrics;
    let grid = chrome.layout.grid;
    if grid.is_empty() {
        return;
    }
    fill_rect(scene, grid, p.panel);

    // A line at nothing, at half and at everything, so a value can be read off
    // the shape rather than guessed.
    for fraction in [0.0, 0.5, 1.0] {
        let y = crate::canvas::auto_y_of_value(grid, fraction);
        fill_rect(
            scene,
            Rect::new(grid.x, y, grid.width, m.border_width.max(1.0)),
            if fraction == 0.5 {
                p.grid_line_strong
            } else {
                p.grid_line
            },
        );
    }

    if chrome.curve.len() > 1 {
        let mut path = BezPath::new();
        path.move_to((chrome.curve[0].0 as f64, chrome.curve[0].1 as f64));
        for (x, y) in &chrome.curve[1..] {
            path.line_to((*x as f64, *y as f64));
        }
        scene.stroke(
            &Stroke::new(2.0),
            Affine::IDENTITY,
            p.accent.to_peniko(),
            None,
            &path,
        );
    }

    for (id, rect) in &chrome.layout.handles {
        let point = chrome.view.points.iter().find(|point| point.id == *id);
        let lit = chrome.hover == Some(*id) || point.is_some_and(|point| point.selected);
        fill_rect_rounded(
            scene,
            *rect,
            rect.width / 2.0,
            if lit { p.accent } else { p.note },
        );
    }

    draw_label(
        scene,
        labels,
        &chrome.view.title,
        chrome.layout.header,
        m,
        p.text,
    );
}

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
pub const EQ_AXIS_CAPTIONS: [&str; 9] = [
    "50", "100", "500", "1k", "5k", "10k", "+12", "0", "-12",
];

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
        path.move_to((
            chrome.spectrum[0].0 as f64,
            chrome.spectrum[0].1 as f64,
        ));
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
        path.move_to((
            chrome.band_curve[0].0 as f64,
            chrome.band_curve[0].1 as f64,
        ));
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
        let lit = selected || chrome.active == Some(handle.band) || chrome.hover == Some(handle.band);
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
        let caption = crate::canvas::eq_field_caption(*field, &chrome.config, chrome.layout.selected);
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
    draw_effect_menu(scene, theme, labels, chrome);
}

/// The menu of effects a track can be given.
fn draw_effect_menu(scene: &mut Scene, theme: &Theme, labels: &Labels, chrome: &MixerChrome<'_>) {
    let Some(menu) = chrome.effect_menu else {
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
    for (kind, rect) in &menu.items {
        if rect.is_empty() {
            continue;
        }
        let Some(text) = labels_get(labels, kind.label()) else {
            continue;
        };
        draw_text_clipped(
            scene,
            text,
            *rect,
            rect.x + m.panel_padding.min(rect.width),
            rect.y + (rect.height - text.height) / 2.0,
            p.text,
        );
    }
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

    row_text(scene, &strip.name, options.title, options.title.x + 2.0, p.text);

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
        let landing = chrome
            .insert_drag
            .is_some_and(|(_, over)| over == row.slot);
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
            if insert.bypassed { p.text_muted } else { p.text },
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
                    if insert.bypassed { p.text_muted } else { p.text },
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
            if send.pre_fader { p.accent } else { p.text_muted },
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

    // The rack: a row per insert, in chain order, each with its bypass switch
    // at the left-hand end.
    for (slot, row) in layout.inserts.iter().enumerate() {
        let Some(insert) = strip.inserts.get(slot) else {
            continue;
        };
        let lit = hovering(MixerHit::Insert(layout.index, slot))
            || hovering(MixerHit::BypassInsert(layout.index, slot));
        fill_rect_rounded(
            scene,
            row.inset(1.0),
            m.corner_radius * 0.5,
            if lit { p.border } else { p.panel_header },
        );
        // A filled dot is in, a hollow one is out — readable without a legend
        // because it is the same shape a bypass switch has everywhere.
        let dot_size = (row.height * 0.4).min(5.0);
        let dot = Rect::new(
            row.x + 3.0,
            row.y + (row.height - dot_size) / 2.0,
            dot_size,
            dot_size,
        );
        fill_rect_rounded(
            scene,
            dot,
            dot_size / 2.0,
            if insert.bypassed {
                p.text_muted
            } else {
                p.accent
            },
        );
        if let Some(text) = labels_get(labels, &insert.label) {
            draw_text_clipped(
                scene,
                text,
                *row,
                row.x + 3.0 + dot_size + 3.0,
                row.y + (row.height - text.height) / 2.0,
                if insert.bypassed {
                    p.text_muted
                } else {
                    p.text
                },
            );
        }
    }
    if !layout.add.is_empty()
        && let Some(text) = labels_get(labels, ADD_INSERT)
    {
        let lit = hovering(MixerHit::AddInsert(layout.index));
        if lit {
            fill_rect_rounded(
                scene,
                layout.add.inset(1.0),
                m.corner_radius * 0.5,
                p.border,
            );
        }
        draw_text_clipped(
            scene,
            text,
            layout.add,
            layout.add.x + 3.0,
            layout.add.y + (layout.add.height - text.height) / 2.0,
            p.text_muted,
        );
    }

    if let Some(text) = labels_get(labels, &strip.name) {
        draw_text_clipped(
            scene,
            text,
            layout.name,
            layout.name.x + 2.0,
            layout.name.y + (layout.name.height - text.height) / 2.0,
            if hovering(MixerHit::Name(layout.index)) {
                p.text
            } else {
                p.text_muted
            },
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

    // The cut tool's stroke, over the notes it is about to cut and under the
    // chrome. A tool whose gesture leaves no mark is one you have to aim
    // blind, which on rows fourteen pixels apart is a guess.
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
            &Stroke::new(1.5),
            Affine::IDENTITY,
            p.meter_peak.to_peniko(),
            None,
            &line,
        );
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
    for (control, rect) in &chrome.toolbar.items {
        if rect.is_empty() {
            continue;
        }
        let on = match control {
            RollControl::Tool(tool) => *tool == chrome.tool,
            RollControl::Velocity => !chrome.layout.velocity.is_empty(),
            RollControl::Ghost => chrome.ghost_filter != GhostFilter::Off,
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
            RollControl::Snap | RollControl::Lane | RollControl::Ghost | RollControl::Keys
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
            RollControl::Snap => chrome.snap.label(),
            RollControl::Lane => &lane_caption,
            RollControl::Ghost => &ghost_caption,
            // The chip says which view the strip is in, the way the snap chip
            // says which division is on.
            RollControl::Keys => chrome.key_style.label(),
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

fn draw_rack(scene: &mut Scene, theme: &Theme, labels: &Labels, chrome: &RackChrome<'_>) {
    let p = &theme.palette;
    let l = &chrome.layout;

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
                draw_caret(scene, p.accent, row.name, row.name.x + 7.0 + text.width);
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

    // The two chip rows, above the first heading. A chip rather than a knob,
    // because choosing one is a different gesture from turning one: a preset
    // writes the whole panel and then has nothing further to say (rule 10),
    // and a key names a track, which is not a number with a range.
    //
    // The **chosen** chip is filled rather than outlined. A preset row has no
    // chosen one — nothing stays selected once its knobs have been turned —
    // and the key row always does, because "no key" is one of them.
    for (chips, names, chosen, hovered) in [
        (
            &l.presets,
            &chrome.view.presets,
            None,
            chrome.hover_preset,
        ),
        (&l.keys, &chrome.view.keys, chrome.view.key, chrome.hover_key),
    ] {
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
            ParamKind::Knob => {
                draw_knob(scene, theme, control, param.value, hot, param.automated)
            }
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

fn draw_knob(
    scene: &mut Scene,
    theme: &Theme,
    area: Rect,
    value: f32,
    hot: bool,
    automated: bool,
) {
    let p = &theme.palette;
    if area.is_empty() {
        return;
    }
    let radius = (area.width.min(area.height) / 2.0 - 1.0).max(2.0);
    let cx = area.x + area.width / 2.0;
    let cy = area.y + area.height / 2.0;
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
    fill_rect(scene, chrome.layout.toolbar, p.panel_header);

    let selected = !chrome.selection.is_empty();
    for (control, rect) in &chrome.toolbar.items {
        if rect.is_empty() {
            continue;
        }
        // What this button would do, if pressed now.
        let live = match control {
            TimelineControl::Snap
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
            _ => false,
        };
        // The snap chip is a read-out and always carries its frame: a bare
        // word in a toolbar is not something anyone reads as a control. That
        // is exactly how the roll's snap chip came to be reported missing.
        if on || *control == TimelineControl::Snap || chrome.hover == Some(*control) {
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
            TimelineControl::Snap => chrome.view.snap.label(),
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
                draw_caret(scene, p.accent, header, header.x + 9.0 + text.width);
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

    // The clips.
    for clip in chrome.clips {
        if !lanes.contains(&clip.lane) {
            continue;
        }
        if clip.start + clip.length < ticks.start || clip.start > ticks.end {
            continue;
        }
        let block = clip_rect(v, l.grid, clip).intersection(&l.grid);
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
        fill_rect_rounded(
            scene,
            block.inset(1.0),
            3.0,
            if automation && !selected {
                p.panel_header
            } else {
                body
            },
        );
        if automation {
            draw_automation_curve(scene, theme, &block, clip, body);
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
            let first = block.x + 5.0;
            let y = block.y + (block.height - text.height) / 2.0;
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

    // The cut tool's stroke, over the clips it is about to divide and clipped
    // to the grid — the same mark the roll's makes, for the same reason.
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
            &Stroke::new(1.5),
            Affine::IDENTITY,
            p.meter_peak.to_peniko(),
            None,
            &line,
        );
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
    // And the focus edge, so Delete visibly belongs to one canvas.
    if chrome.focused && theme.metrics.border_width > 0.0 {
        scene.stroke(
            &Stroke::new(theme.metrics.border_width as f64),
            Affine::IDENTITY,
            p.accent.to_peniko(),
            None,
            &rounded(chrome.panel.frame, theme.metrics.corner_radius),
        );
    }
}

/// One automation block's curve (TDD §12.1).
///
/// A polyline with a dot at each point — the same picture the editor draws,
/// small. Which is the point: *"a lane of them looks like a lane of empty
/// clips"* was true because an automation clip was drawn exactly like a note
/// clip, and the one thing an automation clip has to show is its shape.
fn draw_automation_curve(
    scene: &mut Scene,
    theme: &Theme,
    block: &Rect,
    clip: &ClipInfo,
    ink: Color,
) {
    let points = crate::canvas::automation_polyline(block.inset(1.0), clip.length, &clip.curve);
    if points.is_empty() {
        return;
    }
    let p = &theme.palette;

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
    // block too small for them to be anything but noise.
    if block.height >= 12.0 {
        for (x, y) in &points {
            fill_rect_rounded(
                scene,
                Rect::new(x - 1.5, y - 1.5, 3.0, 3.0),
                1.5,
                p.text,
            );
        }
    }
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
    for (mode, rect) in [
        (crate::canvas::BrowserMode::Sounds, l.sounds_tab),
        (crate::canvas::BrowserMode::Projects, l.projects_tab),
        (crate::canvas::BrowserMode::Settings, l.settings_tab),
    ] {
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
        if let Some(text) = labels.get(mode.label()) {
            draw_text_clipped(
                scene,
                text,
                rect,
                rect.x + ((rect.width - text.width) / 2.0).max(2.0),
                rect.y + (rect.height - text.height) / 2.0,
                if on { p.panel } else { p.text },
            );
        }
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

    let mut list = |area: Rect,
                    rows: &[(usize, Rect)],
                    entries: &[LibraryEntry],
                    selected: Option<usize>,
                    hovered: Option<usize>| {
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
        }
    };
    list(
        l.files,
        &l.file_rows,
        chrome.files,
        chrome.selected_file,
        hover_file,
    );
    list(
        l.presets,
        &l.preset_rows,
        chrome.presets,
        chrome.selected_preset,
        hover_preset,
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
        // There is no box in this mode — see `browser_layout_for` — but a
        // function over an enum answers for every case of it.
        crate::canvas::BrowserMode::Settings => "search settings\u{2026}",
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
    fill_rect(scene, l.keys, if style == KeyStyle::Piano { p.key_white } else { p.panel });

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
            KeyStyle::Names => named
                .map(str::to_string)
                .or_else(|| Some(key_name(key))),
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
fn draw_caret(scene: &mut Scene, color: Color, field: Rect, x: f32) {
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

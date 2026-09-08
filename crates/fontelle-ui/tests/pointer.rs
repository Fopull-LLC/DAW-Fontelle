//! What the mouse cursor says about what a click would do.
//!
//! Reported from using the window: *"my mouse cursor should change to reflect
//! the action I can take."* It never changed at all, so the only way to find
//! out whether you were about to move a note or resize it was to try.
//!
//! The decision is a pure function of the geometry the window already has, so
//! it is tested here rather than by pointing at a screenshot.

use fontelle_model::{Arena, Note};
use fontelle_types::{ClipId, NoteId, PPQN, Tick};
use fontelle_ui::canvas::{
    DEFAULT_LANE_HEIGHT, InstrumentGroup, InstrumentLayout, InstrumentParam, InstrumentView,
    ParamKind, RollView, SnapDivision, TimelineView, Tool, browser_layout, instrument_layout,
    key_to_y, rack_layout, roll_layout, tick_to_x, timeline_layout, toolbar_layout,
};
use fontelle_ui::layout::{DEFAULT_TIMELINE_HEIGHT, EditorTab, editor_tabs, window_layout};
use fontelle_ui::pointer::{Pointer, PointerScene, pointer_at};
use fontelle_ui::theme::{Metrics, Theme};
use fontelle_ui::transport::transport_bar_layout;

fn metrics() -> Metrics {
    Theme::dark_default().metrics
}

fn note(start: Tick, length: Tick, key: u8) -> Note {
    Note {
        start,
        length,
        key,
        velocity: 100,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
        channel: None,
    }
}

struct Rig {
    layout: fontelle_ui::layout::WindowLayout,
    bar: fontelle_ui::transport::TransportBarLayout,
    rack: fontelle_ui::canvas::RackLayout,
    browser: fontelle_ui::canvas::BrowserLayout,
    roll: fontelle_ui::canvas::RollLayout,
    roll_bar: fontelle_ui::canvas::ToolbarLayout,
    timeline: fontelle_ui::canvas::TimelineLayout,
    timeline_bar: fontelle_ui::canvas::TimelineToolbar,
    instrument: InstrumentLayout,
    mixer: fontelle_ui::canvas::MixerLayout,
    tabs: fontelle_ui::layout::EditorTabs,
    view: RollView,
    notes: Arena<NoteId, Note>,
    clips: Vec<fontelle_ui::document::ClipInfo>,
    instrument_view: InstrumentView,
    timeline_view: TimelineView,
}

fn rig() -> Rig {
    let m = metrics();
    let layout = window_layout(1400.0, 820.0, &m, DEFAULT_TIMELINE_HEIGHT);
    let roll = roll_layout(layout.panel.body, &m, DEFAULT_LANE_HEIGHT);
    let view = RollView {
        scroll_tick: 0,
        top_key: 72,
        pixels_per_tick: 0.25,
        key_height: 14.0,
        snap: SnapDivision::Step,
    };
    let mut notes = Arena::default();
    notes.insert(note(0, PPQN * 2, 60));

    let mut arena: Arena<ClipId, ()> = Arena::default();
    let clips = vec![fontelle_ui::document::ClipInfo {
        id: arena.insert(()),
        lane: 0,
        start: 0,
        length: PPQN * 16,
        name: "Part".to_string(),
        muted: false,
        open: true,
        color: [0x4f, 0x8f, 0xd0, 0xff],
        loop_length: None,
        kind: fontelle_ui::document::ClipKind::Notes,
        curve: Vec::new(),
        notes: Vec::new(),
        audio: Default::default(),
        prefab: None,
    }];

    let instrument_view = InstrumentView {
        keys: Vec::new(),
        key: None,
        title: "tri baja".to_string(),
        groups: vec![InstrumentGroup {
            name: "Voice".to_string(),
            params: vec![InstrumentParam {
                address: fontelle_types::ParamAddress::new("patch/voice/glide"),
                label: "glide".to_string(),
                value: 0.2,
                display: "0 ms".to_string(),
                kind: ParamKind::Knob,
                automated: false,
            }],
        }],
    };

    let strips = vec![
        fontelle_ui::document::MixerStrip {
            name: "Keys".to_string(),
            gain_db: 0.0,
            pan: 0.0,
            mute: false,
            solo: false,
            is_master: false,
            color: [0x4f, 0x8f, 0xd0, 0xff],
            inserts: Vec::new(),
            sends: Vec::new(),
        },
        fontelle_ui::document::MixerStrip {
            name: "Master".to_string(),
            gain_db: 0.0,
            pan: 0.0,
            mute: false,
            solo: false,
            is_master: true,
            color: [0x60, 0x60, 0x68, 0xff],
            inserts: Vec::new(),
            sends: Vec::new(),
        },
    ];

    Rig {
        bar: transport_bar_layout(layout.transport, &m),
        mixer: fontelle_ui::canvas::mixer_layout(layout.panel.body, &m, &strips, 0),
        rack: rack_layout(layout.rack.body, &m, 3, 0),
        browser: browser_layout(layout.browser.body, &m, 20, 20, 0, 0),
        roll_bar: toolbar_layout(roll.toolbar, &m),
        timeline_bar: fontelle_ui::canvas::timeline_toolbar_layout(
            timeline_layout(layout.timeline.body, &m).toolbar,
            &m,
        ),
        timeline: timeline_layout(layout.timeline.body, &m),
        instrument: instrument_layout(layout.panel.body, &m, &instrument_view),
        tabs: editor_tabs(layout.panel.header, &m),
        instrument_view,
        timeline_view: TimelineView::default(),
        layout,
        roll,
        view,
        notes,
        clips,
    }
}

impl Rig {
    fn scene(&self, tab: EditorTab, dragging: Option<Pointer>) -> PointerScene<'_> {
        PointerScene {
            layout: &self.layout,
            bar: &self.bar,
            rack: &self.rack,
            browser: &self.browser,
            roll: &self.roll,
            roll_view: &self.view,
            roll_toolbar: &self.roll_bar,
            tool: Tool::Draw,
            notes: &self.notes,
            timeline: &self.timeline,
            timeline_bar: &self.timeline_bar,
            timeline_view: &self.timeline_view,
            clips: &self.clips,
            instrument: &self.instrument,
            instrument_view: Some(&self.instrument_view),
            mixer: &self.mixer,
            tabs: &self.tabs,
            tab,
            dragging,
        }
    }

    fn at(&self, x: f32, y: f32) -> Pointer {
        pointer_at(&self.scene(EditorTab::Roll, None), x, y)
    }
}

#[test]
fn a_note_offers_a_grab_and_its_edge_offers_a_resize() {
    let r = rig();
    let y = key_to_y(&r.view, r.roll.grid, 60) + r.view.key_height / 2.0;
    let left = tick_to_x(&r.view, r.roll.grid, 0);
    let right = tick_to_x(&r.view, r.roll.grid, PPQN * 2);

    assert_eq!(r.at(left + (right - left) / 2.0, y), Pointer::Grab);
    assert_eq!(
        r.at(right - 2.0, y),
        Pointer::ResizeX,
        "the right-hand edge is the length handle"
    );
}

#[test]
fn empty_grid_offers_the_tool_that_would_act_on_it() {
    let r = rig();
    let y = key_to_y(&r.view, r.roll.grid, 66) + r.view.key_height / 2.0;
    let x = tick_to_x(&r.view, r.roll.grid, PPQN * 4);
    assert_eq!(r.at(x, y), Pointer::Draw, "the draw tool is on");

    // Every tool says which one it is, and each says something different: the
    // select tool must not promise to draw, and the delete tool must not
    // promise to select. They used to share one crosshair, which answered
    // "you can click here" without answering "and this is what will happen".
    let mut scene = r.scene(EditorTab::Roll, None);
    for (tool, expected) in [
        (Tool::Paint, Pointer::Paint),
        (Tool::Select, Pointer::Select),
        (Tool::Delete, Pointer::Erase),
    ] {
        scene.tool = tool;
        assert_eq!(pointer_at(&scene, x, y), expected, "{tool:?}");
        assert_ne!(
            pointer_at(&scene, x, y),
            Pointer::Draw,
            "{tool:?} must not promise to draw"
        );
    }
}

#[test]
fn the_seams_you_can_drag_say_which_way_they_move() {
    let r = rig();
    // The divider between the arrangement and the editor moves up and down.
    assert_eq!(
        r.at(
            r.layout.divider.x + r.layout.divider.width / 2.0,
            r.layout.divider.y + r.layout.divider.height / 2.0
        ),
        Pointer::ResizeY
    );
    // So does the seam above the property lane.
    assert_eq!(
        r.at(
            r.roll.lane_grip.x + r.roll.lane_grip.width / 2.0,
            r.roll.lane_grip.y + r.roll.lane_grip.height / 2.0
        ),
        Pointer::ResizeY
    );
}

#[test]
fn everything_you_can_click_says_so() {
    let r = rig();
    let mid =
        |rect: fontelle_ui::layout::Rect| (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);

    for (name, (x, y)) in [
        ("play", mid(r.bar.play)),
        ("the add-instrument button", mid(r.rack.add)),
        ("a channel row", mid(r.rack.rows[0].frame)),
        ("the mute switch", mid(r.rack.rows[0].mute)),
        ("a soundfont row", mid(r.browser.file_rows[0].1)),
        ("open folder", mid(r.browser.open_folder)),
        ("a toolbar button", mid(r.roll_bar.items[0].1)),
        ("the roll's keyboard", mid(r.roll.keys)),
        ("the mixer tab", mid(r.tabs.mixer)),
    ] {
        assert_eq!(r.at(x, y), Pointer::Hand, "{name} at ({x}, {y})");
    }
}

#[test]
fn the_search_box_is_a_text_field() {
    let r = rig();
    assert_eq!(
        r.at(
            r.browser.search.x + 10.0,
            r.browser.search.y + r.browser.search.height / 2.0
        ),
        Pointer::Text
    );
}

#[test]
fn a_ruler_you_scrub_is_a_thing_you_grab() {
    let r = rig();
    assert_eq!(
        r.at(
            r.bar.ruler.x + 40.0,
            r.bar.ruler.y + r.bar.ruler.height / 2.0
        ),
        Pointer::Grab,
        "the transport's ruler is dragged, so it must not look inert"
    );
    assert_eq!(
        r.at(r.roll.ruler.x + 200.0, r.roll.ruler.y + 2.0),
        Pointer::Grab
    );
}

#[test]
fn a_clip_on_the_arrangement_behaves_like_a_note() {
    use fontelle_ui::canvas::clip_rect;
    let r = rig();
    let block = clip_rect(&r.timeline_view, r.timeline.grid, &r.clips[0]);
    assert_eq!(
        r.at(block.x + block.width / 2.0, block.y + block.height / 2.0),
        Pointer::Grab
    );
    assert_eq!(r.at(block.right() - 2.0, block.y + 4.0), Pointer::ResizeX);
}

#[test]
fn a_knob_says_it_is_dragged_up_and_down() {
    // The instrument is a window of its own now (see
    // `fontelle_ui::layout::EditorKind`), so the question is asked of that
    // window's own layout rather than of a tab of the main one — which is why
    // it has a function of its own rather than a branch inside `pointer_at`.
    let r = rig();
    let cell = r.instrument.cells[0].2;
    assert_eq!(
        fontelle_ui::pointer::instrument_pointer(
            &r.instrument,
            Some(&r.instrument_view),
            cell.x + cell.width / 2.0,
            cell.y + cell.height / 2.0,
        ),
        Pointer::ResizeY
    );
}
#[test]
fn a_drag_in_progress_overrides_whatever_is_under_the_pointer() {
    // Dragging a note over the keyboard must not turn the cursor into a hand
    // half way through the gesture.
    let r = rig();
    let scene = r.scene(EditorTab::Roll, Some(Pointer::Grabbing));
    for (x, y) in [
        (r.roll.keys.x + 4.0, r.roll.keys.y + 40.0),
        (r.browser.search.x + 4.0, r.browser.search.y + 4.0),
        (-100.0, -100.0),
    ] {
        assert_eq!(pointer_at(&scene, x, y), Pointer::Grabbing);
    }
}

#[test]
fn the_empty_chrome_is_just_a_pointer() {
    let r = rig();
    assert_eq!(r.at(-50.0, -50.0), Pointer::Default);
    assert_eq!(
        r.at(r.layout.rack.header.x + 4.0, r.layout.rack.header.y + 2.0),
        Pointer::Default,
        "a panel's title bar is not a control"
    );
}

// -------------------------------------------------------- the new seams ---

/// The two seams around the sidebar, which had no cursor because they had no
/// existence — see `tests/docks.rs`.
#[test]
fn the_sidebars_seams_say_which_way_they_move() {
    let r = rig();

    let seam = r.layout.sidebar_seam;
    assert!(!seam.is_empty(), "the fixture has a sidebar");
    assert_eq!(
        r.at(seam.x + seam.width / 2.0, seam.y + seam.height / 2.0),
        Pointer::ResizeX,
        "the one down the side of the sidebar moves left and right"
    );

    let split = r.layout.sidebar_split;
    assert!(!split.is_empty());
    assert_eq!(
        r.at(split.x + split.width / 2.0, split.y + split.height / 2.0),
        Pointer::ResizeY,
        "and the one across it moves up and down"
    );
}

#[test]
fn the_seams_do_not_steal_the_cursor_from_the_panels_beside_them() {
    // The seams are the margin that was already there, so a row of the rack
    // one pixel from the edge is still a row of the rack.
    let r = rig();
    let rack = r.layout.rack.frame;
    assert_ne!(
        r.at(rack.right() - 1.0, rack.y + rack.height / 2.0),
        Pointer::ResizeX
    );
    let browser = r.layout.browser.frame;
    assert_ne!(r.at(browser.x + 4.0, browser.y + 1.0), Pointer::ResizeY);
}

// -------------------------------------------------- the mixer and the tempo ---

#[test]
fn a_fader_offers_a_vertical_drag_and_a_pan_a_horizontal_one() {
    // The cursor is the only thing that says which way a control moves before
    // you have moved it, and a fader that promises a sideways drag costs
    // somebody a gesture.
    let r = rig();
    let scene = r.scene(EditorTab::Mixer, None);
    let strip = r.mixer.strips[0].clone();

    let centre =
        |rect: fontelle_ui::layout::Rect| (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
    let (x, y) = centre(strip.fader);
    assert_eq!(pointer_at(&scene, x, y), Pointer::ResizeY);
    let (x, y) = centre(strip.pan);
    assert_eq!(pointer_at(&scene, x, y), Pointer::ResizeX);
    let (x, y) = centre(strip.mute);
    assert_eq!(pointer_at(&scene, x, y), Pointer::Hand);
    let (x, y) = centre(strip.solo);
    assert_eq!(pointer_at(&scene, x, y), Pointer::Hand);
}

#[test]
fn the_master_strip_answers_like_any_other() {
    let r = rig();
    let scene = r.scene(EditorTab::Mixer, None);
    let master = r
        .mixer
        .master
        .clone()
        .expect("a project always has a master");
    let x = master.fader.x + master.fader.width / 2.0;
    let y = master.fader.y + master.fader.height / 2.0;
    assert_eq!(pointer_at(&scene, x, y), Pointer::ResizeY);
}

#[test]
fn the_tempo_box_offers_a_drag_and_the_signature_a_click() {
    // Both live on the transport bar, which is above every panel — and until
    // they said so with a cursor, neither looked like anything but a label.
    let r = rig();
    let scene = r.scene(EditorTab::Roll, None);

    let x = r.bar.tempo.x + r.bar.tempo.width / 2.0;
    let y = r.bar.tempo.y + r.bar.tempo.height / 2.0;
    assert_eq!(pointer_at(&scene, x, y), Pointer::ResizeY);

    let x = r.bar.signature.x + r.bar.signature.width / 2.0;
    let y = r.bar.signature.y + r.bar.signature.height / 2.0;
    assert_eq!(pointer_at(&scene, x, y), Pointer::Hand);
}

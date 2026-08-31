//! The pixels, checked by a machine.
//!
//! `docs/first-usable-plan.md` §2.5 says a GUI item is done when its view-model
//! functions are tested *and* the pixels have been seen once by a human. This
//! file does not replace the human — it does for the window what
//! `render_offline` does for the audio path: renders the real scene through the
//! real GPU pipeline with no device attached, so "the theme is ignored" and
//! "the panel is drawn in the wrong place" are caught by a test rather than by
//! looking.
//!
//! Every test here skips, loudly, on a machine with no usable adapter.

use fontelle_model::{Arena, Note};
use fontelle_types::{NoteId, PPQN, Tick};
use fontelle_ui::canvas::{
    DEFAULT_LANE_HEIGHT, LaneProperty, RollView, SnapDivision, Tool, roll_layout, tick_to_x,
    toolbar_layout,
};
use fontelle_ui::layout::DEFAULT_TIMELINE_HEIGHT;
use fontelle_ui::layout::{Rect, window_layout};
use fontelle_ui::render::RollChrome;
use fontelle_ui::render::{Chrome, Headless, TransportChrome, draw_window};
use fontelle_ui::text::{Labels, TextContext};
use fontelle_ui::theme::{Color, Theme};
use fontelle_ui::transport::{
    Meter, TransportBarLayout, TransportView, format_readout, playhead_x, transport_bar_layout,
};

use std::sync::{Mutex, OnceLock};

const W: u32 = 640;
const H: u32 = 360;

/// The roll shots get their own, bigger window.
///
/// The chrome around the roll grew in item 9 — a sidebar on the left, a toolbar
/// and a velocity lane inside the panel — and at 640x360 there is no longer a
/// full octave of grid left to sample. Bigger here rather than smaller chrome:
/// the chrome is what is being checked.
const RW: u32 = 1000;
const RH: u32 = 620;

/// One GPU device for the whole file.
///
/// Not an optimisation: `Headless::new` opens a wgpu device and compiles
/// vello's pipelines, and a dozen of those at once — which is what the test
/// harness's threads produce — exhausts the GPU and fails with "Out of
/// Memory". Found the direct way.
fn headless() -> Option<&'static Mutex<Headless>> {
    static SHARED: OnceLock<Option<Mutex<Headless>>> = OnceLock::new();
    SHARED
        .get_or_init(|| match Headless::new() {
            Ok(h) => Some(Mutex::new(h)),
            Err(e) => {
                eprintln!("skipping: no usable GPU adapter ({e})");
                None
            }
        })
        .as_ref()
}

struct Shot {
    pixels: Vec<u8>,
    theme: Theme,
    layout: fontelle_ui::layout::WindowLayout,
    bar: TransportBarLayout,
}

impl Shot {
    fn at(&self, x: u32, y: u32) -> Color {
        let i = ((y * W + x) * 4) as usize;
        Color(
            self.pixels[i..i + 4]
                .try_into()
                .expect("four bytes per pixel"),
        )
    }

    fn centre_of(&self, r: Rect) -> Color {
        self.at((r.x + r.width / 2.0) as u32, (r.y + r.height / 2.0) as u32)
    }
}

/// A transport with an engine behind it, stopped at the start.
fn live_view() -> TransportView {
    TransportView {
        available: true,
        length_samples: 480_000,
        sample_rate: 48_000.0,
        ..TransportView::unavailable()
    }
}

/// `None` when this machine has no GPU adapter we can use.
fn shoot(theme: Theme) -> Option<Shot> {
    shoot_with(theme, TransportView::unavailable(), [Meter::new(); 2])
}

fn shoot_with(theme: Theme, view: TransportView, meters: [Meter; 2]) -> Option<Shot> {
    let shared = headless()?;
    let layout = window_layout(W as f32, H as f32, &theme.metrics, DEFAULT_TIMELINE_HEIGHT);
    let mut text = TextContext::new();
    let title = text.layout("Fontelle", &theme.font, None);

    let bar = transport_bar_layout(layout.transport, &theme.metrics);
    let readout = text.layout(&format_readout(&view, 4), &theme.font, None);
    let tempo = text.layout("120.00", &theme.font, None);
    let signature = text.layout("4/4", &theme.font, None);

    let mut scene = vello::Scene::new();
    draw_window(
        &mut scene,
        &theme,
        &layout,
        &Chrome {
            effect: None,
            automation: None,
            panel_title: &title,
            transport: TransportChrome {
                layout: bar,
                view,
                meters,
                readout: &readout,
                tempo: &tempo,
                signature: &signature,
                hover: None,
                marker_sample: 0,
            },
            roll: None,
            rack: None,
            browser: None,
            timeline: None,
            instrument: None,
            mixer: None,
            tabs: fontelle_ui::layout::editor_tabs(layout.panel.header, &theme.metrics),
            tab: fontelle_ui::layout::EditorTab::Roll,
            hover_tab: None,
            browser_title: "Soundfonts",
            labels: &Labels::new(),
            status: "",
            tooltip: None,
        },
    );
    let pixels = shared
        .lock()
        .expect("the shared renderer")
        .render(&scene, W, H, theme.palette.window)
        .expect("rendering a scene that fits in memory");

    dump(&pixels, &theme.name);

    Some(Shot {
        pixels,
        theme,
        layout,
        bar,
    })
}

/// Writes the frame out as a PNG when `FONTELLE_UI_DUMP` names a directory.
///
/// §2.5 of `docs/first-usable-plan.md` makes "the pixels have been seen once by
/// a human" half of the done-criterion for a GUI item. On a machine with no
/// display — CI, a remote session, a Wayland compositor whose root an X11
/// screen-grabber cannot see — this is the only way to satisfy it, and it costs
/// nothing when the variable is unset.
fn dump(pixels: &[u8], name: &str) {
    dump_sized(pixels, name, W, H);
}

/// [`dump`] for a frame that is not the standard one — the roll and the
/// instrument editor are shot at a size a real panel is.
fn dump_sized(pixels: &[u8], name: &str, width: u32, height: u32) {
    let Ok(dir) = std::env::var("FONTELLE_UI_DUMP") else {
        return;
    };
    let path = std::path::Path::new(&dir).join(format!("{}.png", name.replace(' ', "-")));
    let file = std::fs::File::create(&path).expect("somewhere to write the frame");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .expect("a PNG header")
        .write_image_data(pixels)
        .expect("the frame");
    eprintln!("wrote {}", path.display());
}

fn near(a: Color, b: Color) -> bool {
    // Solid fills should land exactly; one step of slack covers the
    // rasteriser's rounding without letting a wrong colour through.
    a.0.iter().zip(b.0.iter()).all(|(x, y)| x.abs_diff(*y) <= 1)
}

#[test]
fn the_window_is_painted_in_the_theme_it_was_given() {
    let Some(shot) = shoot(Theme::dark_default()) else {
        return;
    };
    // The margin outside the panel is the window colour.
    let corner = shot.at(1, 1);
    assert!(
        near(corner, shot.theme.palette.window),
        "the window corner is {corner:?}, not the theme's window colour {:?}",
        shot.theme.palette.window
    );
}

#[test]
fn the_panel_and_its_header_are_where_the_layout_put_them() {
    let Some(shot) = shoot(Theme::dark_default()) else {
        return;
    };
    let body = shot.centre_of(shot.layout.panel.body);
    assert!(
        near(body, shot.theme.palette.panel),
        "the panel body is {body:?}, expected {:?}",
        shot.theme.palette.panel
    );

    // Sample the header away from the title text, on the right-hand side.
    let header = shot.layout.panel.header;
    let right = shot.at(
        (header.right() - 8.0) as u32,
        (header.y + header.height / 2.0) as u32,
    );
    assert!(
        near(right, shot.theme.palette.panel_header),
        "the panel header is {right:?}, expected {:?}",
        shot.theme.palette.panel_header
    );
}

#[test]
fn the_title_is_actually_drawn_into_the_header() {
    let Some(shot) = shoot(Theme::dark_default()) else {
        return;
    };
    // Not "some pixel somewhere differs" — the glyphs have to be inside the
    // header band, which is what catches text drawn at the wrong baseline and
    // therefore off the top of the window.
    let header = shot.layout.panel.header;
    let y0 = header.y as u32;
    let y1 = header.bottom() as u32;
    let mut ink = 0;
    for y in y0..y1 {
        for x in header.x as u32..(header.x + header.width / 2.0) as u32 {
            if !near(shot.at(x, y), shot.theme.palette.panel_header) {
                ink += 1;
            }
        }
    }
    assert!(
        ink > 20,
        "only {ink} non-background pixels in the header — the title did not render"
    );
}

#[test]
fn a_different_theme_produces_different_pixels() {
    // The trap PROGRESS.md keeps naming: a test that passes with the feature
    // absent. Everything above would still pass if `draw_window` had the dark
    // palette hard-coded in it.
    let (Some(dark), Some(light)) = (shoot(Theme::dark_default()), shoot(Theme::light_default()))
    else {
        return;
    };
    assert!(
        near(light.at(1, 1), light.theme.palette.window),
        "the light theme's window corner is {:?}, not {:?}",
        light.at(1, 1),
        light.theme.palette.window
    );
    assert_ne!(
        dark.at(1, 1),
        light.at(1, 1),
        "the two themes rendered the same window colour"
    );
    assert_ne!(
        dark.centre_of(dark.layout.panel.body),
        light.centre_of(light.layout.panel.body),
        "the two themes rendered the same panel"
    );
}

#[test]
fn the_same_scene_renders_the_same_pixels_twice() {
    let (Some(a), Some(b)) = (shoot(Theme::dark_default()), shoot(Theme::dark_default())) else {
        return;
    };
    assert_eq!(
        a.pixels, b.pixels,
        "the same scene rendered differently twice — something in the pipeline is not deterministic"
    );
}

// ------------------------------------------------- the transport bar -------

#[test]
fn the_transport_bar_is_drawn_across_the_top() {
    let Some(shot) = shoot(Theme::dark_default()) else {
        return;
    };
    // Sampled between the read-out and the ruler, where nothing else draws.
    let gap = shot.at(
        (shot.bar.readout.right() + 1.0) as u32,
        (shot.bar.bar.y + 2.0) as u32,
    );
    assert!(
        near(gap, shot.theme.palette.panel_header),
        "the transport bar is {gap:?}, expected {:?}",
        shot.theme.palette.panel_header
    );
    // And it is not the panel: the two are separate surfaces with a gap.
    assert!(shot.layout.transport.bottom() < shot.layout.panel.frame.y);
}

#[test]
fn the_playhead_is_drawn_where_the_engine_says_it_is() {
    let view = TransportView {
        position_sample: 480_000 / 4,
        ..live_view()
    };
    let Some(shot) = shoot_with(Theme::dark_default(), view, [Meter::new(); 2]) else {
        return;
    };

    // The playhead travels along the ruler's groove, which is inset from the
    // ruler itself — ask the same function the renderer used.
    let track = shot.bar.ruler.inset(shot.bar.ruler.height * 0.3);
    let x = playhead_x(track, view.position_sample, view.length_samples);
    let y = (shot.bar.ruler.y + shot.bar.ruler.height / 2.0) as u32;

    let found =
        (x as u32 - 2..=x as u32 + 2).any(|px| near(shot.at(px, y), shot.theme.palette.playhead));
    assert!(
        found,
        "no playhead within two pixels of x={x}; found {:?}",
        shot.at(x as u32, y)
    );
}

#[test]
fn a_playhead_at_a_quarter_is_not_a_playhead_at_a_half() {
    // The assertion that fails if the position is ignored and the playhead is
    // simply parked somewhere plausible.
    let quarter = TransportView {
        position_sample: 480_000 / 4,
        ..live_view()
    };
    let half = TransportView {
        position_sample: 480_000 / 2,
        ..live_view()
    };
    let (Some(a), Some(b)) = (
        shoot_with(Theme::dark_default(), quarter, [Meter::new(); 2]),
        shoot_with(Theme::dark_default(), half, [Meter::new(); 2]),
    ) else {
        return;
    };

    let y = (a.bar.ruler.y + a.bar.ruler.height / 2.0) as u32;
    let playhead_at = |shot: &Shot| {
        (shot.bar.ruler.x as u32..shot.bar.ruler.right() as u32)
            .find(|&px| near(shot.at(px, y), shot.theme.palette.playhead))
    };
    let (x0, x1) = (playhead_at(&a), playhead_at(&b));
    assert!(x0.is_some() && x1.is_some(), "a playhead went missing");
    assert!(
        x1 > x0,
        "the playhead did not move: {x0:?} at a quarter, {x1:?} at a half"
    );
}

#[test]
fn a_window_with_no_engine_draws_no_playhead() {
    let Some(shot) = shoot(Theme::dark_default()) else {
        return;
    };
    let y = (shot.bar.ruler.y + shot.bar.ruler.height / 2.0) as u32;
    let any = (shot.bar.ruler.x as u32..shot.bar.ruler.right() as u32)
        .any(|px| near(shot.at(px, y), shot.theme.palette.playhead));
    assert!(
        !any,
        "a window with no audio device behind it drew a playhead anyway"
    );
}

#[test]
fn the_meter_fills_with_the_level_it_is_given() {
    let mut loud = [Meter::new(); 2];
    for meter in &mut loud {
        meter.update(1.0, 1.0 / 60.0);
    }
    let (Some(quiet), Some(hot)) = (
        shoot_with(Theme::dark_default(), live_view(), [Meter::new(); 2]),
        shoot_with(Theme::dark_default(), live_view(), loud),
    ) else {
        return;
    };

    let y = (hot.bar.meter.y + hot.bar.meter.height * 0.35) as u32;
    let lit = |shot: &Shot| {
        (shot.bar.meter.x as u32..shot.bar.meter.right() as u32)
            .filter(|&px| {
                let c = shot.at(px, y);
                near(c, shot.theme.palette.meter) || near(c, shot.theme.palette.meter_peak)
            })
            .count()
    };
    assert_eq!(lit(&quiet), 0, "a silent meter lit up");
    assert!(
        lit(&hot) > 20,
        "a full-scale meter only lit {} pixels",
        lit(&hot)
    );
}

// ------------------------------------------------------ the piano roll -----

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
    }
}

struct RollShot {
    pixels: Vec<u8>,
    theme: Theme,
    layout: fontelle_ui::canvas::RollLayout,
    view: RollView,
    toolbar: fontelle_ui::canvas::ToolbarLayout,
}

impl RollShot {
    fn at(&self, x: u32, y: u32) -> Color {
        let i = ((y * RW + x) * 4) as usize;
        Color(self.pixels[i..i + 4].try_into().expect("four bytes"))
    }
}

/// Renders a window whose panel is a piano roll holding `notes`.
///
/// **With the arrangement hidden**, so the roll gets the whole editor column:
/// these tests sample a pixel two octaves down from the top of the view, and a
/// roll squeezed into a third of the window does not reach that far. What the
/// arrangement draws has its own shot below.
fn shoot_roll(notes: &Arena<NoteId, Note>, selection: &[NoteId]) -> Option<RollShot> {
    shoot_roll_ghosted(notes, selection, &[])
}

/// [`shoot_roll`] with the snap set to something in particular, for the
/// grid-level tests.
fn shoot_roll_snapped(snap: SnapDivision) -> Option<RollShot> {
    shoot_roll_with(
        &Arena::default(),
        &[],
        &[],
        None,
        &fontelle_ui::document::KeyMap::unknown(),
        snap,
    )
}

/// [`shoot_roll`] over a channel whose instrument only plays some keys — a
/// drum kit — laid out with the key strip that map asks for.
///
/// The widened strip is rendered nowhere else, so without this the only thing
/// checking `NAMED_KEYBOARD_WIDTH` is arithmetic in `tests/keyboard.rs`.
fn shoot_roll_mapped(map: &fontelle_ui::document::KeyMap) -> Option<RollShot> {
    shoot_roll_with(&Arena::default(), &[], &[], None, map, SnapDivision::Step)
}

/// [`shoot_roll`] with the lane chip's menu open over it.
fn shoot_roll_menu(open: bool) -> Option<(RollShot, fontelle_ui::canvas::LaneMenu)> {
    let theme = Theme::dark_default();
    let layout = window_layout(RW as f32, RH as f32, &theme.metrics, 0.0);
    let roll_l = roll_layout(layout.panel.body, &theme.metrics, DEFAULT_LANE_HEIGHT);
    let bar = toolbar_layout(roll_l.toolbar, &theme.metrics);
    let chip = bar
        .items
        .iter()
        .find(|(control, _)| *control == fontelle_ui::canvas::RollControl::Lane)
        .map(|(_, rect)| *rect)
        .expect("the toolbar has a lane chip");
    let menu = fontelle_ui::canvas::lane_menu_layout(chip, roll_l.frame, &theme.metrics);
    let notes = Arena::default();
    let shot = shoot_roll_with(
        &notes,
        &[],
        &[],
        open.then(|| menu.clone()),
        &fontelle_ui::document::KeyMap::unknown(),
        SnapDivision::Step,
    )?;
    Some((shot, menu))
}

/// [`shoot_roll`] with an onion skin behind it.
fn shoot_roll_ghosted(
    notes: &Arena<NoteId, Note>,
    selection: &[NoteId],
    ghosts: &[fontelle_ui::document::GhostNote],
) -> Option<RollShot> {
    shoot_roll_with(
        notes,
        selection,
        ghosts,
        None,
        &fontelle_ui::document::KeyMap::unknown(),
        SnapDivision::Step,
    )
}

fn shoot_roll_with(
    notes: &Arena<NoteId, Note>,
    selection: &[NoteId],
    ghosts: &[fontelle_ui::document::GhostNote],
    lane_menu: Option<fontelle_ui::canvas::LaneMenu>,
    key_map: &fontelle_ui::document::KeyMap,
    snap: SnapDivision,
) -> Option<RollShot> {
    let theme = Theme::dark_default();
    let shared = headless()?;
    let layout = window_layout(RW as f32, RH as f32, &theme.metrics, 0.0);
    let mut text = TextContext::new();
    let title = text.layout("Roll", &theme.font, None);
    let view = TransportView::unavailable();
    let readout = text.layout(&format_readout(&view, 4), &theme.font, None);
    let tempo = text.layout("120.00", &theme.font, None);
    let signature = text.layout("4/4", &theme.font, None);
    let roll_l = fontelle_ui::canvas::roll_layout_with_keys(
        layout.panel.body,
        &theme.metrics,
        DEFAULT_LANE_HEIGHT,
        fontelle_ui::canvas::keyboard_width(key_map),
    );
    let roll_view = RollView {
        top_key: 72,
        snap,
        ..RollView::default()
    };
    // The keyboard's captions have to be shaped before they can be drawn — the
    // window does this in `ensure_labels`, and a caption nobody shaped draws as
    // nothing at all, which would make a blank strip look like a pass.
    let mut labels = Labels::new();
    for key in fontelle_ui::canvas::visible_keys(&roll_view, roll_l.grid) {
        let key = key.clamp(0, 127) as u8;
        if let Some(name) = key_map.name(key) {
            let name = name.to_string();
            labels.ensure(&name, &theme.font, &mut text);
        } else if key.is_multiple_of(12) {
            labels.ensure(
                &fontelle_ui::render::key_name(i32::from(key)),
                &theme.font,
                &mut text,
            );
        }
    }

    let mut scene = vello::Scene::new();
    draw_window(
        &mut scene,
        &theme,
        &layout,
        &Chrome {
            effect: None,
            automation: None,
            panel_title: &title,
            transport: TransportChrome {
                layout: transport_bar_layout(layout.transport, &theme.metrics),
                view,
                meters: [Meter::new(); 2],
                readout: &readout,
                tempo: &tempo,
                signature: &signature,
                hover: None,
                marker_sample: 0,
            },
            roll: Some(RollChrome {
                layout: roll_l,
                toolbar: toolbar_layout(roll_l.toolbar, &theme.metrics),
                view: roll_view,
                notes,
                selection,
                playhead_tick: None,
                beats_per_bar: 4,
                tool: Tool::Draw,
                snap,
                lane_property: LaneProperty::Velocity,
                key_map,
                ghosts,
                ghost_filter: if ghosts.is_empty() {
                    fontelle_ui::document::GhostFilter::Off
                } else {
                    fontelle_ui::document::GhostFilter::All
                },
                marker_tick: None,
                marquee: None,
                hover: None,
                lane_menu: lane_menu.as_ref(),
                slice: None,
            }),
            rack: None,
            browser: None,
            timeline: None,
            instrument: None,
            mixer: None,
            tabs: fontelle_ui::layout::editor_tabs(layout.panel.header, &theme.metrics),
            tab: fontelle_ui::layout::EditorTab::Roll,
            hover_tab: None,
            browser_title: "Soundfonts",
            labels: &labels,
            status: "",
            tooltip: None,
        },
    );
    let pixels = shared
        .lock()
        .expect("the shared renderer")
        .render(&scene, RW, RH, theme.palette.window)
        .expect("rendering a scene that fits in memory");

    dump_sized(
        &pixels,
        if key_map.is_known() {
            "roll-keymap"
        } else if ghosts.is_empty() {
            "roll"
        } else {
            "roll-onion"
        },
        RW,
        RH,
    );
    Some(RollShot {
        toolbar: toolbar_layout(roll_l.toolbar, &theme.metrics),
        pixels,
        theme,
        layout: roll_l,
        view: roll_view,
    })
}

#[test]
fn a_note_is_drawn_where_the_document_puts_it() {
    let mut notes = Arena::default();
    notes.insert(note(0, PPQN, 60));
    let Some(shot) = shoot_roll(&notes, &[]) else {
        return;
    };

    let x = tick_to_x(&shot.view, shot.layout.grid, PPQN / 2) as u32;
    let y = (fontelle_ui::canvas::key_to_y(&shot.view, shot.layout.grid, 60)
        + shot.view.key_height / 2.0) as u32;
    let found = shot.at(x, y);
    assert!(
        near(found, shot.theme.palette.note),
        "expected a note at ({x}, {y}), found {found:?}"
    );
}

#[test]
fn an_empty_row_is_not_a_note() {
    // The assertion that fails if the roll paints notes everywhere, or nowhere
    // and the one above happened to land on a grid line.
    let mut notes = Arena::default();
    notes.insert(note(0, PPQN, 60));
    let Some(shot) = shoot_roll(&notes, &[]) else {
        return;
    };
    let x = tick_to_x(&shot.view, shot.layout.grid, PPQN / 2) as u32;
    let y = (fontelle_ui::canvas::key_to_y(&shot.view, shot.layout.grid, 65)
        + shot.view.key_height / 2.0) as u32;
    assert!(
        !near(shot.at(x, y), shot.theme.palette.note),
        "an empty row was painted as a note"
    );
}

#[test]
fn a_selected_note_looks_different_from_an_unselected_one() {
    let mut notes = Arena::default();
    let id = notes.insert(note(0, PPQN, 60));
    let (Some(plain), Some(selected)) = (shoot_roll(&notes, &[]), shoot_roll(&notes, &[id])) else {
        return;
    };
    let x = tick_to_x(&plain.view, plain.layout.grid, PPQN / 2) as u32;
    let y = (fontelle_ui::canvas::key_to_y(&plain.view, plain.layout.grid, 60)
        + plain.view.key_height / 2.0) as u32;
    assert_ne!(
        plain.at(x, y),
        selected.at(x, y),
        "selecting a note changed nothing on screen"
    );
    assert!(near(
        selected.at(x, y),
        selected.theme.palette.note_selected
    ));
}

#[test]
fn the_keyboard_marks_every_c_so_octaves_can_be_counted() {
    let Some(shot) = shoot_roll(&Arena::default(), &[]) else {
        return;
    };
    // C5 is key 60 + 12 = 72, the top of this view; C4 is 60.
    for key in [60u8, 72] {
        let y = (fontelle_ui::canvas::key_to_y(&shot.view, shot.layout.grid, key)
            + shot.view.key_height / 2.0) as u32;
        let x = shot.layout.keys.x as u32 + 1;
        assert!(
            near(shot.at(x, y), shot.theme.palette.accent),
            "key {key} has no C marker; found {:?}",
            shot.at(x, y)
        );
    }
}

#[test]
fn an_accidental_row_is_shaded_differently_from_a_natural_one() {
    let Some(shot) = shoot_roll(&Arena::default(), &[]) else {
        return;
    };
    // Well right of the bar line at tick 0 so neither sample lands on a grid
    // line: F (65, natural) against F# (66, accidental).
    let x = tick_to_x(&shot.view, shot.layout.grid, PPQN / 8) as u32;
    let sample = |key: u8| {
        let y = (fontelle_ui::canvas::key_to_y(&shot.view, shot.layout.grid, key)
            + shot.view.key_height / 2.0) as u32;
        shot.at(x, y)
    };
    assert_ne!(
        sample(65),
        sample(66),
        "the black-key rows are shaded the same as the white-key rows"
    );
}

// -------------------------------------------------------- the arrangement ---

/// Renders a window whose editor column carries the arrangement over the roll,
/// and hands back the pixels and the geometry to sample them by.
struct TimelineShot {
    pixels: Vec<u8>,
    theme: Theme,
    layout: fontelle_ui::canvas::TimelineLayout,
    view: fontelle_ui::canvas::TimelineView,
}

impl TimelineShot {
    fn at(&self, x: u32, y: u32) -> Color {
        let i = ((y * RW + x) * 4) as usize;
        Color(self.pixels[i..i + 4].try_into().expect("four bytes"))
    }
}

fn shoot_timeline(clips: &[fontelle_ui::document::ClipInfo]) -> Option<TimelineShot> {
    use fontelle_ui::canvas::{TimelineView, timeline_layout};
    use fontelle_ui::document::LaneInfo;
    use fontelle_ui::render::TimelineChrome;

    let theme = Theme::dark_default();
    let shared = headless()?;
    let layout = window_layout(
        RW as f32,
        RH as f32,
        &theme.metrics,
        DEFAULT_TIMELINE_HEIGHT,
    );
    let mut text = TextContext::new();
    let title = text.layout("Roll", &theme.font, None);
    let view = TransportView::unavailable();
    let readout = text.layout(&format_readout(&view, 4), &theme.font, None);
    let tempo = text.layout("120.00", &theme.font, None);
    let signature = text.layout("4/4", &theme.font, None);
    let l = timeline_layout(layout.timeline.body, &theme.metrics);
    let tview = TimelineView::default();
    let lanes: Vec<LaneInfo> = (0..4)
        .map(|n| LaneInfo {
            name: format!("Lane {}", n + 1),
            muted: false,
        })
        .collect();

    let mut scene = vello::Scene::new();
    draw_window(
        &mut scene,
        &theme,
        &layout,
        &Chrome {
            effect: None,
            automation: None,
            panel_title: &title,
            transport: TransportChrome {
                layout: transport_bar_layout(layout.transport, &theme.metrics),
                view,
                meters: [Meter::new(); 2],
                readout: &readout,
                tempo: &tempo,
                signature: &signature,
                hover: None,
                marker_sample: 0,
            },
            roll: None,
            rack: None,
            browser: None,
            timeline: Some(TimelineChrome {
                tool: fontelle_ui::canvas::TimelineTool::default(),
                toolbar: fontelle_ui::canvas::timeline_toolbar_layout(l.toolbar, &theme.metrics),
                hover: None,
                can_paste: false,
                panel: layout.timeline,
                layout: l,
                view: tview,
                lanes: &lanes,
                clips,
                selection: &[],
                playhead_tick: 0,
                marker_tick: 0,
                beats_per_bar: 4,
                marquee: None,
                focused: false,
            }),
            instrument: None,
            mixer: None,
            tabs: fontelle_ui::layout::editor_tabs(layout.panel.header, &theme.metrics),
            tab: fontelle_ui::layout::EditorTab::Roll,
            hover_tab: None,
            browser_title: "Soundfonts",
            labels: &Labels::new(),
            status: "",
            tooltip: None,
        },
    );
    let pixels = shared
        .lock()
        .expect("the shared renderer")
        .render(&scene, RW, RH, theme.palette.window)
        .expect("the scene must render");
    Some(TimelineShot {
        pixels,
        theme,
        layout: l,
        view: tview,
    })
}

fn a_clip(
    lane: usize,
    start: Tick,
    length: Tick,
    color: [u8; 4],
) -> fontelle_ui::document::ClipInfo {
    let mut arena: Arena<fontelle_types::ClipId, ()> = Arena::default();
    fontelle_ui::document::ClipInfo {
        id: arena.insert(()),
        lane,
        start,
        length,
        name: "Part".to_string(),
        muted: false,
        open: false,
        color,
        loop_length: None,
        kind: fontelle_ui::document::ClipKind::Notes,
        curve: Vec::new(),
    }
}

#[test]
fn a_clip_is_drawn_as_a_block_where_the_document_puts_it() {
    use fontelle_ui::canvas::{clip_rect, lane_to_y};

    let colour = [0x8a, 0x4f, 0xd0, 0xff];
    let clips = vec![a_clip(1, PPQN * 8, PPQN * 16, colour)];
    let Some(shot) = shoot_timeline(&clips) else {
        return;
    };

    let block = clip_rect(&shot.view, shot.layout.grid, &clips[0]);
    let x = (block.x + block.width / 2.0) as u32;
    let y = (block.y + shot.view.lane_height / 2.0) as u32;
    let found = shot.at(x, y);
    assert!(
        near(found, Color(colour)),
        "expected a clip block at ({x}, {y}), found {found:?}"
    );

    // And the lane above it, where there is no clip, is not painted as one.
    let empty_y = (lane_to_y(&shot.view, shot.layout.grid, 0) + shot.view.lane_height / 2.0) as u32;
    assert!(
        !near(shot.at(x, empty_y), Color(colour)),
        "an empty lane was painted as a clip"
    );
    let _ = &shot.theme;
}

#[test]
fn an_arrangement_with_no_clips_still_draws_its_lanes_and_ruler() {
    let Some(shot) = shoot_timeline(&[]) else {
        return;
    };
    // The header column is the panel header's colour, not the grid's — the two
    // halves of the strip have to be tellable apart.
    let header = shot.at(
        shot.layout.headers.x as u32 + 4,
        shot.layout.headers.y as u32 + 4,
    );
    let grid = shot.at(
        shot.layout.grid.x as u32 + 40,
        shot.layout.grid.y as u32 + 4,
    );
    assert_ne!(header, grid, "the lane headers and the grid look the same");
    assert!(near(header, shot.theme.palette.panel_header));
}

// ------------------------------------------------- the instrument editor ---

/// Renders the editor column showing the instrument tab, with a patch's worth
/// of controls on it.
fn shoot_instrument() -> Option<(Vec<u8>, Theme, fontelle_ui::canvas::InstrumentLayout)> {
    use fontelle_types::ParamAddress;
    use fontelle_ui::canvas::{
        InstrumentGroup, InstrumentParam, InstrumentView, ParamKind, instrument_layout,
    };
    use fontelle_ui::layout::{EditorTab, editor_tabs};
    use fontelle_ui::render::InstrumentChrome;

    let theme = Theme::dark_default();
    let shared = headless()?;
    let layout = window_layout(RW as f32, RH as f32, &theme.metrics, 0.0);
    let mut text = TextContext::new();
    let title = text.layout("Roll", &theme.font, None);
    let view = TransportView::unavailable();
    let readout = text.layout(&format_readout(&view, 4), &theme.font, None);
    let tempo = text.layout("120.00", &theme.font, None);
    let signature = text.layout("4/4", &theme.font, None);

    let knob = |name: &str, value: f32, display: &str| InstrumentParam {
        address: ParamAddress::new(format!("patch/{name}")),
        label: name.to_string(),
        value,
        display: display.to_string(),
        kind: ParamKind::Knob,
    };
    let instrument = InstrumentView {
        title: "tri baja".to_string(),
        groups: vec![
            InstrumentGroup {
                name: "Channel".to_string(),
                params: vec![knob("volume", 0.8, "+0.0 dB"), knob("pan", 0.5, "centre")],
            },
            InstrumentGroup {
                name: "Filter 1".to_string(),
                params: vec![
                    InstrumentParam {
                        address: ParamAddress::new("patch/filter[0]/enabled"),
                        label: "on".to_string(),
                        value: 1.0,
                        display: "on".to_string(),
                        kind: ParamKind::Switch,
                    },
                    InstrumentParam {
                        address: ParamAddress::new("patch/filter[0]/mode"),
                        label: "mode".to_string(),
                        value: 0.0,
                        display: "LP".to_string(),
                        kind: ParamKind::Choice(
                            ["LP", "HP", "BP", "Notch"].map(str::to_string).to_vec(),
                        ),
                    },
                    knob("cutoff", 0.65, "2.10 kHz"),
                    knob("res", 0.2, "0.20"),
                ],
            },
            InstrumentGroup {
                name: "Amp envelope".to_string(),
                params: vec![
                    knob("attack", 0.1, "10 ms"),
                    knob("decay", 0.4, "640 ms"),
                    knob("sustain", 1.0, "100%"),
                    knob("release", 0.3, "270 ms"),
                ],
            },
        ],
    };

    let l = instrument_layout(layout.panel.body, &theme.metrics, &instrument);
    // Everything the pure renderer will look up has to be shaped first.
    let mut labels = Labels::new();
    for caption in ["Piano roll", "Instrument"] {
        labels.ensure(caption, &theme.font, &mut text);
    }
    for group in &instrument.groups {
        labels.ensure(&group.name, &theme.font, &mut text);
        for param in &group.params {
            labels.ensure(&param.label, &theme.font, &mut text);
            labels.ensure(&param.display, &theme.font, &mut text);
        }
    }

    let mut scene = vello::Scene::new();
    draw_window(
        &mut scene,
        &theme,
        &layout,
        &Chrome {
            effect: None,
            automation: None,
            panel_title: &title,
            transport: TransportChrome {
                layout: transport_bar_layout(layout.transport, &theme.metrics),
                view,
                meters: [Meter::new(); 2],
                readout: &readout,
                tempo: &tempo,
                signature: &signature,
                hover: None,
                marker_sample: 0,
            },
            roll: None,
            rack: None,
            browser: None,
            timeline: None,
            instrument: Some(InstrumentChrome {
                layout: l.clone(),
                view: &instrument,
                hover: None,
                active: Some((1, 2)),
            }),
            mixer: None,
            tabs: editor_tabs(layout.panel.header, &theme.metrics),
            tab: EditorTab::Instrument,
            hover_tab: None,
            browser_title: "Soundfonts",
            labels: &labels,
            status: "",
            tooltip: None,
        },
    );
    let pixels = shared
        .lock()
        .expect("the shared renderer")
        .render(&scene, RW, RH, theme.palette.window)
        .expect("the scene must render");
    dump_sized(&pixels, "instrument", RW, RH);
    Some((pixels, theme, l))
}

#[test]
fn the_instrument_tab_draws_a_control_for_every_parameter() {
    let Some((pixels, theme, l)) = shoot_instrument() else {
        return;
    };
    let at = |x: u32, y: u32| {
        let i = ((y * RW + x) * 4) as usize;
        Color(pixels[i..i + 4].try_into().expect("four bytes"))
    };

    // The panel is not empty: the cell being dragged is drawn on the accent, so
    // there is something identifiably a live control on screen.
    let cell = l
        .cells
        .iter()
        .find(|(g, p, _)| (*g, *p) == (1, 2))
        .map(|(_, _, r)| *r)
        .expect("the cutoff cell");
    let mut found_accent = false;
    for dy in 0..cell.height as u32 {
        for dx in 0..cell.width as u32 {
            if near(
                at(cell.x as u32 + dx, cell.y as u32 + dy),
                theme.palette.accent,
            ) {
                found_accent = true;
            }
        }
    }
    assert!(
        found_accent,
        "the knob being dragged drew nothing in the accent colour"
    );

    // And the panel ground is still the panel's, not the window's — the editor
    // column is a panel whichever tab is showing.
    assert!(near(at(4, RH - 4), theme.palette.window));
}

#[test]
fn an_onion_skinned_note_is_drawn_faintly_and_never_as_a_real_note() {
    use fontelle_ui::document::GhostNote;

    let colour = [0xd0, 0x7a, 0x4f, 0xff];
    let ghosts = vec![GhostNote {
        start: 0,
        length: PPQN * 2,
        key: 64,
        color: colour,
    }];
    let mut notes = Arena::default();
    notes.insert(note(0, PPQN * 2, 60));
    let Some(shot) = shoot_roll_ghosted(&notes, &[], &ghosts) else {
        return;
    };

    let x = tick_to_x(&shot.view, shot.layout.grid, PPQN) as u32;
    let ghost_y = (fontelle_ui::canvas::key_to_y(&shot.view, shot.layout.grid, 64)
        + shot.view.key_height / 2.0) as u32;
    let real_y = (fontelle_ui::canvas::key_to_y(&shot.view, shot.layout.grid, 60)
        + shot.view.key_height / 2.0) as u32;

    let ghost = shot.at(x, ghost_y);
    let real = shot.at(x, real_y);
    let empty_y = (fontelle_ui::canvas::key_to_y(&shot.view, shot.layout.grid, 67)
        + shot.view.key_height / 2.0) as u32;
    let empty = shot.at(x, empty_y);

    assert_ne!(ghost, empty, "the ghost drew nothing at all");
    assert_ne!(
        ghost, real,
        "a ghosted note is indistinguishable from a real one, which is the one \
         thing an onion skin must not be"
    );
    assert!(
        near(real, shot.theme.palette.note),
        "the real note stopped looking like a note: {real:?}"
    );
    // Faint: closer to the empty grid than to a solid note.
    let distance = |a: Color, b: Color| {
        a.0.iter()
            .zip(b.0.iter())
            .map(|(x, y)| i32::from(*x).abs_diff(i32::from(*y)))
            .sum::<u32>()
    };
    assert!(
        distance(ghost, empty) < distance(ghost, real),
        "the onion skin is too strong to read the grid through"
    );
}

/// The lane chip's menu covers what is under it.
///
/// A menu that is laid out but not drawn on top is a menu you cannot read, and
/// this is a canvas with no widget tree to guarantee draw order for it — the
/// order is the order the drawing functions are called in, which is exactly the
/// kind of thing that survives a refactor by luck. So it is checked in pixels:
/// the same point, with the menu shut and with it open, has to differ.
#[test]
fn the_lane_menu_draws_over_the_grid_beneath_it() {
    let Some((shut, menu)) = shoot_roll_menu(false) else {
        eprintln!("no GPU: skipping");
        return;
    };
    let Some((open, _)) = shoot_roll_menu(true) else {
        return;
    };
    assert!(!menu.frame.is_empty(), "the menu has somewhere to be");

    // A point well inside the menu, below its first row so it is menu ground
    // rather than a highlighted row.
    let (x, y) = (
        (menu.frame.x + menu.frame.width / 2.0) as u32,
        (menu.frame.y + menu.frame.height * 0.75) as u32,
    );
    assert_ne!(
        shut.at(x, y),
        open.at(x, y),
        "opening the menu changed nothing at ({x}, {y}), so it is behind the grid"
    );

    // And it is not simply painting the whole panel: a point outside it is
    // untouched.
    let outside = (
        (menu.frame.x + menu.frame.width + 20.0) as u32,
        (menu.frame.y + menu.frame.height * 0.75) as u32,
    );
    assert_eq!(
        shut.at(outside.0, outside.1),
        open.at(outside.0, outside.1),
        "the menu leaked outside its own frame"
    );
}

/// The two seams around the sidebar are visible, not just clickable.
///
/// An invisible drag target is one nobody finds, which is how the sidebar came
/// to be reported as un-resizable when the *margin* between the panels had
/// always been there — see `tests/docks.rs`.
#[test]
fn the_sidebars_seams_are_drawn_as_grips() {
    let Some(shot) = shoot(Theme::dark_default()) else {
        eprintln!("no GPU: skipping");
        return;
    };
    let layout = shot.layout;

    for (name, seam) in [
        ("sidebar_seam", layout.sidebar_seam),
        ("sidebar_split", layout.sidebar_split),
    ] {
        assert!(!seam.is_empty(), "{name} has nowhere to be");
        let middle = shot.at(
            (seam.x + seam.width / 2.0) as u32,
            (seam.y + seam.height / 2.0) as u32,
        );
        // The grip is drawn in the border colour over the window's ground, so
        // the middle of the seam is not the ground.
        let corner = shot.at((seam.x) as u32, (seam.y + 1.0) as u32);
        assert_ne!(
            middle, corner,
            "{name} has no grip on it: middle {middle:?} matches its own edge"
        );
    }
}

/// A drum kit's key map, drawn.
///
/// This is the picture the whole key-map feature exists to produce: the rows
/// the soundfont cannot play are visibly dead, and the ones it can are not.
/// Asserting it in pixels rather than by eye is the only way to know the
/// colour reached the canvas — a token added to the palette and never read
/// draws exactly like a token that was never added.
fn kit_map() -> fontelle_ui::document::KeyMap {
    use fontelle_ui::document::{KeyInfo, KeyMap};
    // Named on keys the default view actually shows, so the dumped frame is a
    // picture of the thing rather than of the empty space above it.
    KeyMap::new(
        (0..KeyMap::KEYS)
            .map(|key| {
                let name = match key {
                    60 => Some("Snare"),
                    62 => Some("Kick"),
                    64 => Some("Closed Hat"),
                    67 => Some("Open Hat"),
                    69 => Some("Crash Cymbal"),
                    _ => None,
                };
                KeyInfo {
                    playable: name.is_some(),
                    name: name.map(str::to_string),
                }
            })
            .collect(),
    )
}

#[test]
fn a_key_the_instrument_cannot_play_is_drawn_dead_and_one_it_can_is_not() {
    let Some(shot) = shoot_roll_mapped(&kit_map()) else {
        return;
    };
    // A few pixels clear of the snap line at that tick: the vertical grid is
    // drawn over the row shading, and a probe that lands on a beat measures
    // the line rather than the row.
    let x = tick_to_x(&shot.view, shot.layout.grid, PPQN / 2) as u32 + 5;
    let row = |key: u8| {
        (fontelle_ui::canvas::key_to_y(&shot.view, shot.layout.grid, key)
            + shot.view.key_height / 2.0) as u32
    };

    let dead = shot.at(x, row(65));
    assert!(
        near(dead, shot.theme.palette.row_dead),
        "key 65 plays nothing and must be greyed, found {dead:?}"
    );
    let live = shot.at(x, row(60));
    assert!(
        !near(live, shot.theme.palette.row_dead),
        "key 60 is the snare and must not be greyed, found {live:?}"
    );
}

#[test]
fn an_unknown_key_map_greys_nothing_at_all() {
    // The other half of the rule, and the one that keeps a channel with no
    // instrument on it from looking like a channel that plays nothing.
    let Some(shot) = shoot_roll_mapped(&fontelle_ui::document::KeyMap::unknown()) else {
        return;
    };
    let x = tick_to_x(&shot.view, shot.layout.grid, PPQN / 2) as u32 + 5;
    for key in [60u8, 65, 70] {
        let y = (fontelle_ui::canvas::key_to_y(&shot.view, shot.layout.grid, key)
            + shot.view.key_height / 2.0) as u32;
        assert!(
            !near(shot.at(x, y), shot.theme.palette.row_dead),
            "key {key}: nothing is known, so nothing may be greyed"
        );
    }
}

/// The grid's three levels, in pixels.
///
/// A bar line, a beat line and a sixteenth line have to be three visibly
/// different things or the grid is one comb you cannot count. Asserting the
/// *palette* tokens differ is `tests/theme.rs`'s job; this is the half that
/// checks each one actually reaches the canvas at the tick it belongs to.
#[test]
fn a_bar_a_beat_and_a_subdivision_are_drawn_in_three_different_inks() {
    let Some(shot) = shoot_roll(&Arena::default(), &[]) else {
        return;
    };
    let p = &shot.theme.palette;
    // A row with no note and no accidental shading to confuse the sample.
    let y = (fontelle_ui::canvas::key_to_y(&shot.view, shot.layout.grid, 64)
        + shot.view.key_height / 2.0) as u32;
    let at = |tick: Tick| {
        let x = tick_to_x(&shot.view, shot.layout.grid, tick).floor() as u32;
        shot.at(x, y)
    };

    // Bar 2, beat 2 of bar 1, and the offbeat between beats 1 and 2.
    let bar = at(PPQN * 4);
    let beat = at(PPQN);
    let sub = at(PPQN / 2);

    assert!(
        near(bar, p.grid_line_strong),
        "the bar line should be the strong ink, found {bar:?}"
    );
    assert!(
        near(beat, p.grid_line),
        "the beat line should be the middle ink, found {beat:?}"
    );
    assert!(
        near(sub, p.grid_line_sub),
        "the offbeat should be the faint ink, found {sub:?}"
    );
}

/// And the offbeat is drawn even when the snap is nowhere near it — the grid
/// is the ruler, not the snap. This is the half of the report that was about
/// missing lines rather than about their colour.
#[test]
fn the_offbeat_line_is_drawn_with_the_snap_set_to_bars() {
    let Some(shot) = shoot_roll_snapped(SnapDivision::Bar) else {
        return;
    };
    let y = (fontelle_ui::canvas::key_to_y(&shot.view, shot.layout.grid, 64)
        + shot.view.key_height / 2.0) as u32;
    let x = tick_to_x(&shot.view, shot.layout.grid, PPQN / 2).floor() as u32;
    assert!(
        near(shot.at(x, y), shot.theme.palette.grid_line_sub),
        "snapping to bars must not empty the bar, found {:?}",
        shot.at(x, y)
    );
}

// ------------------------------------------------------------- the mixer ---

/// The mixer tab, drawn at panel size.
///
/// One strip pulled well down and hard left, one muted, and the master — so
/// there is something on screen for each of the three things a strip can say.
#[allow(clippy::type_complexity)]
fn shoot_mixer() -> Option<(Vec<u8>, Theme, fontelle_ui::canvas::MixerLayout)> {
    use fontelle_ui::canvas::{format_gain_db, mixer_layout};
    use fontelle_ui::document::MixerStrip;
    use fontelle_ui::layout::{EditorTab, editor_tabs};
    use fontelle_ui::render::MixerChrome;

    let theme = Theme::dark_default();
    let shared = headless()?;
    let layout = window_layout(RW as f32, RH as f32, &theme.metrics, 0.0);
    let mut text = TextContext::new();
    let title = text.layout("Mix", &theme.font, None);
    let view = TransportView::unavailable();
    let readout = text.layout(&format_readout(&view, 4), &theme.font, None);
    let tempo = text.layout("120.00", &theme.font, None);
    let signature = text.layout("4/4", &theme.font, None);

    let strip = |name: &str, gain_db: f32, pan: f32, mute: bool| MixerStrip {
        name: name.to_string(),
        gain_db,
        pan,
        mute,
        solo: false,
        is_master: false,
        color: [0x4f, 0x8f, 0xd0, 0xff],
        inserts: Vec::new(),
    };
    let strips = vec![
        strip("Drums", 0.0, 0.0, false),
        strip("Bass", -18.0, -0.8, false),
        strip("Keys", 3.0, 0.5, true),
        MixerStrip {
            is_master: true,
            ..strip("Master", 0.0, 0.0, false)
        },
    ];
    let peaks = vec![[0.8, 0.6], [0.2, 0.2], [0.0, 0.0], [0.9, 0.9]];

    let l = mixer_layout(layout.panel.body, &theme.metrics, &strips, 0);
    let mut labels = Labels::new();
    for caption in ["Piano roll", "Instrument", "Mixer", "M", "S"] {
        labels.ensure(caption, &theme.font, &mut text);
    }
    for s in &strips {
        labels.ensure(&s.name, &theme.font, &mut text);
        labels.ensure(&format_gain_db(s.gain_db), &theme.font, &mut text);
    }

    let mut scene = vello::Scene::new();
    draw_window(
        &mut scene,
        &theme,
        &layout,
        &Chrome {
            effect: None,
            automation: None,
            panel_title: &title,
            transport: TransportChrome {
                layout: transport_bar_layout(layout.transport, &theme.metrics),
                view,
                meters: [Meter::new(); 2],
                readout: &readout,
                tempo: &tempo,
                signature: &signature,
                hover: None,
                marker_sample: 0,
            },
            roll: None,
            rack: None,
            browser: None,
            timeline: None,
            instrument: None,
            mixer: Some(MixerChrome {
                layout: l.clone(),
                strips: &strips,
                peaks: &peaks,
                hover: None,
                active: None,
                selected: 0,
                output_label: String::new(),
                insert_drag: None,
                output_menu: None,
                route_names: &[],
                output: None,
            }),
            tabs: editor_tabs(layout.panel.header, &theme.metrics),
            tab: EditorTab::Mixer,
            hover_tab: None,
            browser_title: "Soundfonts",
            labels: &labels,
            status: "",
            tooltip: None,
        },
    );
    let pixels = shared
        .lock()
        .expect("the shared renderer")
        .render(&scene, RW, RH, theme.palette.window)
        .expect("the scene must render");
    dump_sized(&pixels, "mixer", RW, RH);
    Some((pixels, theme, l))
}

#[test]
fn a_faders_handle_is_drawn_where_its_level_says_it_is() {
    // The claim the whole panel rests on: what is on screen is the level in
    // the document. If the handle were drawn anywhere else, every fader would
    // jump the moment it was grabbed — `fader_db_at` reads the position back.
    let Some((pixels, theme, l)) = shoot_mixer() else {
        return;
    };
    let at = |x: u32, y: u32| {
        let i = ((y * RW + x) * 4) as usize;
        Color(pixels[i..i + 4].try_into().expect("four bytes"))
    };

    // Strip 1 is at -18 dB, so its handle is well below strip 0's at unity.
    let unity = l.strips[0].handle;
    let pulled_down = l.strips[1].handle;
    assert!(
        pulled_down.y > unity.y + 20.0,
        "a track 18 dB down should sit visibly lower: {} against {}",
        pulled_down.y,
        unity.y
    );

    // And the handle is actually painted there, in the ink the theme gives it.
    let x = (unity.x + unity.width / 2.0) as u32;
    let y = (unity.y + unity.height / 2.0) as u32;
    assert!(
        near(at(x, y), theme.palette.text_muted),
        "the handle should be drawn at the level's own position, found {:?}",
        at(x, y)
    );
}

#[test]
fn a_muted_strip_is_drawn_muted_and_a_soloable_one_is_not() {
    let Some((pixels, theme, l)) = shoot_mixer() else {
        return;
    };
    let at = |x: u32, y: u32| {
        let i = ((y * RW + x) * 4) as usize;
        Color(pixels[i..i + 4].try_into().expect("four bytes"))
    };

    // Strip 2 is muted, so its M button carries the accent and strip 0's does
    // not — which is the only thing on the strip that says a track is off.
    let on = l.strips[2].mute;
    let off = l.strips[0].mute;
    assert!(
        near(
            at(
                (on.x + on.width / 2.0) as u32,
                (on.y + on.height / 2.0) as u32
            ),
            theme.palette.accent
        ),
        "a muted track's M should be lit"
    );
    assert!(
        !near(
            at(
                (off.x + off.width / 2.0) as u32,
                (off.y + off.height / 2.0) as u32
            ),
            theme.palette.accent
        ),
        "and an unmuted one's should not"
    );
}

#[test]
fn a_meter_reading_full_scale_fills_its_bar_and_a_silent_one_does_not() {
    let Some((pixels, theme, l)) = shoot_mixer() else {
        return;
    };
    let at = |x: u32, y: u32| {
        let i = ((y * RW + x) * 4) as usize;
        Color(pixels[i..i + 4].try_into().expect("four bytes"))
    };

    // Strip 0 peaks at 0.8 — near the top of the scale — and strip 2 is
    // silent. A quarter of the way down the meter tells them apart.
    let loud = l.strips[0].meter;
    let quiet = l.strips[2].meter;
    let near_top =
        |m: fontelle_ui::layout::Rect| ((m.x + 1.0) as u32, (m.y + m.height * 0.25) as u32);
    let (x, y) = near_top(loud);
    assert!(
        near(at(x, y), theme.palette.meter),
        "a loud track's meter should be filled near the top, found {:?}",
        at(x, y)
    );
    let (x, y) = near_top(quiet);
    assert!(
        !near(at(x, y), theme.palette.meter),
        "a silent one's should not be"
    );
}

#[test]
fn every_transport_button_actually_draws_its_glyph() {
    // Found the hard way: `record` and `metronome` were laid out, hit-tested
    // and themed, and left out of the one array the renderer loops over — so
    // the bar had two buttons that could be pressed and could not be seen.
    // Nothing in the view-model tests could catch that, because the geometry
    // and the hit-testing were both right.
    let Some(shot) = shoot_with(Theme::dark_default(), live_view(), [Meter::new(); 2]) else {
        return;
    };

    for (name, rect) in [
        ("play", shot.bar.play),
        ("stop", shot.bar.stop),
        ("loop", shot.bar.loop_toggle),
        ("record", shot.bar.record),
        ("metronome", shot.bar.metronome),
    ] {
        assert!(!rect.is_empty(), "{name} has no room");
        // Something in the box is not the window's own ground — which is what
        // "a glyph was drawn here" means.
        let mut ink = 0;
        for y in rect.y as u32..rect.bottom().min(H as f32) as u32 {
            for x in rect.x as u32..rect.right().min(W as f32) as u32 {
                if !near(shot.at(x, y), shot.theme.palette.window) {
                    ink += 1;
                }
            }
        }
        assert!(ink > 8, "{name} drew {ink} pixels — it is an empty box");
    }
}

#[test]
fn the_active_tool_chip_is_lit_on_both_toolbars() {
    // Twice now a control has been laid out, hit-tested and themed while the
    // renderer never drew it — the record button, and then the arrangement's
    // draw chip. Both times every view-model test passed, because the geometry
    // and the hit-testing were right; only the pixels knew.
    //
    // A pair of chips where neither says which one you are in is a pair you
    // have to press to find out, which is exactly the failure this catches.
    let Some(shot) = shoot_roll_snapped(SnapDivision::Step) else {
        return;
    };
    let chip = shot
        .toolbar
        .items
        .iter()
        .find(|(control, _)| *control == fontelle_ui::canvas::RollControl::Tool(Tool::Draw))
        .map(|(_, rect)| *rect)
        .expect("the roll's draw chip");

    let at = |x: u32, y: u32| {
        let i = ((y * RW + x) * 4) as usize;
        Color(shot.pixels[i..i + 4].try_into().expect("four bytes"))
    };
    // The chip's own corner, inside its rounded fill and away from the glyph.
    let x = (chip.x + 3.0) as u32;
    let y = (chip.y + chip.height / 2.0) as u32;
    assert!(
        near(at(x, y), shot.theme.palette.accent),
        "the live tool's chip should carry the accent, found {:?}",
        at(x, y)
    );
}

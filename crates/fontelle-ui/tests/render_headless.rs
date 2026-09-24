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
    // GitHub's Windows runner has no GPU and its software adapter dies with
    // an access violation inside the driver — the test process, not a test,
    // so no assertion can catch it. Vulkan (Linux) and Metal (macOS) render
    // these frames on their runners; a Windows machine with a real adapter
    // runs them too.
    if cfg!(windows) && std::env::var_os("CI").is_some() {
        eprintln!("skipping: no GPU on this runner");
        return None;
    }
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
    /// How wide the frame is, because not every shot is [`W`] across — the
    /// transport bar has controls it only has room for on a real window.
    width: u32,
}

impl Shot {
    fn at(&self, x: u32, y: u32) -> Color {
        let i = ((y * self.width + x) * 4) as usize;
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
    shoot_mode(theme, view, meters, false)
}

/// The same, saying which mode the transport is in — what lights the song/clip
/// chip.
fn shoot_mode(
    theme: Theme,
    view: TransportView,
    meters: [Meter; 2],
    clip_mode: bool,
) -> Option<Shot> {
    shoot_sized(theme, view, meters, clip_mode, W)
}

/// The same, at a given width. The window opens wider than [`W`] in practice
/// and the transport bar drops controls it has no room for, so a shot of one
/// of those has to be taken at a width it is there at.
fn shoot_sized(
    theme: Theme,
    view: TransportView,
    meters: [Meter; 2],
    clip_mode: bool,
    width: u32,
) -> Option<Shot> {
    let shared = headless()?;
    let layout = window_layout(
        width as f32,
        H as f32,
        &theme.metrics,
        DEFAULT_TIMELINE_HEIGHT,
    );
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
            field: None,
            panel_title: &title,
            transport: TransportChrome {
                layout: bar,
                view,
                meters,
                readout: &readout,
                tempo: &tempo,
                signature: &signature,
                mode: &tempo,
                hover: None,
                marker_sample: 0,
                clip_mode,
                tempo_field: None,
            },
            roll: None,
            rack: None,
            prefabs: None,
            browser: None,
            timeline: None,
            mixer: None,
            tabs: fontelle_ui::layout::editor_tabs(layout.panel.header, &theme.metrics),
            tab: fontelle_ui::layout::EditorTab::Roll,
            hover_tab: None,
            browser_title: "Soundfonts",
            labels: &Labels::new(),
            status: "",
            toast: None,
            confirm: None,
            notices: Default::default(),
            tooltip: None,
            menu: None,
            carry: None,
            welcome: None,
            keybinds: None,
        },
    );
    let pixels = shared
        .lock()
        .expect("the shared renderer")
        .render(&scene, width, H, theme.palette.window)
        .expect("rendering a scene that fits in memory");

    // `dump_sized`, because this one is not the standard frame: a shot at
    // any other width written as a standard-sized PNG is a panic in the
    // encoder, and it fires only when `FONTELLE_UI_DUMP` is set — which is
    // exactly when somebody is trying to look at the window.
    dump_sized(&pixels, &theme.name, width, H);

    Some(Shot {
        pixels,
        theme,
        layout,
        bar,
        width,
    })
}

/// Writes the frame out as a PNG when `FONTELLE_UI_DUMP` names a directory.
///
/// §2.5 of `docs/first-usable-plan.md` makes "the pixels have been seen once by
/// a human" half of the done-criterion for a GUI item. On a machine with no
/// display — CI, a remote session, a Wayland compositor whose root an X11
/// screen-grabber cannot see — this is the only way to satisfy it, and it costs
/// nothing when the variable is unset.
///
/// **The size is always passed in.** There used to be a `dump` beside this that
/// filled in `W`/`H` for you, and the one shot in the file that is not that
/// size — the rack's, which is deliberately taller so two rows fit — called it
/// and handed a 640x480 buffer to a 640x360 encoder. The failure was invisible
/// until somebody set the variable, because the *only* thing the wrapper did
/// was guess, and it guessed nowhere else. A convenience whose whole body is an
/// assumption about its caller is worth less than the line it saves.
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
        channel: None,
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
        0,
    )
}

/// [`shoot_roll`] over a channel whose instrument only plays some keys — a
/// drum kit — laid out with the key strip that map asks for.
///
/// The widened strip is rendered nowhere else, so without this the only thing
/// checking `NAMED_KEYBOARD_WIDTH` is arithmetic in `tests/keyboard.rs`.
fn shoot_roll_mapped(map: &fontelle_ui::document::KeyMap) -> Option<RollShot> {
    shoot_roll_with(
        &Arena::default(),
        &[],
        &[],
        None,
        map,
        SnapDivision::Step,
        0,
    )
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
        0,
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
        0,
    )
}

/// [`shoot_roll`] with keys held down on a MIDI keyboard.
fn shoot_roll_lit(live_keys: u128) -> Option<RollShot> {
    shoot_roll_with(
        &Arena::default(),
        &[],
        &[],
        None,
        &fontelle_ui::document::KeyMap::unknown(),
        SnapDivision::Step,
        live_keys,
    )
}

/// The roll with a clip **end**, so the grid past it can be looked at.
fn shoot_roll_ending(clip_length: Tick) -> Option<RollShot> {
    let notes = Arena::default();
    let key_map = fontelle_ui::document::KeyMap::unknown();
    shoot_roll_full_ending(
        &notes,
        &[],
        &[],
        None,
        &key_map,
        SnapDivision::Step,
        0,
        false,
        Some(clip_length),
    )
}

fn shoot_roll_with(
    notes: &Arena<NoteId, Note>,
    selection: &[NoteId],
    ghosts: &[fontelle_ui::document::GhostNote],
    lane_menu: Option<fontelle_ui::canvas::LaneMenu>,
    key_map: &fontelle_ui::document::KeyMap,
    snap: SnapDivision,
    live_keys: u128,
) -> Option<RollShot> {
    shoot_roll_full(
        notes, selection, ghosts, lane_menu, key_map, snap, live_keys, false,
    )
}

/// The same, with the Tools panel open over the grid.
#[allow(clippy::too_many_arguments)]
fn shoot_roll_full(
    notes: &Arena<NoteId, Note>,
    selection: &[NoteId],
    ghosts: &[fontelle_ui::document::GhostNote],
    lane_menu: Option<fontelle_ui::canvas::LaneMenu>,
    key_map: &fontelle_ui::document::KeyMap,
    snap: SnapDivision,
    live_keys: u128,
    tools_open: bool,
) -> Option<RollShot> {
    shoot_roll_full_ending(
        notes, selection, ghosts, lane_menu, key_map, snap, live_keys, tools_open, None,
    )
}

#[allow(clippy::too_many_arguments)]
fn shoot_roll_full_ending(
    notes: &Arena<NoteId, Note>,
    selection: &[NoteId],
    ghosts: &[fontelle_ui::document::GhostNote],
    lane_menu: Option<fontelle_ui::canvas::LaneMenu>,
    key_map: &fontelle_ui::document::KeyMap,
    snap: SnapDivision,
    live_keys: u128,
    tools_open: bool,
    clip_length: Option<Tick>,
) -> Option<RollShot> {
    shoot_roll_everything(
        notes,
        selection,
        ghosts,
        lane_menu,
        key_map,
        snap,
        live_keys,
        tools_open,
        clip_length,
        &[],
        None,
    )
}

/// [`shoot_roll`] with the cut tool's stroke in progress from `from` to
/// `to`, in window points.
fn shoot_roll_slicing(
    notes: &Arena<NoteId, Note>,
    from: (f32, f32),
    to: (f32, f32),
) -> Option<RollShot> {
    shoot_roll_everything(
        notes,
        &[],
        &[],
        None,
        &fontelle_ui::document::KeyMap::unknown(),
        SnapDivision::Step,
        0,
        false,
        None,
        &[],
        Some((from, to)),
    )
}

/// [`shoot_roll`] while a take is being recorded: `takes` are the notes the
/// capture has caught so far, drawn but not yet in the document.
fn shoot_roll_recording(
    notes: &Arena<NoteId, Note>,
    takes: &[fontelle_ui::document::NotePreview],
) -> Option<RollShot> {
    shoot_roll_everything(
        notes,
        &[],
        &[],
        None,
        &fontelle_ui::document::KeyMap::unknown(),
        SnapDivision::Step,
        0,
        false,
        None,
        takes,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn shoot_roll_everything(
    notes: &Arena<NoteId, Note>,
    selection: &[NoteId],
    ghosts: &[fontelle_ui::document::GhostNote],
    lane_menu: Option<fontelle_ui::canvas::LaneMenu>,
    key_map: &fontelle_ui::document::KeyMap,
    snap: SnapDivision,
    live_keys: u128,
    tools_open: bool,
    clip_length: Option<Tick>,
    takes: &[fontelle_ui::document::NotePreview],
    slice: Option<((f32, f32), (f32, f32))>,
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
        key_offset: 0.0,
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

    // The Tools panel, when it is open, and every caption on it — a row
    // nobody shaped draws as nothing at all, which would make an empty panel
    // look like a pass.
    let tools = fontelle_ui::canvas::Tools::default();
    let tools_panel = tools_open.then(|| {
        let chip = toolbar_layout(roll_l.toolbar, &theme.metrics)
            .items
            .iter()
            .find(|(control, _)| *control == fontelle_ui::canvas::RollControl::Tools)
            .map(|(_, rect)| *rect)
            .unwrap_or(Rect::ZERO);
        fontelle_ui::canvas::tools_dialog_layout(
            fontelle_ui::canvas::ToolKind::Adjust,
            chip,
            roll_l.frame,
            &theme.metrics,
        )
    });
    if let Some(panel) = &tools_panel {
        labels.ensure(panel.kind.title(), &theme.font, &mut text);
        for (row, _) in &panel.rows {
            for caption in [tools.label(*row), tools.value(*row)] {
                if !caption.is_empty() {
                    labels.ensure(&caption, &theme.font, &mut text);
                }
            }
        }
    }
    labels.ensure(
        &fontelle_ui::canvas::tools_caption(),
        &theme.font,
        &mut text,
    );

    let mut scene = vello::Scene::new();
    draw_window(
        &mut scene,
        &theme,
        &layout,
        &Chrome {
            field: None,
            panel_title: &title,
            transport: TransportChrome {
                layout: transport_bar_layout(layout.transport, &theme.metrics),
                view,
                meters: [Meter::new(); 2],
                readout: &readout,
                tempo: &tempo,
                signature: &signature,
                mode: &tempo,
                hover: None,
                marker_sample: 0,
                clip_mode: false,
                tempo_field: None,
            },
            roll: Some(RollChrome {
                focused: false,
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
                clip_length,
                loop_range: None,
                marquee: None,
                hover: None,
                lane_menu: lane_menu.as_ref(),
                tools_panel: tools_panel.as_ref(),
                tools: &tools,
                slice,
                key_style: fontelle_ui::canvas::KeyStyle::Piano,
                live_keys,
                recording: takes,
            }),
            rack: None,
            prefabs: None,
            browser: None,
            timeline: None,
            mixer: None,
            tabs: fontelle_ui::layout::editor_tabs(layout.panel.header, &theme.metrics),
            tab: fontelle_ui::layout::EditorTab::Roll,
            hover_tab: None,
            browser_title: "Soundfonts",
            labels: &labels,
            status: "",
            toast: None,
            confirm: None,
            notices: Default::default(),
            tooltip: None,
            menu: None,
            carry: None,
            welcome: None,
            keybinds: None,
        },
    );
    let pixels = shared
        .lock()
        .expect("the shared renderer")
        .render(&scene, RW, RH, theme.palette.window)
        .expect("rendering a scene that fits in memory");

    dump_sized(
        &pixels,
        if slice.is_some() {
            "roll-cut"
        } else if tools_open {
            "roll-tools"
        } else if key_map.is_known() {
            "roll-keymap"
        } else if live_keys != 0 {
            "roll-lit"
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

/// The cut tool's marks: a diagonal stroke through a chord draws a bright
/// mark across each note at the tick it will be cut on, and the stroke itself
/// is faint — the arrangement's rule, brought to the roll.
///
/// > *"it would be nice when im cutting in the piano roll it drew the line
/// > that cut the notes i was cutting with the cut tool to visualize it
/// > cleanly."*
#[test]
fn the_blade_marks_each_note_it_will_cut() {
    let mut notes = Arena::default();
    for key in [60, 64, 67] {
        notes.insert(note(0, PPQN * 4, key));
    }
    let theme = Theme::dark_default();
    let layout = window_layout(RW as f32, RH as f32, &theme.metrics, 0.0);
    let roll_l = fontelle_ui::canvas::roll_layout_with_keys(
        layout.panel.body,
        &theme.metrics,
        DEFAULT_LANE_HEIGHT,
        fontelle_ui::canvas::keyboard_width(&fontelle_ui::document::KeyMap::unknown()),
    );
    let view = RollView {
        top_key: 72,
        key_offset: 0.0,
        ..RollView::default()
    };
    let row_mid =
        |key: u8| fontelle_ui::canvas::key_to_y(&view, roll_l.grid, key) + view.key_height / 2.0;
    let from = (tick_to_x(&view, roll_l.grid, PPQN), row_mid(67) - 10.0);
    let to = (tick_to_x(&view, roll_l.grid, PPQN * 3), row_mid(60) + 10.0);
    let Some(shot) = shoot_roll_slicing(&notes, from, to) else {
        return;
    };

    let marks = fontelle_ui::canvas::note_marks(&shot.view, shot.layout.grid, &notes, from, to);
    assert_eq!(marks.len(), 3, "three notes crossed, three marks");
    for mark in &marks {
        let x = (mark.x + mark.width / 2.0) as u32;
        let y = (mark.y + mark.height / 2.0) as u32;
        let found = shot.at(x, y);
        assert!(
            near(found, shot.theme.palette.meter_peak),
            "expected a cut mark at ({x}, {y}), found {found:?}"
        );
    }
    // The three marks are at three different x: a diagonal cuts each row at
    // its own time, and the picture says so.
    assert!(marks[0].x != marks[1].x && marks[1].x != marks[2].x);
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
fn a_key_held_on_a_midi_keyboard_lights_up_on_the_roll_keyboard() {
    // Asked for from playing the studio: seeing what you just played on the
    // keyboard down the side is how a phrase gets written into the grid.
    let (Some(quiet), Some(lit)) = (shoot_roll_lit(0), shoot_roll_lit(1u128 << 60)) else {
        return;
    };
    // The right-hand end of the key, clear of the C marker down its left edge
    // and of the caption written beside it.
    let x = lit.layout.keys.right() as u32 - 4;
    let y = (fontelle_ui::canvas::key_to_y(&lit.view, lit.layout.grid, 60)
        + lit.view.key_height / 2.0) as u32;
    assert_ne!(
        quiet.at(x, y),
        lit.at(x, y),
        "a key held down looks exactly like one that is not"
    );
    assert!(
        near(lit.at(x, y), lit.theme.palette.accent),
        "expected the held key lit in the accent; found {:?}",
        lit.at(x, y)
    );
}

#[test]
fn only_the_key_being_played_lights_up() {
    let (Some(quiet), Some(lit)) = (shoot_roll_lit(0), shoot_roll_lit(1u128 << 60)) else {
        return;
    };
    let x = lit.layout.keys.right() as u32 - 4;
    for key in [59u8, 61, 62, 67] {
        let y = (fontelle_ui::canvas::key_to_y(&lit.view, lit.layout.grid, key)
            + lit.view.key_height / 2.0) as u32;
        assert_eq!(
            quiet.at(x, y),
            lit.at(x, y),
            "key {key} is not down and must not have changed"
        );
    }
}

#[test]
fn a_black_key_lights_up_too() {
    // The accidentals are drawn as a short dark bar over the naturals, and
    // lighting only the strip behind one would light the neighbour it sits on
    // rather than the key that is down.
    let (Some(quiet), Some(lit)) = (shoot_roll_lit(0), shoot_roll_lit(1u128 << 61)) else {
        return;
    };
    // Inside the black bar, which runs 62% of the way across the strip.
    let x = (lit.layout.keys.x + lit.layout.keys.width * 0.3) as u32;
    let y = (fontelle_ui::canvas::key_to_y(&lit.view, lit.layout.grid, 61)
        + lit.view.key_height / 2.0) as u32;
    assert_ne!(quiet.at(x, y), lit.at(x, y), "C#4 is down and unlit");
    assert!(
        near(lit.at(x, y), lit.theme.palette.accent),
        "expected the held accidental lit in the accent; found {:?}",
        lit.at(x, y)
    );
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
    shoot_timeline_selected(clips, &[])
}

/// The same, with some of the clips selected — what a block looks like once
/// you have clicked it.
fn shoot_timeline_selected(
    clips: &[fontelle_ui::document::ClipInfo],
    selection: &[fontelle_types::ClipId],
) -> Option<TimelineShot> {
    shoot_timeline_switch(clips, selection, false)
}

/// The same again, with the Stretch switch in a known state and the toolbar's
/// own words shaped, so a shot can show which way it is set.
fn shoot_timeline_switch(
    clips: &[fontelle_ui::document::ClipInfo],
    selection: &[fontelle_types::ClipId],
    stretch: bool,
) -> Option<TimelineShot> {
    shoot_timeline_recording(clips, selection, stretch, &[], None)
}

/// The same again, with the pointer over one part of one block.
fn shoot_timeline_hovered(
    clips: &[fontelle_ui::document::ClipInfo],
    hover_clip: Option<(fontelle_types::ClipId, fontelle_ui::canvas::ClipPart)>,
) -> Option<TimelineShot> {
    shoot_timeline_recording(clips, &[], false, &[], hover_clip)
}

/// The same again, with the notes of a take being recorded into the open
/// clip, and the pointer over one part of one block.
fn shoot_timeline_recording(
    clips: &[fontelle_ui::document::ClipInfo],
    selection: &[fontelle_types::ClipId],
    stretch: bool,
    takes: &[fontelle_ui::document::NotePreview],
    hover_clip: Option<(fontelle_types::ClipId, fontelle_ui::canvas::ClipPart)>,
) -> Option<TimelineShot> {
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
    // The toolbar's own words, so the shot shows what each chip says — the
    // snap division and the stretch switch are read-outs, not glyphs.
    let mut labels = Labels::new();
    for (control, _) in
        &fontelle_ui::canvas::timeline_toolbar_layout(l.toolbar, &theme.metrics).items
    {
        let caption = match control {
            fontelle_ui::canvas::TimelineControl::Snap => TimelineView::default().snap.label(),
            other => other.label(),
        };
        labels.ensure(caption, &theme.font, &mut text);
    }
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
            field: None,
            panel_title: &title,
            transport: TransportChrome {
                layout: transport_bar_layout(layout.transport, &theme.metrics),
                view,
                meters: [Meter::new(); 2],
                readout: &readout,
                tempo: &tempo,
                signature: &signature,
                mode: &tempo,
                hover: None,
                marker_sample: 0,
                clip_mode: false,
                tempo_field: None,
            },
            roll: None,
            rack: None,
            prefabs: None,
            browser: None,
            timeline: Some(TimelineChrome {
                tool: fontelle_ui::canvas::TimelineTool::default(),
                stretch,
                toolbar: fontelle_ui::canvas::timeline_toolbar_layout(l.toolbar, &theme.metrics),
                hover: None,
                hover_clip,
                fading: None,
                can_paste: false,
                panel: layout.timeline,
                layout: l,
                view: tview,
                lanes: &lanes,
                clips,
                selection,
                playhead_tick: 0,
                marker_tick: 0,
                beats_per_bar: 4,
                marquee: None,
                slice: None,
                focused: false,
                renaming: None,
                rename: None,
                point_clip: None,
                point_selection: &[],
                loop_range: None,
                recording: None,
                take_notes: takes,
            }),
            mixer: None,
            tabs: fontelle_ui::layout::editor_tabs(layout.panel.header, &theme.metrics),
            tab: fontelle_ui::layout::EditorTab::Roll,
            hover_tab: None,
            browser_title: "Soundfonts",
            labels: &labels,
            status: "",
            toast: None,
            confirm: None,
            notices: Default::default(),
            tooltip: None,
            menu: None,
            carry: None,
            welcome: None,
            keybinds: None,
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
        notes: Vec::new(),
        audio: Default::default(),
        prefab: None,
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
fn shoot_instrument() -> Option<(Vec<u8>, Theme, fontelle_ui::canvas::InstrumentLayout, u32)> {
    use fontelle_types::ParamAddress;
    use fontelle_ui::canvas::{
        InstrumentGroup, InstrumentParam, InstrumentView, ParamKind, instrument_layout,
    };
    use fontelle_ui::render::InstrumentChrome;

    let theme = Theme::dark_default();
    let shared = headless()?;
    let mut text = TextContext::new();
    // The window's header, which is its whole chrome: no transport bar, no
    // rack, no browser. That is what a floating editor is.
    let title = text.layout("Instrument \u{2014} Piano", &theme.font, None);

    let knob = |name: &str, value: f32, display: &str| InstrumentParam {
        address: ParamAddress::new(format!("patch/{name}")),
        label: name.to_string(),
        value,
        display: display.to_string(),
        kind: ParamKind::Knob,
        automated: false,
    };
    let instrument = InstrumentView {
        keys: Vec::new(),
        key: None,
        title: "tri baja".to_string(),
        groups: vec![
            InstrumentGroup {
                name: "Channel".to_string(),
                // `pan` is under automation and `volume` is not, so the two
                // cells can be compared against each other in the same shot.
                params: vec![
                    knob("volume", 0.8, "+0.0 dB"),
                    InstrumentParam {
                        automated: true,
                        ..knob("pan", 0.5, "centre")
                    },
                ],
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
                        automated: false,
                    },
                    InstrumentParam {
                        address: ParamAddress::new("patch/filter[0]/mode"),
                        label: "mode".to_string(),
                        value: 0.0,
                        display: "LP".to_string(),
                        kind: ParamKind::Choice(
                            ["LP", "HP", "BP", "Notch"].map(str::to_string).to_vec(),
                        ),
                        automated: false,
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

    // The instrument's own window, not a tab of the main one — see
    // `fontelle_ui::layout::EditorKind`. Shot at the size it opens at.
    let (ew, eh) = fontelle_ui::layout::EditorKind::Instrument.default_size();
    let panel = fontelle_ui::layout::editor_window_layout(ew as f32, eh as f32, &theme.metrics);
    let l = instrument_layout(panel.body, &theme.metrics, &instrument);
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
    fontelle_ui::render::draw_editor_window(
        &mut scene,
        &theme,
        &panel,
        &labels,
        &title,
        &fontelle_ui::render::EditorWindowChrome::Instrument(Some(InstrumentChrome {
            hover_preset: None,
            hover_key: None,
            layout: l.clone(),
            view: &instrument,
            hover: None,
            active: Some((1, 2)),
        })),
        None,
        None,
        None,
        None,
    );
    let pixels = shared
        .lock()
        .expect("the shared renderer")
        .render(&scene, ew, eh, theme.palette.window)
        .expect("the scene must render");
    dump_sized(&pixels, "instrument", ew, eh);
    Some((pixels, theme, l, ew))
}

#[test]
fn the_instrument_window_draws_a_control_for_every_parameter() {
    let Some((pixels, theme, l, width)) = shoot_instrument() else {
        return;
    };
    let at = |x: u32, y: u32| {
        let i = ((y * width + x) * 4) as usize;
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
    shoot_mixer_renaming(None)
}

/// The same, with strip `index` mid-rename and the field's marks — where
/// the caret is and what is selected, in points from the name's left.
fn shoot_mixer_renaming(
    renaming: Option<(usize, fontelle_ui::render::RenameMarks)>,
) -> Option<(Vec<u8>, Theme, fontelle_ui::canvas::MixerLayout)> {
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
        sends: Vec::new(),
    };
    let chain = |label: &str, bypassed: bool| fontelle_ui::canvas::InsertInfo {
        label: label.to_string(),
        bypassed,
        mix: 1.0,
        mix_automated: false,
    };
    let mut strips = vec![
        // A **full** chain on the selected strip, so the shot shows both the
        // track-options column doing the naming and the strip's one-row
        // read-out standing in for it — and, most of the point, that six
        // effects cost this fader nothing against the strips beside it.
        MixerStrip {
            inserts: vec![
                chain("Gate", false),
                chain("Comp", false),
                chain("Dist", false),
                chain("EQ", true),
                chain("Delay", false),
                chain("Reverb", false),
            ],
            ..strip("Drums", 0.0, 0.0, false)
        },
        strip("Bass", -18.0, -0.8, false),
        strip("Keys", 3.0, 0.5, true),
        MixerStrip {
            is_master: true,
            ..strip("Master", 0.0, 0.0, false)
        },
    ];
    // A reverb send off the selected strip, so the shot shows what the
    // sends half of the column is for.
    let send_names = ["Bass".to_string(), "Keys".to_string()];
    strips[0].sends = vec![
        fontelle_ui::document::SendInfo {
            target: 1,
            target_name: send_names[0].clone(),
            level_db: -12.0,
            pre_fader: false,
        },
        fontelle_ui::document::SendInfo {
            target: 2,
            target_name: send_names[1].clone(),
            level_db: -30.0,
            pre_fader: true,
        },
    ];
    let route_names: Vec<String> = strips.iter().map(|s| s.name.clone()).collect();
    let output_label = fontelle_ui::render::output_label("Master");
    let peaks = vec![[0.8, 0.6], [0.2, 0.2], [0.0, 0.0], [0.9, 0.9]];

    let l = mixer_layout(layout.panel.body, &theme.metrics, &strips, 0);
    let mut labels = Labels::new();
    for caption in [
        "Piano roll",
        "Instrument",
        "Mixer",
        "M",
        "S",
        fontelle_ui::render::ADD_TRACK,
        fontelle_ui::render::EFFECTS_HEADING,
        fontelle_ui::render::ADD_EFFECT,
        fontelle_ui::render::GRIP,
        fontelle_ui::render::REMOVE,
        fontelle_ui::render::SENDS_HEADING,
        fontelle_ui::render::ADD_SEND,
        fontelle_ui::render::SEND_PRE,
        fontelle_ui::render::SEND_POST,
    ] {
        labels.ensure(caption, &theme.font, &mut text);
    }
    labels.ensure(&output_label, &theme.font, &mut text);
    labels.ensure("In: none", &theme.font, &mut text);
    for s in &strips {
        labels.ensure(&s.name, &theme.font, &mut text);
        labels.ensure(&format_gain_db(s.gain_db), &theme.font, &mut text);
        for insert in &s.inserts {
            labels.ensure(&insert.label, &theme.font, &mut text);
        }
        for send in &s.sends {
            labels.ensure(&send.target_name, &theme.font, &mut text);
            labels.ensure(
                &fontelle_ui::canvas::format_send_db(send.level_db),
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
            field: None,
            panel_title: &title,
            transport: TransportChrome {
                layout: transport_bar_layout(layout.transport, &theme.metrics),
                view,
                meters: [Meter::new(); 2],
                readout: &readout,
                tempo: &tempo,
                signature: &signature,
                mode: &tempo,
                hover: None,
                marker_sample: 0,
                clip_mode: false,
                tempo_field: None,
            },
            roll: None,
            rack: None,
            prefabs: None,
            browser: None,
            timeline: None,
            mixer: Some(MixerChrome {
                layout: l.clone(),
                strips: &strips,
                peaks: &peaks,
                hover: None,
                active: None,
                selected: 0,
                renaming: renaming.as_ref().map(|(index, _)| *index),
                rename: renaming.as_ref().map(|(_, marks)| *marks),
                output_label: output_label.clone(),
                input_label: "In: none".to_string(),
                insert_drag: None,
                output_menu: None,
                send_menu: None,
                route_names: &route_names,
                output: None,
            }),
            tabs: editor_tabs(layout.panel.header, &theme.metrics),
            tab: EditorTab::Mixer,
            hover_tab: None,
            browser_title: "Soundfonts",
            labels: &labels,
            status: "",
            toast: None,
            confirm: None,
            notices: Default::default(),
            tooltip: None,
            menu: None,
            carry: None,
            welcome: None,
            keybinds: None,
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
fn a_rename_with_everything_selected_shows_the_selection_and_the_caret_where_it_is() {
    // > *"in the mixer track when typing its not showing selection
    // > highlights like when i do ctrl a for example"*
    //
    // A rename is a field like the name prompt: the selection is a wash
    // under the text and the caret sits where the caret *is*, not at the
    // end of the name whatever was typed.
    use fontelle_ui::render::RenameMarks;
    let Some((plain, theme, l)) = shoot_mixer() else {
        return;
    };
    let name_width = {
        let mut text = TextContext::new();
        text.layout("Bass", &theme.font, None).width
    };
    let at = |pixels: &[u8], x: u32, y: u32| {
        let i = ((y * RW + x) * 4) as usize;
        Color(pixels[i..i + 4].try_into().unwrap())
    };
    let name = l.strips[1].name;
    let (px, py) = (name.x as u32 + 4, (name.y + name.height / 2.0) as u32);

    // Everything selected, caret at the end: the wash covers the name.
    let (selected, ..) = shoot_mixer_renaming(Some((
        1,
        RenameMarks {
            caret_x: name_width,
            selection: Some((0.0, name_width)),
            caret_on: true,
        },
    )))
    .unwrap();
    assert_ne!(
        at(&selected, px, py),
        at(&plain, px, py),
        "the selection wash has to change the pixels under the name"
    );

    // Nothing selected, caret in its on-half at the *start*: an accent
    // column at the name's left, and none at its end.
    let (caret_at_start, ..) = shoot_mixer_renaming(Some((
        1,
        RenameMarks {
            caret_x: 0.0,
            selection: None,
            caret_on: true,
        },
    )))
    .unwrap();
    let text_x = name.x as u32 + 3;
    let column = |pixels: &[u8], x: u32| {
        (name.y as u32 + 3..(name.y + name.height) as u32 - 3)
            .any(|y| near(at(pixels, x, y), theme.palette.accent))
    };
    assert!(
        column(&caret_at_start, text_x),
        "a caret at the start of the name"
    );
    assert!(
        !column(&caret_at_start, text_x + name_width as u32 + 1),
        "and not at its end"
    );
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

/// A knob a lane has taken over is drawn with a **different ring**, which is
/// TDD §12.2's own words for it.
///
/// The claim this makes is not "something is drawn" — it is that the ring is
/// tellable from an ordinary knob's *in the same picture*, which is the only
/// form of the claim that means anything to somebody looking at the panel.
/// `is_automated` was implemented, answered by the session and covered by a
/// test long before anything drew it, and that is exactly the shape of defect
/// this file exists to catch.
#[test]
fn a_knob_under_automation_wears_a_ring_an_ordinary_knob_does_not() {
    let Some((pixels, theme, l, width)) = shoot_instrument() else {
        return;
    };
    let at = |x: u32, y: u32| {
        let i = ((y * width + x) * 4) as usize;
        Color(pixels[i..i + 4].try_into().expect("four bytes"))
    };
    let cell = |group: usize, param: usize| {
        l.cells
            .iter()
            .find(|(g, p, _)| (*g, *p) == (group, param))
            .map(|(_, _, r)| *r)
            .expect("that cell is on the panel")
    };
    let ring_pixels = |rect: fontelle_ui::layout::Rect| {
        let mut count = 0;
        for dy in 0..rect.height as u32 {
            for dx in 0..rect.width as u32 {
                if near(
                    at(rect.x as u32 + dx, rect.y as u32 + dy),
                    theme.palette.param_automated,
                ) {
                    count += 1;
                }
            }
        }
        count
    };

    // `pan`, which `shoot_instrument` marks as automated, against `volume`
    // beside it, which it does not.
    let automated = ring_pixels(cell(0, 1));
    let ordinary = ring_pixels(cell(0, 0));
    assert!(
        automated > 20,
        "the automated knob drew {automated} pixels of the ring colour"
    );
    assert_eq!(
        ordinary, 0,
        "an ordinary knob drew the automation ring colour {ordinary} times"
    );
}

// ------------------------------------------------- the caret on a name

/// Shoots the channel rack, optionally with one row being renamed.
fn shoot_rack(
    renaming: Option<usize>,
) -> Option<(Vec<u8>, Theme, fontelle_ui::canvas::RackLayout, u32, u32)> {
    use fontelle_ui::canvas::rack_layout;
    use fontelle_ui::document::ChannelInfo;
    use fontelle_ui::render::RackChrome;

    let theme = Theme::dark_default();
    let shared = headless()?;
    let mut text = TextContext::new();
    let title = text.layout("Fontelle", &theme.font, None);

    let channels: Vec<ChannelInfo> = ["Bass", "Keys"]
        .iter()
        .map(|name| ChannelInfo {
            name: (*name).to_string(),
            muted: false,
            soloed: false,
            has_instrument: true,
            route: None,
        })
        .collect();

    // Taller than the file's usual frame: the rack's body in a 360-pixel
    // window has room for one row once the panel's tab strip
    // (`canvas::tab_strip`) has taken its own, and this test needs two rows to
    // compare against each other.
    const RACK_H: u32 = 480;
    let layout = window_layout(
        W as f32,
        RACK_H as f32,
        &theme.metrics,
        DEFAULT_TIMELINE_HEIGHT,
    );
    let rack = rack_layout(layout.rack.body, &theme.metrics, channels.len(), 0);

    let mut labels = Labels::new();
    for channel in &channels {
        labels.ensure(&channel.name, &theme.font, &mut text);
    }
    labels.ensure("Master", &theme.font, &mut text);
    for tab in fontelle_ui::document::RackTab::ALL {
        labels.ensure(tab.label(), &theme.font, &mut text);
    }

    let mut scene = vello::Scene::new();
    fontelle_ui::render::draw_window(
        &mut scene,
        &theme,
        &layout,
        &Chrome {
            field: None,
            panel_title: &title,
            transport: TransportChrome {
                layout: transport_bar_layout(layout.transport, &theme.metrics),
                view: TransportView::unavailable(),
                meters: [Meter::new(); 2],
                readout: &text.layout("1.1.0", &theme.font, None),
                tempo: &text.layout("120.00", &theme.font, None),
                signature: &text.layout("4/4", &theme.font, None),
                mode: &text.layout("Song", &theme.font, None),
                hover: None,
                marker_sample: 0,
                clip_mode: false,
                tempo_field: None,
            },
            roll: None,
            rack: Some(RackChrome {
                panel: layout.rack,
                layout: rack.clone(),
                channels: &channels,
                selected: 0,
                hover: None,
                route_names: &["Master".to_string()],
                strips: 1,
                route_menu: None,
                route_menu_open: None,
                renaming,
                rename: None,
            }),
            prefabs: None,
            browser: None,
            timeline: None,
            mixer: None,
            tabs: fontelle_ui::layout::editor_tabs(layout.panel.header, &theme.metrics),
            tab: fontelle_ui::layout::EditorTab::Roll,
            hover_tab: None,
            browser_title: "Soundfonts",
            labels: &labels,
            status: "",
            toast: None,
            confirm: None,
            notices: Default::default(),
            tooltip: None,
            menu: None,
            carry: None,
            welcome: None,
            keybinds: None,
        },
    );
    let pixels = shared
        .lock()
        .expect("the shared renderer")
        .render(&scene, W, RACK_H, theme.palette.window)
        .expect("the scene must render");
    dump_sized(
        &pixels,
        &format!("rack-renaming-{}", renaming.is_some()),
        W,
        RACK_H,
    );
    Some((pixels, theme, rack, W, RACK_H))
}

/// **A name being typed has a visible text cursor.**
///
/// Right-click → Rename put every keystroke straight into the document and the
/// row updated live, which is the right mechanism and gave no sign that the
/// keyboard had been captured: the row looked exactly like a row nobody was
/// typing into, and the only way to find out was to press a letter and watch
/// what happened. The search box has had a caret for this reason since it was
/// written; this is the same claim for a row's name.
#[test]
fn a_row_being_renamed_shows_a_caret_and_the_others_do_not() {
    let Some((quiet, theme, rack, width, _)) = shoot_rack(None) else {
        return;
    };
    let Some((typing, _, _, _, _)) = shoot_rack(Some(0)) else {
        return;
    };
    let accent_in = |pixels: &[u8], rect: fontelle_ui::layout::Rect| {
        let mut count = 0;
        for dy in 0..rect.height as u32 {
            for dx in 0..rect.width as u32 {
                let x = rect.x as u32 + dx;
                let y = rect.y as u32 + dy;
                let i = ((y * width + x) * 4) as usize;
                if near(
                    Color(pixels[i..i + 4].try_into().expect("four bytes")),
                    theme.palette.accent,
                ) {
                    count += 1;
                }
            }
        }
        count
    };

    let first = rack.rows.first().expect("a first row").name;
    let second = rack.rows.get(1).expect("a second row").name;

    assert!(
        accent_in(&typing, first) > accent_in(&quiet, first),
        "the row being renamed should gain a caret it did not have"
    );
    assert_eq!(
        accent_in(&typing, second),
        accent_in(&quiet, second),
        "and the row nobody is typing into should be unchanged"
    );
}

// ------------------------------- an automation block draws its own curve ---
//
// *"i want the automation graph to be a literal graph drawn inside the clip."*
// The geometry is `tests/automation_blocks.rs`; this is the half only a real
// frame can answer — that the curve is **on screen**, in ink you can see
// against the block it is drawn on. A curve whose colour matched its own
// background was a real bug in this project once, and no geometry test could
// have caught it.

/// A four-bar automation block on lane 1, with a ramp through it.
fn an_automation_clip(values: &[f64]) -> fontelle_ui::document::ClipInfo {
    let mut arena: Arena<fontelle_types::ClipId, ()> = Arena::default();
    let mut points: Arena<fontelle_types::PointId, ()> = Arena::default();
    let last = values.len().saturating_sub(1).max(1) as Tick;
    let length = PPQN * 16;
    fontelle_ui::document::ClipInfo {
        id: arena.insert(()),
        lane: 1,
        start: 0,
        length,
        name: "Master \u{2014} gain".to_string(),
        muted: false,
        open: false,
        color: [0xb4, 0xa2, 0xe8, 0xff],
        loop_length: None,
        kind: fontelle_ui::document::ClipKind::Automation,
        curve: values
            .iter()
            .enumerate()
            .map(|(i, value)| fontelle_ui::document::CurvePoint {
                id: points.insert(()),
                tick: length * i as Tick / last,
                value: *value,
                curve: fontelle_model::CurveShape::Linear,
            })
            .collect(),
        notes: Vec::new(),
        audio: Default::default(),
        prefab: None,
    }
}

#[test]
fn an_automation_block_draws_a_curve_you_can_see() {
    use fontelle_ui::canvas::{automation_block, clip_rect};

    let clips = vec![an_automation_clip(&[0.0, 1.0])];
    let Some(shot) = shoot_timeline(&clips) else {
        return;
    };
    let block = clip_rect(&shot.view, shot.layout.grid, &clips[0]);
    let area = automation_block(block, &clips[0]).area;

    // The curve's ink is the block's own colour, on the dark ground an
    // automation block is filled with. Both have to be on screen inside the
    // area, or the "curve" is a block with nothing in it.
    let ground = shot.theme.palette.panel_header;
    let ink = Color(clips[0].color);
    let mut found_ink = 0;
    let mut found_ground = 0;
    for x in (area.x as u32)..(area.right() as u32) {
        for y in (area.y as u32)..(area.bottom() as u32) {
            let px = shot.at(x, y);
            if near(px, ink) {
                found_ink += 1;
            }
            if near(px, ground) {
                found_ground += 1;
            }
        }
    }
    assert!(
        found_ink > 20,
        "the curve is not on screen: {found_ink} pixels of its ink in {area:?}"
    );
    assert!(
        found_ground > 20,
        "the block is filled with the curve's own colour, so the line cannot \
         be seen against it: {found_ground} pixels of ground"
    );
}

/// Reported from using the window: *"when an automation clip is selected i
/// cannot see the graph at all so i have to unselect it to see how it actually
/// looks before going back to trying to edit it how i want it."*
///
/// Exactly true, and for a reason no view-model test could reach. A selected
/// block was **filled** in the selection colour and its curve was **stroked**
/// in that same colour, so the line was drawn perfectly onto its own
/// background — the one moment you most need to see the shape, which is while
/// you are editing it, was the one moment it was gone.
///
/// The rule the fix holds to: an automation block keeps its dark ground
/// whatever else is true of it, because the ground is what the shape is read
/// against. Selection says so some other way.
#[test]
fn a_selected_automation_block_still_shows_its_curve() {
    use fontelle_ui::canvas::{automation_block, clip_rect};

    let clips = vec![an_automation_clip(&[0.0, 1.0])];
    let Some(shot) = shoot_timeline_selected(&clips, &[clips[0].id]) else {
        return;
    };
    let block = clip_rect(&shot.view, shot.layout.grid, &clips[0]);
    let area = automation_block(block, &clips[0]).area;

    // Whatever the curve is drawn in when the block is selected, there has to
    // be a *ground* left for it to be drawn against — a block painted edge to
    // edge in one colour is a block with no graph in it. So: count how many
    // distinct inks are inside the curve area.
    let ground = shot.theme.palette.panel_header;
    let mut on_ground = 0;
    for x in (area.x as u32)..(area.right() as u32) {
        for y in (area.y as u32)..(area.bottom() as u32) {
            if near(shot.at(x, y), ground) {
                on_ground += 1;
            }
        }
    }
    assert!(
        on_ground > 20,
        "a selected automation block is filled solid, so the curve is painted \
         onto its own colour and the graph cannot be seen at all: \
         {on_ground} pixels of ground in {area:?}"
    );
}

#[test]
fn a_rising_curve_is_drawn_rising() {
    // One is up. Drawn upside down the picture is a lie about the value, and
    // the mistake is invisible until you put it beside the numbers.
    use fontelle_ui::canvas::{automation_block, clip_rect};

    let clips = vec![an_automation_clip(&[0.0, 1.0])];
    let Some(shot) = shoot_timeline(&clips) else {
        return;
    };
    let block = clip_rect(&shot.view, shot.layout.grid, &clips[0]);
    let area = automation_block(block, &clips[0]).area;
    let ink = Color(clips[0].color);

    // The **nearest** colour rather than an exact match: a 1.5-pixel stroke
    // is anti-aliased, so most of a column's coverage is the ink blended into
    // the ground and an `assert` on the exact value finds nothing at the ends
    // of the line. What is being asked here is "where is the line in this
    // column", and that is the pixel least unlike its ink.
    let distance = |a: Color, b: Color| -> u32 {
        a.0[..3]
            .iter()
            .zip(b.0[..3].iter())
            .map(|(x, y)| u32::from(x.abs_diff(*y)))
            .sum()
    };
    let line_at = |x: u32| {
        ((area.y as u32)..(area.bottom() as u32))
            .map(|y| (distance(shot.at(x, y), ink), y))
            .min()
            .filter(|(d, _)| *d < 120)
            .map(|(_, y)| y)
    };
    let left = line_at(area.x as u32 + 2);
    let right = line_at(area.right() as u32 - 2);
    let (Some(left), Some(right)) = (left, right) else {
        panic!("the curve is missing at one end: {left:?} {right:?}");
    };
    assert!(
        right < left,
        "a curve from 0 to 1 has to end above where it started: {left} then {right}"
    );
    // And it genuinely crosses the block rather than sitting in a band: the
    // two ends are most of the area's height apart.
    assert!(
        left - right > (area.height as u32) / 2,
        "the curve barely moves: {left} to {right} in a {}-tall area",
        area.height
    );
}

// ---------------------------------- a note clip shows the notes in it ---
//
// *"make it so the midi clips in the arrangement arent just blank rectangles
// but instead actually show a preview of the notes drawn out inside of it."*
// The geometry is `tests/note_preview.rs`; this is the half only a real frame
// can answer — that the notes are on screen, in ink you can tell from the
// block they are drawn on.

fn a_note_clip(loop_length: Option<Tick>) -> fontelle_ui::document::ClipInfo {
    let mut arena: Arena<fontelle_types::ClipId, ()> = Arena::default();
    let n = |start: Tick, length: Tick, key: u8| fontelle_ui::document::NotePreview {
        start,
        length,
        key,
    };
    fontelle_ui::document::ClipInfo {
        id: arena.insert(()),
        lane: 1,
        start: 0,
        length: PPQN * 16,
        name: "Keys".to_string(),
        muted: false,
        open: false,
        color: [0x4f, 0x8f, 0xd0, 0xff],
        loop_length,
        kind: fontelle_ui::document::ClipKind::Notes,
        curve: Vec::new(),
        notes: vec![
            n(0, PPQN, 60),
            n(PPQN, PPQN, 64),
            n(PPQN * 2, PPQN, 67),
            n(PPQN * 3, PPQN, 72),
        ],
        audio: Default::default(),
        prefab: None,
    }
}

#[test]
fn a_note_clip_draws_its_notes_and_they_can_be_told_from_the_block() {
    use fontelle_ui::canvas::{clip_bands, clip_notes, clip_rect};

    let clips = vec![a_note_clip(None)];
    let Some(shot) = shoot_timeline(&clips) else {
        return;
    };
    let block = clip_rect(&shot.view, shot.layout.grid, &clips[0]);
    let (_, content) = clip_bands(block);
    let rects = clip_notes(block, shot.layout.grid, &clips[0]);
    assert_eq!(rects.len(), 4, "four notes to draw");

    // A pixel in the middle of the first note, and one in the content band
    // that no note covers. They have to be different colours, or the preview
    // is a block with an invisible pattern on it.
    let inside = {
        // The second note, not the first: the first starts on the block's
        // left edge, and every block has a dark edge line now — *"make it so
        // that the edges of clips are always visible"* — which is the one
        // place a note's ink is not what is on screen.
        let r = rects[1];
        shot.at((r.x + r.width / 2.0) as u32, (r.y + r.height / 2.0) as u32)
    };
    let empty = {
        // The far right of the band: the notes are in the first bar of four.
        let x = content.right() as u32 - 4;
        shot.at(x, (content.y + content.height / 2.0) as u32)
    };
    assert!(
        !near(inside, empty),
        "a note pixel {inside:?} is the same colour as the block {empty:?}"
    );
    // And the note is lighter than the block it is on, which is the rule the
    // ink follows — a preview reads as part of its clip, not as something
    // lying on top of it.
    let brightness = |c: Color| u32::from(c.0[0]) + u32::from(c.0[1]) + u32::from(c.0[2]);
    assert!(
        brightness(inside) > brightness(empty),
        "the notes are darker than their block: {inside:?} on {empty:?}"
    );
}

#[test]
fn a_looped_clip_draws_a_pass_in_every_bar_it_covers() {
    // Four bars of a one-bar pattern is four passes on screen, not one — the
    // report this whole feature answers is about a clip that looked empty.
    use fontelle_ui::canvas::{clip_notes, clip_rect};

    let clips = vec![a_note_clip(Some(PPQN * 4))];
    let Some(shot) = shoot_timeline(&clips) else {
        return;
    };
    let block = clip_rect(&shot.view, shot.layout.grid, &clips[0]);
    let rects = clip_notes(block, shot.layout.grid, &clips[0]);
    assert_eq!(rects.len(), 16, "four notes, four passes");

    let brightness = |c: Color| u32::from(c.0[0]) + u32::from(c.0[1]) + u32::from(c.0[2]);
    let ground = {
        let r = rects[0];
        brightness(shot.at((r.x + r.width / 2.0) as u32, (r.bottom() + 2.0) as u32))
    };
    // The **last** pass, which is the one a preview that drew the pattern
    // once would have left blank.
    let last = rects[rects.len() - 1];
    let ink = brightness(shot.at(
        (last.x + last.width / 2.0) as u32,
        (last.y + last.height / 2.0) as u32,
    ));
    assert!(
        ink > ground,
        "the last pass is not drawn: {ink} against a ground of {ground}"
    );
}

// ------------------------------------------ the song/clip chip is drawn ---
//
// *"there should also be a way to swap between clip and song mode currently
// its always on song."* The chip is on the transport bar, and the one thing a
// geometry test cannot say is whether it is **on screen** and whether it
// looks different in the two modes — a switch that does not visibly switch is
// a switch nobody trusts.

#[test]
fn the_mode_chip_is_drawn_and_says_which_mode_it_is_in() {
    let theme = Theme::dark_default();
    // A width the window actually opens at: at 640 the bar gives the chip up
    // to keep a usable ruler, which is the rule
    // `tests/transport.rs::a_narrow_bar_drops_the_mode_chip...` holds.
    const WIDE: u32 = 1280;
    let (Some(song), Some(clip)) = (
        shoot_sized(theme.clone(), live_view(), [Meter::new(); 2], false, WIDE),
        shoot_sized(theme.clone(), live_view(), [Meter::new(); 2], true, WIDE),
    ) else {
        return;
    };

    let chip = song.bar.mode;
    assert!(
        !chip.is_empty(),
        "there is no chip to draw at {WIDE} across"
    );

    // In clip mode the chip is filled with the accent, because a transport
    // playing one part of a song rather than the song is a state worth
    // noticing across the room.
    let mut accent = 0;
    let mut differ = 0;
    for x in (chip.x as u32 + 3)..(chip.right() as u32 - 3) {
        for y in (chip.y as u32 + 3)..(chip.bottom() as u32 - 3) {
            if near(clip.at(x, y), theme.palette.accent) {
                accent += 1;
            }
            if !near(clip.at(x, y), song.at(x, y)) {
                differ += 1;
            }
        }
    }
    assert!(
        accent > 20,
        "clip mode does not light the chip: {accent} accent pixels"
    );
    assert!(differ > 20, "the two modes draw the chip identically");

    // And nothing outside the chip moved: the mode is not allowed to repaint
    // the rest of the bar.
    let elsewhere = song.bar.readout;
    for x in (elsewhere.x as u32 + 2)..(elsewhere.right() as u32 - 2) {
        let y = (elsewhere.y + elsewhere.height / 2.0) as u32;
        assert!(
            near(clip.at(x, y), song.at(x, y)),
            "the read-out changed with the mode at x={x}"
        );
    }
}

// ---------------------------------------------------------- Tools panel ---

/// The Tools panel opens over the grid, and its rows carry ink.
///
/// The failure this exists to catch is the one a pure layout test cannot: a
/// panel that is laid out correctly and never drawn, or drawn with captions
/// nobody shaped, which is an empty rectangle. Both have happened in this
/// window — *"a menu that was never drawn, a tab that was never drawn"* — and
/// neither had a failing test.
#[test]
fn the_tools_panel_is_actually_painted_over_the_grid() {
    let Some(closed) = shoot_roll(&Arena::default(), &[]) else {
        return;
    };
    let Some(open) = shoot_roll_full(
        &Arena::default(),
        &[],
        &[],
        None,
        &fontelle_ui::document::KeyMap::unknown(),
        SnapDivision::Step,
        0,
        true,
    ) else {
        return;
    };

    let theme = Theme::dark_default();
    let chip = toolbar_layout(open.layout.toolbar, &theme.metrics)
        .items
        .iter()
        .find(|(control, _)| *control == fontelle_ui::canvas::RollControl::Tools)
        .map(|(_, rect)| *rect)
        .expect("the toolbar has a tools chip");
    let panel = fontelle_ui::canvas::tools_dialog_layout(
        fontelle_ui::canvas::ToolKind::Adjust,
        chip,
        open.layout.frame,
        &theme.metrics,
    );
    assert!(!panel.frame.is_empty(), "the panel had nowhere to go");

    // Somewhere inside the panel that is over the grid: the two frames must
    // differ, or the panel is not being drawn at all.
    let mut differences = 0;
    for (_, row) in &panel.rows {
        if row.is_empty() {
            continue;
        }
        let y = (row.y + row.height / 2.0) as u32;
        for step in 0..12 {
            let x = (row.x + 2.0 + step as f32 * (row.width / 14.0)) as u32;
            if x >= RW || y >= RH {
                continue;
            }
            if !near(open.at(x, y), closed.at(x, y)) {
                differences += 1;
            }
        }
    }
    assert!(
        differences > 20,
        "the panel changed only {differences} sampled pixels \u{2014} it is not being drawn"
    );
}

/// The captions on it are ink, not just a coloured slab.
#[test]
fn the_tools_panels_rows_have_words_on_them() {
    let Some(open) = shoot_roll_full(
        &Arena::default(),
        &[],
        &[],
        None,
        &fontelle_ui::document::KeyMap::unknown(),
        SnapDivision::Step,
        0,
        true,
    ) else {
        return;
    };
    let theme = Theme::dark_default();
    let chip = toolbar_layout(open.layout.toolbar, &theme.metrics)
        .items
        .iter()
        .find(|(control, _)| *control == fontelle_ui::canvas::RollControl::Tools)
        .map(|(_, rect)| *rect)
        .expect("the toolbar has a tools chip");
    let panel = fontelle_ui::canvas::tools_dialog_layout(
        fontelle_ui::canvas::ToolKind::Adjust,
        chip,
        open.layout.frame,
        &theme.metrics,
    );

    // A row's own background against the darkest pixel on it: text is drawn in
    // `text` over `panel_header`, so a row with a caption has a pixel well
    // away from its ground and a row without one is flat.
    let mut rows_with_ink = 0;
    for (_, row) in &panel.rows {
        if row.is_empty() {
            continue;
        }
        let y0 = row.y as u32;
        let mut seen: Vec<Color> = Vec::new();
        for dy in 0..(row.height as u32).max(1) {
            for dx in 0..(row.width as u32).max(1) {
                let (x, y) = (row.x as u32 + dx, y0 + dy);
                if x < RW && y < RH {
                    seen.push(open.at(x, y));
                }
            }
        }
        if seen.iter().any(|c| !near(*c, seen[0])) {
            rows_with_ink += 1;
        }
    }
    // **Every** row, now that a dialog holds one tool's worth of them: a
    // blank row in a list of four is half the dialog.
    let drawn = panel.rows.iter().filter(|(_, r)| !r.is_empty()).count();
    assert!(drawn > 0, "the dialog had nowhere to go");
    assert_eq!(
        rows_with_ink, drawn,
        "only {rows_with_ink} of {drawn} rows have anything on them"
    );
}

// ------------------------------------------ edges, overlaps and fades ---

/// An audio clip on `lane` from `start` for `length`, with a flat waveform
/// and the fades given as fractions of the block.
fn an_audio_clip(
    lane: usize,
    start: Tick,
    length: Tick,
    fade_in: f32,
    fade_out: f32,
) -> fontelle_ui::document::ClipInfo {
    let mut clip = a_clip(lane, start, length, [0x4f, 0x8f, 0xd0, 0xff]);
    clip.kind = fontelle_ui::document::ClipKind::Audio;
    clip.audio = fontelle_ui::document::AudioPreview {
        peaks: vec![(-0.6, 0.6); 128].into(),
        rms: vec![0.4; 128].into(),
        fade_in,
        fade_out,
        fade_in_tension: 0.5,
        fade_out_tension: 0.0,
        seconds: 4.0,
        // The file exactly fills its block, which is what a drop makes and
        // so what these shots should be of.
        natural_length: length,
        stretched: false,
        loop_offset: 0,
    };
    clip
}

/// **A block under the pointer shows its fade handles, and lights the one
/// the pointer is on** — though it is not selected.
///
/// > *"the clip fades also feels a little janky please make it have really
/// > polished ux like fl studios clip fades"*
///
/// The handles used to appear only on the selected block, so a corner of
/// an unselected take looked like the rest of the caption: the first thing
/// to learn about fades was that the corner did anything at all.
#[test]
fn a_block_under_the_pointer_shows_its_fade_handles_with_the_one_under_it_lit() {
    use fontelle_ui::canvas::{ClipPart, FadeEnd, clip_rect, fade_anatomy};
    let clip = an_audio_clip(0, 0, PPQN * 8, 0.0, 0.0);
    let clips = vec![clip.clone()];
    let Some(plain) = shoot_timeline_hovered(&clips, None) else {
        return;
    };
    let block = clip_rect(&plain.view, plain.layout.grid, &clip);
    let anatomy = fade_anatomy(block, &clip).expect("an audio block has fade handles");
    let probe = |handle: fontelle_ui::layout::Rect| {
        (
            (handle.x + handle.width / 2.0) as u32,
            (handle.y + handle.height / 2.0) as u32,
        )
    };
    let (ix, iy) = probe(anatomy.handle_in);
    let (ox, oy) = probe(anatomy.handle_out);
    let body = Color(clip.color);
    assert!(
        near(plain.at(ix, iy), body),
        "with no pointer on it the corner is the block: {:?}",
        plain.at(ix, iy)
    );

    // The pointer on the body: both handles drawn, neither lit.
    let over_body = shoot_timeline_hovered(&clips, Some((clip.id, ClipPart::Body))).unwrap();
    assert!(
        !near(over_body.at(ix, iy), body) && !near(over_body.at(ox, oy), body),
        "the pointer on the block did not bring its handles up"
    );
    let accent = plain.theme.palette.accent;
    assert!(
        !near(over_body.at(ix, iy), accent),
        "a handle the pointer is not on is lit"
    );

    // The pointer on the in handle: that one lit, the other not.
    let over_handle =
        shoot_timeline_hovered(&clips, Some((clip.id, ClipPart::FadeHandle(FadeEnd::In)))).unwrap();
    assert!(
        near(over_handle.at(ix, iy), accent),
        "the handle under the pointer is not lit: {:?} against {:?}",
        over_handle.at(ix, iy),
        accent
    );
    assert!(
        !near(over_handle.at(ox, oy), accent),
        "the other handle lit too"
    );
}

#[test]
fn two_blocks_end_to_end_are_parted_by_an_edge_and_an_overlap_is_striped() {
    use fontelle_ui::canvas::{clip_bands, clip_overlaps, clip_rect};

    // *"make it so that the edges of clips are always visible and dont blend
    // into eachother when they get close or even overlapped, and when theyre
    // overlapped, there should be a kind of diagonal stripe pattern on the
    // overlapping part."* Two blocks of one colour end to end on the first
    // row, two overlapping on the second, and a fade on the third.
    let mut clips = vec![
        an_audio_clip(0, 0, PPQN * 8, 0.0, 0.0),
        an_audio_clip(0, PPQN * 8, PPQN * 8, 0.0, 0.0),
        an_audio_clip(1, 0, PPQN * 10, 0.0, 0.0),
        an_audio_clip(1, PPQN * 6, PPQN * 10, 0.0, 0.0),
        an_audio_clip(2, 0, PPQN * 12, 0.4, 0.3),
    ];
    // Distinct ids, out of one arena: `a_clip` mints each from a fresh one,
    // and five clips sharing an id are five clips the selection matches.
    let mut ids: Arena<fontelle_types::ClipId, ()> = Arena::default();
    for clip in &mut clips {
        clip.id = ids.insert(());
    }
    let Some(shot) = shoot_timeline_selected(&clips, &[clips[4].id]) else {
        return;
    };
    dump_sized(&shot.pixels, "arrangement overlaps and fades", RW, RH);
    let brightness = |c: Color| u32::from(c.0[0]) + u32::from(c.0[1]) + u32::from(c.0[2]);

    // The seam: the pixel on the boundary is darker than the bodies either
    // side of it, so two blocks of one colour read as two blocks.
    let first = clip_rect(&shot.view, shot.layout.grid, &clips[0]);
    let (_, content) = clip_bands(first);
    let y = (content.y + content.height * 0.9) as u32; // under the waveform's reach
    let seam = shot.at(first.right() as u32, y);
    let body_left = shot.at((first.right() - 6.0) as u32, y);
    let body_right = shot.at((first.right() + 6.0) as u32, y);
    assert!(
        brightness(seam) < brightness(body_left) && brightness(seam) < brightness(body_right),
        "no edge at the seam: {seam:?} between {body_left:?} and {body_right:?}"
    );

    // The overlap: striped, so a row of pixels across it is not one colour.
    let shared = clip_overlaps(&shot.view, shot.layout.grid, &clips);
    assert_eq!(shared.len(), 1);
    let band = shared[0].area;
    let (_, content) = clip_bands(band);
    let y = (content.y + content.height * 0.9) as u32;
    let mut seen: Vec<Color> = Vec::new();
    for x in (band.x as u32 + 2)..(band.right() as u32 - 2) {
        let c = shot.at(x, y);
        if !seen.iter().any(|s| near(*s, c)) {
            seen.push(c);
        }
    }
    assert!(seen.len() >= 2, "the overlap is one flat colour: {seen:?}");

    // The fade: the shaded region above the curve is darker than the same
    // height of the block where there is no fade.
    let third = clip_rect(&shot.view, shot.layout.grid, &clips[4]);
    let (_, content) = clip_bands(third);
    let y = (content.y + 2.0) as u32;
    let faded = shot.at((third.x + third.width * 0.05) as u32, y);
    let plain = shot.at((third.x + third.width * 0.55) as u32, y);
    assert!(
        brightness(faded) < brightness(plain),
        "the fade is not shaded: {faded:?} against {plain:?}"
    );
}

#[test]
fn the_crossfade_is_drawn_across_the_striped_overlap() {
    use fontelle_ui::canvas::{clip_bands, clip_overlaps, clip_rect};

    // *"i do want it to also show the graph line drawn to show the fade on
    // the overlap as well ... except we will render ours on top of the
    // diagonal striped background on the overlayed section."* The two curves
    // are stroked in the text ink, at full strength, over stripes that are
    // the same ink at a third of it — so the line is the brightest thing in
    // the overlap and there is nothing that bright anywhere else on a block.
    //
    // No waveform on these two, deliberately: a peak drawn seven tenths of
    // the way to white is within a few steps of the curve's own ink, and a
    // test that could not tell them apart would pass on the waveform alone.
    let mut clips = vec![
        an_audio_clip(0, 0, PPQN * 8, 0.0, 0.0),
        an_audio_clip(0, PPQN * 6, PPQN * 8, 0.0, 0.0),
    ];
    let mut ids: Arena<fontelle_types::ClipId, ()> = Arena::default();
    for clip in &mut clips {
        clip.id = ids.insert(());
        clip.audio.peaks = Vec::new().into();
    }
    let Some(shot) = shoot_timeline(&clips) else {
        return;
    };
    dump_sized(&shot.pixels, "arrangement crossfade", RW, RH);

    let ink = shot.theme.palette.text;
    let close = |c: Color| {
        c.0.iter()
            .zip(ink.0.iter())
            .take(3)
            .all(|(x, y)| x.abs_diff(*y) <= 20)
    };
    let (_, content) = clip_bands(clip_rect(&shot.view, shot.layout.grid, &clips[0]));
    let band = clip_overlaps(&shot.view, shot.layout.grid, &clips)[0].area;
    let lit =
        |x: u32| ((content.y as u32 + 1)..content.bottom() as u32).any(|y| close(shot.at(x, y)));

    let middle = ((band.x + band.right()) / 2.0) as u32;
    assert!(lit(middle), "no curve where the two cross");
    // At each end of the overlap too, where one curve is at the top of the
    // band and the other at its foot.
    assert!(
        lit(band.x as u32 + 3),
        "no curve at the start of the overlap"
    );
    assert!(
        lit(band.right() as u32 - 3),
        "no curve at the end of the overlap"
    );
    // And nowhere else on the block: the stripes are a third of this ink,
    // and a plain body is nothing like it.
    assert!(
        !lit(band.x as u32 - 20),
        "something as bright as the curve is drawn outside the overlap"
    );
}

// -------------------------------------------- the stretch switch, drawn ---

/// An audio clip whose file takes `natural` ticks, on a block of `length`.
fn a_take(
    lane: usize,
    length: Tick,
    natural: Tick,
    stretched: bool,
    loop_length: Option<Tick>,
) -> fontelle_ui::document::ClipInfo {
    let mut clip = a_clip(lane, 0, length, [0x4f, 0x8f, 0xd0, 0xff]);
    clip.kind = fontelle_ui::document::ClipKind::Audio;
    clip.loop_length = loop_length;
    // A ramp, so *which* part of the file a column came from is readable off
    // the picture rather than inferred.
    clip.audio = fontelle_ui::document::AudioPreview {
        peaks: (0..128)
            .map(|i| {
                let v = 0.08 + 0.9 * (i as f32 / 128.0);
                (-v, v)
            })
            .collect(),
        natural_length: natural,
        stretched,
        ..Default::default()
    };
    clip
}

#[test]
fn a_take_is_drawn_where_it_sounds_and_the_switch_says_which_way_it_is_set() {
    // The report: *"i cannot loop clips, whenever i drag them it is ALWAYS
    // stretching them."* The picture was the half of it that lied — the
    // waveform filled whatever block it was given, so a clip dragged longer
    // *looked* stretched whatever it did. Four rows, each a different answer
    // to "what is this block playing":
    //
    //   1. not stretched, block twice the file  -> the ramp ends halfway
    //   2. stretched, same block                -> the ramp fills it
    //   3. not stretched, looping every bar     -> the ramp again per pass
    //   4. stretched, looping every bar         -> a full ramp per pass
    let clips = vec![
        a_take(0, PPQN * 8, PPQN * 4, false, None),
        a_take(1, PPQN * 8, PPQN * 4, true, None),
        a_take(2, PPQN * 8, PPQN * 2, false, Some(PPQN * 2)),
        a_take(3, PPQN * 8, PPQN, true, Some(PPQN * 2)),
    ];
    let Some(shot) = shoot_timeline_switch(&clips, &[], true) else {
        return;
    };
    dump_sized(&shot.pixels, "timeline-stretch-on", RW, RH);

    let Some(off) = shoot_timeline_switch(&clips, &[], false) else {
        return;
    };
    dump_sized(&off.pixels, "timeline-stretch-off", RW, RH);

    // The switch is lit one way and not the other, which is the whole of what
    // a switch has to do. Read off the pixels of its own chip rather than
    // eyeballed: a control nobody can tell the state of is the bug this is for.
    let bar =
        fontelle_ui::canvas::timeline_toolbar_layout(shot.layout.toolbar, &shot.theme.metrics);
    let chip = bar
        .items
        .iter()
        .find(|(c, _)| *c == fontelle_ui::canvas::TimelineControl::Stretch)
        .map(|(_, r)| *r)
        .expect("the switch is on the toolbar");
    let at = |pixels: &[u8], x: f32, y: f32| {
        let i = ((y as usize) * RW as usize + x as usize) * 4;
        [pixels[i], pixels[i + 1], pixels[i + 2]]
    };
    let (x, y) = (chip.x + chip.width / 2.0, chip.y + chip.height / 2.0);
    assert_ne!(
        at(&shot.pixels, x, y),
        at(&off.pixels, x, y),
        "the stretch chip looks the same on as off"
    );
}

/// A clip does **not** grow to hold a note drawn past its end — asked for in
/// those words — so the roll has to say where the end is, or the note is
/// silent with nothing to show for it. The grid past the end carries the
/// same "nothing here sounds" ink the dead rows of a drum kit use.
#[test]
fn the_grid_past_the_clips_end_is_shaded() {
    let end = PPQN * 2;
    let Some(shot) = shoot_roll_ending(end) else {
        return;
    };
    dump_sized(&shot.pixels, "roll-clip-end", RW, RH);

    let grid = shot.layout.grid;
    // A row that plays and is not an accidental, so what is under test is the
    // shade and not the striping: C, and a whole key height inside the grid.
    let y =
        (fontelle_ui::canvas::key_to_y(&shot.view, grid, 60) + shot.view.key_height / 2.0) as u32;

    // **Off the grid lines**, five pixels along from a snap boundary: the
    // lines are drawn over the shade and land exactly on these ticks, so a
    // sample taken on one reads `grid_line_sub` whatever the shade did — as
    // the first draft of this test did, on both sides.
    let inside = tick_to_x(&shot.view, grid, end - PPQN / 2) as u32 + 5;
    let outside = tick_to_x(&shot.view, grid, end + PPQN / 2) as u32 + 5;
    assert!(
        (outside as f32) < grid.right(),
        "the test needs both sides on screen: {outside} vs {}",
        grid.right()
    );
    assert!(
        near(shot.at(outside, y), shot.theme.palette.row_dead),
        "past the end should be shaded, found {:?}",
        shot.at(outside, y)
    );
    assert!(
        !near(shot.at(inside, y), shot.theme.palette.row_dead),
        "inside the clip should not be"
    );
}

/// And a roll with no clip end shades nothing — every test fake, and any
/// host that has no clip open.
#[test]
fn a_roll_with_no_clip_end_shades_nothing() {
    let notes = Arena::default();
    let Some(shot) = shoot_roll(&notes, &[]) else {
        return;
    };
    let grid = shot.layout.grid;
    let y =
        (fontelle_ui::canvas::key_to_y(&shot.view, grid, 60) + shot.view.key_height / 2.0) as u32;
    let x = (grid.right() - 4.0) as u32;
    assert!(!near(shot.at(x, y), shot.theme.palette.row_dead));
}

// ------------------------------------------------ Flopsynth's own window ---

/// Renders Flopsynth's window: the cards, a picture, the tab strip, the
/// modulation ring and the preset bar (`docs/flopsynth-plan.md` §8.9).
///
/// The one test that runs `draw_flopsynth` at all. Everything it draws comes
/// from `canvas/flopsynth.rs`, which is pure and tested, so what is left here
/// is **colour** — and colour is the one thing a geometry test cannot see.
fn shoot_flopsynth() -> Option<(Vec<u8>, Theme, fontelle_ui::canvas::FlopsynthLayout, u32)> {
    shoot_flopsynth_in(Theme::dark_default(), "flopsynth")
}

fn shoot_flopsynth_sky(
    sky: Option<fontelle_ui::render::SkyFrame>,
) -> Option<(Vec<u8>, Theme, fontelle_ui::canvas::FlopsynthLayout, u32)> {
    shoot_flopsynth_full(Theme::dark_default(), "flopsynth-sky", sky)
}

/// [`shoot_flopsynth`] in a theme of the caller's choosing. The theme handed
/// back is the one the **window was painted in**, which for this window is
/// its own (`Theme::for_bridge`) whatever the studio's.
fn shoot_flopsynth_in(
    theme: Theme,
    name: &str,
) -> Option<(Vec<u8>, Theme, fontelle_ui::canvas::FlopsynthLayout, u32)> {
    shoot_flopsynth_full(theme, name, None)
}

fn shoot_flopsynth_full(
    theme: Theme,
    name: &str,
    sky: Option<fontelle_ui::render::SkyFrame>,
) -> Option<(Vec<u8>, Theme, fontelle_ui::canvas::FlopsynthLayout, u32)> {
    shoot_flopsynth_view(theme, name, sky, |_| {})
}

/// [`shoot_flopsynth_full`] with the view changed before it is laid out —
/// a page, a strip's sources, an open inspector.
fn shoot_flopsynth_view(
    theme: Theme,
    name: &str,
    sky: Option<fontelle_ui::render::SkyFrame>,
    tweak: impl FnOnce(&mut fontelle_ui::canvas::FlopsynthView),
) -> Option<(Vec<u8>, Theme, fontelle_ui::canvas::FlopsynthLayout, u32)> {
    use fontelle_types::ParamAddress;
    use fontelle_ui::canvas::{
        FlopsynthCard, FlopsynthPicture, FlopsynthView, InstrumentGroup, InstrumentParam,
        ParamKind, flopsynth_layout,
    };
    use fontelle_ui::render::FlopsynthChrome;

    let shared = headless()?;
    let mut text = TextContext::new();
    let title = text.layout("Flopsynth \u{2014} Choir Ahh", &theme.font, None);

    let knob = |name: &str, value: f32| InstrumentParam {
        address: ParamAddress::new(format!("patch/{name}")),
        label: name.to_string(),
        value,
        display: format!("{value:.2}"),
        kind: ParamKind::Knob,
        automated: false,
    };
    let view = FlopsynthView {
        title: "Choir Ahh".to_string(),
        cards: vec![
            FlopsynthCard {
                oscillator: None,
                row: 0,
                aside: false,
                columns: 0,
                removable: false,
                sizes: Vec::new(),
                group: InstrumentGroup {
                    name: "OSC A".to_string(),
                    params: vec![knob("pos", 0.3), knob("level", 0.7)],
                },
                picture: FlopsynthPicture::Wave {
                    points: (0..128)
                        .map(|i| (i as f32 / 128.0 * std::f32::consts::TAU).sin())
                        .collect(),
                    position: 0.3,
                },
            },
            FlopsynthCard {
                oscillator: None,
                row: 1,
                aside: false,
                columns: 0,
                removable: false,
                sizes: Vec::new(),
                group: InstrumentGroup {
                    name: "Filter 1".to_string(),
                    params: vec![knob("cutoff", 0.6), knob("res", 0.2)],
                },
                picture: FlopsynthPicture::Response {
                    points: (0..96).map(|i| 6.0 - i as f32 * 0.4).collect(),
                    cutoff: 0.5,
                    resonance: 0.3,
                },
            },
        ],
        page: fontelle_ui::canvas::FlopsynthPage::Synth,
        sources: Vec::new(),
        routes: Vec::new(),
        voices: 3,
        ..FlopsynthView::default()
    };
    let mut view = view;
    tweak(&mut view);

    let (ew, eh) = fontelle_ui::layout::FLOPSYNTH_SIZE;
    let panel = fontelle_ui::layout::editor_window_layout(ew as f32, eh as f32, &theme.metrics);
    let l = flopsynth_layout(panel.body, &theme.metrics, &view);

    let mut labels = Labels::new();
    for caption in [
        fontelle_ui::render::NO_ROUTES,
        fontelle_ui::render::SAVE,
        fontelle_ui::render::SAVE_AS,
        fontelle_ui::canvas::NO_PRESET,
    ] {
        labels.ensure(caption, &theme.font, &mut text);
    }
    // The bridge's three styles, the way the window shapes them
    // (`render::bridge_type`).
    let t = fontelle_ui::render::bridge_type(view.scale);
    for page in fontelle_ui::canvas::FlopsynthPage::ALL {
        labels.ensure_styled(page.label(), &theme.font, t.heading, &mut text);
    }
    labels.ensure_styled(
        &fontelle_ui::render::voice_count_label(view.voices),
        &theme.font,
        t.value,
        &mut text,
    );
    labels.ensure_styled(
        &fontelle_ui::render::scale_label(view.scale),
        &theme.font,
        t.value,
        &mut text,
    );
    for card in &view.cards {
        labels.ensure_styled(&card.group.name, &theme.font, t.heading, &mut text);
        for param in &card.group.params {
            labels.ensure_styled(&param.label, &theme.font, t.caption, &mut text);
            labels.ensure_styled(&param.display, &theme.font, t.value, &mut text);
        }
    }
    // The strip's badges wear their names as captions, and the drawer's
    // header is the inspected source's name as a heading.
    for source in &view.sources {
        labels.ensure_styled(source, &theme.font, t.caption, &mut text);
        labels.ensure_styled(source, &theme.font, t.heading, &mut text);
    }

    // A route on the filter's cutoff, so the violet arc is in the shot.
    let bar = fontelle_ui::canvas::PresetBarView {
        name: Some("Choir Ahh".to_string()),
        category: "Choir & Vocal".to_string(),
        origin: Some(fontelle_types::PresetOrigin::Factory),
        dirty: true,
        favourite: true,
        can_save: false,
    };
    labels.ensure(
        &fontelle_ui::canvas::preset_bar_name(&bar),
        &theme.font,
        &mut text,
    );
    labels.ensure(&bar.category, &theme.font, &mut text);
    let preset = fontelle_ui::render::PresetBarChrome {
        layout: fontelle_ui::canvas::preset_bar_layout(
            panel.header,
            title.width + 24.0,
            &bar,
            &theme.metrics,
        ),
        view: &bar,
        hover: None,
    };

    let mut scene = vello::Scene::new();
    fontelle_ui::render::draw_editor_window(
        &mut scene,
        &theme,
        &panel,
        &labels,
        &title,
        &fontelle_ui::render::EditorWindowChrome::Flopsynth(FlopsynthChrome {
            layout: l.clone(),
            view: &view,
            hover: None,
            active: None,
            modulated: vec![(
                (1, 0),
                vec![fontelle_ui::document::ModRing {
                    family: fontelle_ui::document::SourceFamily::Envelope,
                    depth: 0.6,
                    source: 0,
                }],
            )],
            source_values: Vec::new(),
            assigning: None,
            carrying_slot: None,
            carrying_route: None,
            destinations: vec![(1, 0), (1, 1)],
            about: Vec::new(),
            loaded: None,
            thumbnail: None,
            hover_at: (f32::MIN, f32::MIN),
            tooltip: None,
            searching: false,
            sky: sky.as_ref(),
            skin: None,
            arc_values: Vec::new(),
            page_alpha: 1.0,
            bubble_alpha: 1.0,
        }),
        Some(&preset),
        None,
        None,
        None,
    );
    let pixels = shared
        .lock()
        .expect("the shared renderer")
        .render(&scene, ew, eh, theme.palette.window)
        .expect("the scene must render");
    dump_sized(&pixels, name, ew, eh);
    Some((pixels, theme.for_bridge(), l, ew))
}

/// §3.4: the strip under every page wears a badge per source in its family's
/// ink, with the source's own picture on it, and the inspector is an opaque
/// drawer over the page with the inspected source's name and a ✕ — read
/// back by pixel, the way the canopy is.
#[test]
fn the_strip_wears_a_badge_per_source_and_the_inspector_is_a_drawer() {
    use fontelle_types::ParamAddress;
    use fontelle_ui::canvas::{
        FlopsynthCard, FlopsynthPicture, INSPECTOR_ROW, InstrumentGroup, InstrumentParam, ParamKind,
    };
    use fontelle_ui::document::SourceFamily;
    let Some((pixels, theme, l, width)) =
        shoot_flopsynth_view(Theme::dark_default(), "flopsynth-strip", None, |view| {
            view.sources = vec!["ENV 1".into(), "LFO 1".into(), "Macro 1".into()];
            view.source_families = vec![
                SourceFamily::Envelope,
                SourceFamily::Lfo,
                SourceFamily::Macro,
            ];
            let cycle: Vec<f32> = (0..64)
                .map(|i| (i as f32 / 64.0 * std::f32::consts::TAU).sin())
                .collect();
            let rise: Vec<f32> = (0..64).map(|i| (i as f32 / 20.0).min(1.0)).collect();
            view.source_shapes = vec![rise, cycle, vec![0.4]];
            view.inspector = Some(1);
            view.cards.push(FlopsynthCard {
                oscillator: None,
                row: INSPECTOR_ROW,
                aside: false,
                columns: 8,
                removable: false,
                sizes: vec![fontelle_ui::canvas::KnobSize::Large],
                group: InstrumentGroup {
                    name: "LFO 1".to_string(),
                    params: vec![InstrumentParam {
                        address: ParamAddress::new("patch/lfo[0]/rate"),
                        label: "RATE".to_string(),
                        value: 0.4,
                        display: "2.0 Hz".to_string(),
                        kind: ParamKind::Knob,
                        automated: false,
                    }],
                },
                picture: FlopsynthPicture::Lfo {
                    points: vec![0.0; 64],
                    phase: 0.0,
                },
            });
        })
    else {
        return;
    };
    let at = |x: u32, y: u32| {
        let i = ((y * width + x) * 4) as usize;
        Color::rgb(pixels[i], pixels[i + 1], pixels[i + 2])
    };
    let p = &theme.palette;
    let inks = [
        p.mod_envelope,
        p.mod_lfo,
        p.mod_macro,
        p.mod_note,
        p.mod_performance,
    ];
    let distance = |a: Color, b: Color| -> u32 {
        a.0.iter()
            .zip(b.0.iter())
            .take(3)
            .map(|(x, y)| x.abs_diff(*y) as u32)
            .sum()
    };
    // A hairline is anti-aliased, so "wears the ink" is a pixel nearer this
    // family's ink than any other's, and near it at all.
    let has_ink = |rect: fontelle_ui::layout::Rect, ink: Color| {
        (rect.y as u32..rect.bottom() as u32).any(|y| {
            (rect.x as u32..rect.right() as u32).any(|x| {
                let c = at(x, y);
                let d = distance(c, ink);
                d <= 60
                    && inks
                        .iter()
                        .all(|other| *other == ink || distance(c, *other) > d)
            })
        })
    };
    assert!(!l.strip.is_empty() && l.badges.len() == 3);
    assert!(
        has_ink(l.badges[0], p.mod_envelope),
        "the envelope's badge wears no envelope ink"
    );
    assert!(
        has_ink(l.badges[1], p.mod_lfo),
        "the LFO's badge wears no LFO ink"
    );
    assert!(
        has_ink(l.badges[2], p.mod_macro),
        "the macro's badge wears no macro ink"
    );
    assert!(
        !has_ink(l.badges[0], p.mod_lfo),
        "the envelope's badge wears the LFO's ink"
    );
    // The drawer: opaque over the page — its ground is the panel's flat
    // ink, not the graded sky — with its card on it and the ✕ drawn over
    // the card's header.
    let drawer = l.inspector;
    assert!(!drawer.is_empty());
    let ground = at(drawer.x as u32 + 3, drawer.y as u32 + 3);
    assert!(
        near(ground, p.panel),
        "the drawer's ground is not a flat panel: {ground:?}"
    );
    assert!(
        contrast_in(&pixels, width, l.inspector_close) > 1.8,
        "no ✕ in the drawer's corner"
    );
    let card = l.cards.last().expect("the inspected card");
    assert!(
        card.header
            .contains(l.inspector_close.x + 1.0, l.inspector_close.y + 1.0),
        "the ✕ is in the card's header"
    );
    let heading =
        fontelle_ui::layout::Rect::new(card.header.x, card.header.y, 80.0, card.header.height);
    assert!(
        contrast_in(&pixels, width, heading) > 2.0,
        "the card's header names nothing"
    );
}

/// WCAG relative luminance of a pixel, and the contrast ratio between the
/// brightest and darkest pixels in a rectangle — which for a rectangle
/// holding a label on a ground is the label against its ground, whatever
/// the anti-aliasing between.
fn luminance(c: Color) -> f32 {
    let channel = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(c.0[0]) + 0.7152 * channel(c.0[1]) + 0.0722 * channel(c.0[2])
}

fn contrast_in(pixels: &[u8], width: u32, rect: fontelle_ui::layout::Rect) -> f32 {
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for y in rect.y as u32..rect.bottom() as u32 {
        for x in rect.x as u32..rect.right() as u32 {
            let i = ((y * width + x) * 4) as usize;
            let l = luminance(Color::rgb(pixels[i], pixels[i + 1], pixels[i + 2]));
            lo = lo.min(l);
            hi = hi.max(l);
        }
    }
    (hi + 0.05) / (lo + 0.05)
}

/// §3.2, Ty's decision §9.1: the canopy is the instrument's eyes. Shot with
/// a synthetic analyser frame — a sine in the scope, one loud band in the
/// spectrum, three of eight voices sounding — and read back by pixel: the
/// scope has ink where the wave is and none on its empty line, the loud
/// band's column has ink and a silent one none, and the lamps are lit
/// where the voices are.
#[test]
fn the_canopy_draws_the_scope_the_spectrum_and_the_lamps() {
    use fontelle_ui::canvas::{canopy_eyes, lamp_dots, spectrum_bars};
    let Some((pixels, _theme, l, width)) = shoot_flopsynth_with_sky() else {
        return;
    };
    let at = |x: u32, y: u32| {
        let i = ((y * width + x) * 4) as usize;
        Color::rgb(pixels[i], pixels[i + 1], pixels[i + 2])
    };
    let eyes = canopy_eyes(l.canopy, 1.0);
    let bright = |c: Color| luminance(c) > 0.08;
    // The scope: the wave is a full-scale sine, so a column a quarter of
    // the way across has ink near the top of the screen.
    let inner = eyes.scope.inset(4.0);
    let x = (inner.x + inner.width * 0.25) as u32;
    let lit = (inner.y as u32..inner.bottom() as u32)
        .filter(|y| bright(at(x, *y)))
        .count();
    assert!(lit >= 2, "no wave in the scope at column {x}");
    // The spectrum: band 20 is loud, band 80 silent.
    let sinner = eyes.spectrum.inset(4.0);
    let bars = spectrum_bars(sinner, &synthetic_bands());
    let loud = bars[20];
    let quiet = bars[80];
    let y = (loud.y + loud.height / 2.0) as u32;
    assert!(
        bright(at((loud.x + loud.width / 2.0) as u32, y)),
        "the loud band draws no bar"
    );
    let y = (sinner.bottom() - 6.0) as u32;
    assert!(
        !bright(at((quiet.x + quiet.width / 2.0) as u32, y)),
        "a silent band draws a bar"
    );
    // The lamps: three of eight lit.
    let dots = lamp_dots(eyes.lamps.inset(4.0), 8, 1.0);
    let centre = |d: fontelle_ui::layout::Rect| {
        ((d.x + d.width / 2.0) as u32, (d.y + d.height / 2.0) as u32)
    };
    let (x, y) = centre(dots[0]);
    assert!(bright(at(x, y)), "the first lamp is out");
    let (x, y) = centre(dots[7]);
    assert!(
        !bright(at(x, y)),
        "the last lamp is lit with three voices sounding"
    );
}

fn synthetic_bands() -> Vec<f32> {
    (0..96)
        .map(|i| match i {
            18..=22 => -6.0,
            _ => -90.0,
        })
        .collect()
}

/// [`shoot_flopsynth`] with a sky frame carrying a synthetic sound.
fn shoot_flopsynth_with_sky() -> Option<(Vec<u8>, Theme, fontelle_ui::canvas::FlopsynthLayout, u32)>
{
    shoot_flopsynth_sky(Some(fontelle_ui::render::SkyFrame {
        wave: (0..256)
            .map(|i| (i as f32 / 256.0 * std::f32::consts::TAU * 2.0).sin())
            .collect(),
        bands_db: synthetic_bands(),
        voices: (3, 8),
        level: 0.8,
        ..Default::default()
    }))
}

/// `docs/flopsynth-next.md` §1.4(4), Ty's decision §9.2(a): Flopsynth's
/// window keeps **its own palette, dark under both themes**. Grabbed with
/// `--light` at v0.9.0 the window was a purple smear on grey, the *Synth*
/// tab label and the voice read-out white on white, the pictures grey on
/// grey. Serum, Omnisphere and Vital are dark-only and it is not a defect;
/// the skins are the way to change it.
#[test]
fn flopsynths_window_is_dark_and_legible_under_the_light_theme() {
    let Some((light, painted, l, width)) =
        shoot_flopsynth_in(Theme::light_default(), "flopsynth-light")
    else {
        return;
    };
    let Some((dark, _, _, _)) = shoot_flopsynth() else {
        return;
    };
    // The same window: what the studio's theme is makes no difference to it.
    assert_eq!(painted.palette.window, Theme::dark_default().palette.window);
    let ground = |pixels: &[u8]| {
        let (x, y) = (l.body.x as u32 + 2, l.body.bottom() as u32 - 2);
        let i = ((y * width + x) * 4) as usize;
        Color::rgb(pixels[i], pixels[i + 1], pixels[i + 2])
    };
    assert!(
        near(ground(&light), ground(&dark)),
        "the ground under the cards is {:?} in the light theme and {:?} in the dark",
        ground(&light),
        ground(&dark)
    );
    assert!(
        luminance(ground(&light)) < 0.1,
        "the ground is not dark: {:?}",
        ground(&light)
    );
    // The tab label against the canopy's glass, and the picture's ink against
    // its screen: each at least 3:1, which is WCAG's floor for large text and
    // for graphics — and the two that were 1:1 at v0.9.0.
    let (_, tab) = l.tabs[0];
    let tab_contrast = contrast_in(&light, width, tab);
    assert!(
        tab_contrast >= 3.0,
        "the Synth tab reads at {tab_contrast:.1}:1 against the canopy"
    );
    let picture = l.cards[0].picture;
    let picture_contrast = contrast_in(&light, width, picture);
    assert!(
        picture_contrast >= 3.0,
        "the wave reads at {picture_contrast:.1}:1 against its screen"
    );
}

#[test]
fn flopsynths_window_draws_its_cards_and_its_ring() {
    let Some((pixels, theme, l, width)) = shoot_flopsynth() else {
        return;
    };
    let at = |x: u32, y: u32| {
        let i = ((y * width + x) * 4) as usize;
        Color::rgb(pixels[i], pixels[i + 1], pixels[i + 2])
    };

    // A card's header carries its family's rule — the first oscillator's is
    // the accent (§8.1 rule 2) — sampled on the short solid stroke at the
    // rule's left end, off any text. The card itself is glass over a graded
    // sky, so its ground is not one ink to sample; the rule is.
    let header = l.cards[0].header;
    assert!(
        near(
            at(header.x as u32 + 12, header.bottom() as u32 - 1),
            theme.palette.accent
        ),
        "OSC A's header rule is not the accent"
    );
    // And the sky behind the cards is not the flat window colour: the ground
    // is graded, which is the one place this program draws a gradient.
    let sky = at(l.body.x as u32 + 2, l.body.bottom() as u32 - 2);
    assert!(
        !near(sky, theme.palette.panel),
        "the ground under the cards is a panel, not a sky"
    );

    // The modulation arc round the filter's cutoff, sampled **on the band
    // `canvas::ring_hit` answers to** — which is the whole point of the two
    // agreeing — partway along the sweep a depth of 0.6 draws.
    //
    // The arc follows the knob's own 270° sweep from straight up, so `t` here
    // is the drawing's own parameter: 0.5 is twelve o'clock and 0.8 is where a
    // route at +0.6 ends.
    let cell = l.cards[1]
        .cells
        .iter()
        .find(|(param, _)| *param == 0)
        .map(|(_, cell)| *cell)
        .expect("the cutoff has a cell");
    let knob = fontelle_ui::canvas::flop_knob_rect(cell);
    let radius =
        knob.width / 2.0 + fontelle_ui::canvas::RING_GAP + fontelle_ui::canvas::RING_BAND / 2.0;
    let (cx, cy) = (knob.x + knob.width / 2.0, knob.y + knob.height / 2.0);
    let angle = (-0.75f32 + 1.5 * 0.65) * std::f32::consts::PI;
    let (rx, ry) = (cx + radius * angle.sin(), cy - radius * angle.cos());
    let mut found = false;
    for dy in -2i32..=2 {
        for dx in -2i32..=2 {
            let (x, y) = ((rx as i32 + dx) as u32, (ry as i32 + dy) as u32);
            if near(at(x, y), theme.palette.modulation) {
                found = true;
            }
        }
    }
    assert!(
        found,
        "no modulation ink on the ring band at ({rx:.0}, {ry:.0})"
    );

    // And **not** in the caption band above it: the arc reached over the top
    // of the cell once, and struck through the word naming the knob it
    // belonged to.
    let caption_y = cell.y as u32 + 4;
    for x in cell.x as u32..(cell.right() as u32) {
        assert!(
            !near(at(x, caption_y), theme.palette.modulation),
            "the ring is drawn through the caption at ({x}, {caption_y})"
        );
    }
}

// ------------------------------------------------ the take being recorded ---
//
// > *"recording notes also doesnt show you the notes as youre recording them
// > which would be nice and for it like audio to show you it making the clip
// > as youre recording it so you can be sure it is indeed recording it."*

#[test]
fn a_note_being_recorded_is_drawn_in_the_record_colour_before_it_is_kept() {
    let takes = vec![fontelle_ui::document::NotePreview {
        start: 0,
        length: PPQN * 2,
        key: 60,
    }];
    let Some(shot) = shoot_roll_recording(&Arena::default(), &takes) else {
        return;
    };
    let x = tick_to_x(&shot.view, shot.layout.grid, PPQN) as u32;
    let y = (fontelle_ui::canvas::key_to_y(&shot.view, shot.layout.grid, 60)
        + shot.view.key_height / 2.0) as u32;
    let found = shot.at(x, y);
    // The record colour, so it cannot be mistaken for a note that is already
    // in the clip — the same ink the arrangement's take band uses.
    assert!(
        near(found, shot.theme.palette.meter_peak),
        "expected the take's note at ({x}, {y}), found {found:?}"
    );
    // And not on a row it was not played on.
    let empty_y = (fontelle_ui::canvas::key_to_y(&shot.view, shot.layout.grid, 67)
        + shot.view.key_height / 2.0) as u32;
    assert!(!near(shot.at(x, empty_y), shot.theme.palette.meter_peak));
}

#[test]
fn the_notes_being_recorded_appear_in_the_open_clips_block() {
    use fontelle_ui::canvas::{clip_notes, clip_rect};

    // The clip is open and empty; the take is landing in it.
    let mut open = a_note_clip(None);
    open.open = true;
    let takes = std::mem::take(&mut open.notes);
    let clips = vec![open.clone()];
    let Some(shot) = shoot_timeline_recording(&clips, &[], false, &takes, None) else {
        return;
    };
    // Where the notes would be drawn if they were the clip's own.
    let mut as_if = open.clone();
    as_if.notes = takes.clone();
    let block = clip_rect(&shot.view, shot.layout.grid, &as_if);
    let rects = clip_notes(block, shot.layout.grid, &as_if);
    assert_eq!(rects.len(), 4);
    let r = rects[1];
    let found = shot.at((r.x + r.width / 2.0) as u32, (r.y + r.height / 2.0) as u32);
    assert!(
        near(found, shot.theme.palette.meter_peak),
        "the take's notes should be in the block, in the record colour: {found:?}"
    );

    // And a clip that is not the open one gets none of them: the take goes
    // into the clip that is open and nowhere else.
    let mut other = a_note_clip(None);
    other.open = false;
    other.notes.clear();
    let clips = vec![other.clone()];
    let Some(shot) = shoot_timeline_recording(&clips, &[], false, &takes, None) else {
        return;
    };
    let found = shot.at((r.x + r.width / 2.0) as u32, (r.y + r.height / 2.0) as u32);
    assert!(!near(found, shot.theme.palette.meter_peak));
}

// ------------------------------------------- the corrector's own window ---

/// Renders the pitch corrector's console: the trace, the two octaves, the
/// seven cards (`docs/tune-plan.md` §7.7's last item).
///
/// What a shot of the console hands back: the pixels, the theme they were
/// drawn in, where everything ended up, how wide the frame is, and the card
/// names — the last so a test can look a card up by name rather than by an
/// index that moves when the bands are rebalanced.
type TuneShot = (
    Vec<u8>,
    Theme,
    fontelle_ui::canvas::TuneLayout,
    u32,
    Vec<String>,
);

/// The one test that runs `draw_tune` at all, and the one that can **see**.
/// Everything its geometry rests on is pure and tested in `tests/tune.rs`;
/// what is left here is colour and the fact that the picture is drawn at all,
/// and colour is the one thing a geometry test cannot check. Set
/// `FONTELLE_UI_DUMP` to a directory and the PNG lands there.
fn shoot_tune() -> Option<TuneShot> {
    use fontelle_types::{ParamAddress, TUNE_LOCKED, TUNE_VOICED, TuneFrame};
    use fontelle_ui::canvas::{
        FlopsynthCard, FlopsynthPicture, InstrumentGroup, InstrumentParam, ParamKind, TuneView,
        tune_layout,
    };
    use fontelle_ui::render::TuneChrome;

    let theme = Theme::dark_default();
    let shared = headless()?;
    let mut text = TextContext::new();
    let title = text.layout(
        "Vocal \u{2014} TUNE \u{b7} pitch correction",
        &theme.font,
        None,
    );

    let knob = |name: &str, value: f32| InstrumentParam {
        address: ParamAddress::new(format!("mixer/track[0]/insert[0]/{name}")),
        label: name.to_string(),
        value,
        display: format!("{value:.2}"),
        kind: ParamKind::Knob,
        automated: false,
    };
    let card =
        |name: &str, row: usize, columns: usize, params: Vec<InstrumentParam>| FlopsynthCard {
            group: InstrumentGroup {
                name: name.to_string(),
                params,
            },
            picture: FlopsynthPicture::None,
            oscillator: None,
            row,
            aside: false,
            columns,
            removable: false,
            sizes: Vec::new(),
        };
    let choice = |name: &str, at: usize, options: &[&str]| InstrumentParam {
        address: ParamAddress::new(format!("mixer/track[0]/insert[0]/{name}")),
        label: name.to_string(),
        value: at as f32 / (options.len() - 1) as f32,
        display: options[at].to_string(),
        kind: ParamKind::Choice(options.iter().map(|s| s.to_string()).collect()),
        automated: false,
    };

    // Four seconds of somebody singing up to A3 and being pulled onto it: the
    // two lines part and then close, which is the whole story of the effect
    // and the reason the viewport is a picture rather than a number.
    let target = 5700.0;
    let trace: Vec<TuneFrame> = (0..1500)
        .map(|i| {
            let t = i as f32 / 1500.0;
            let sung = target - 60.0 * (1.0 - t) + 18.0 * (t * 40.0).sin();
            let out = sung + (target - sung) * (0.15 + 0.85 * t);
            let mut flags = TUNE_VOICED;
            if (out - target).abs() < fontelle_types::TUNE_LOCK_CENTS {
                flags |= TUNE_LOCKED;
            }
            // A held key over the middle of the take, so the MIDI band is in
            // the shot and can be told from the scale's own correction.
            if (0.55..0.85).contains(&t) {
                flags |= fontelle_types::TUNE_FROM_MIDI;
            }
            // A breath in the middle: unvoiced, and the trace must break
            // rather than run along the floor.
            if (0.44..0.50).contains(&t) {
                flags = 0;
            }
            TuneFrame {
                sung_cents: sung,
                out_cents: out,
                target_cents: target,
                flags,
            }
        })
        .collect();

    let view = TuneView {
        title: "Vocal \u{2014} TUNE \u{b7} pitch correction".to_string(),
        cards: vec![
            card(
                "Input",
                0,
                5,
                vec![
                    choice(
                        "Range",
                        1,
                        &[
                            "soprano",
                            "alto/tenor",
                            "baritone/bass",
                            "instrument",
                            "low",
                        ],
                    ),
                    choice("Mode", 1, &["live", "studio"]),
                    knob("Tracking", 0.5),
                    knob("Gate", 0.3),
                ],
            ),
            card(
                "Correction",
                0,
                5,
                vec![
                    knob("Retune speed", 0.2),
                    knob("Amount", 1.0),
                    knob("Humanize", 0.15),
                    knob("Flex", 0.4),
                    knob("Natural vibrato", 0.6),
                ],
            ),
            card(
                "Voice",
                0,
                7,
                vec![
                    choice("Engine", 0, &["smooth", "hard", "grain"]),
                    knob("Texture", 0.35),
                    knob("Grain", 0.5),
                    knob("Formant", 0.55),
                    knob("Formant follow", 0.8),
                    knob("Transpose", 0.5),
                    knob("Detune", 0.5),
                ],
            ),
            card(
                "Scale",
                1,
                4,
                vec![
                    choice(
                        "Root",
                        9,
                        &[
                            "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
                        ],
                    ),
                    choice(
                        "Scale",
                        1,
                        &[
                            "chromatic",
                            "major",
                            "natural minor",
                            "harmonic minor",
                            "melodic minor",
                            "dorian",
                            "phrygian",
                            "lydian",
                            "mixolydian",
                            "locrian",
                            "major pentatonic",
                            "minor pentatonic",
                            "blues",
                            "whole tone",
                            "custom",
                        ],
                    ),
                    choice("Control", 1, &["scale", "MIDI melody", "MIDI scale"]),
                ],
            ),
            card(
                "MIDI",
                1,
                2,
                vec![
                    InstrumentParam {
                        address: ParamAddress::new(fontelle_ui::canvas::TUNE_SOURCE),
                        label: "Source".to_string(),
                        value: 1.0,
                        display: "Melody".to_string(),
                        kind: ParamKind::Choice(vec![
                            fontelle_ui::canvas::NO_MIDI.to_string(),
                            "Melody".to_string(),
                        ]),
                        automated: false,
                    },
                    choice("MIDI bend", 1, &["off", "on"]),
                ],
            ),
            card(
                "Vibrato",
                1,
                6,
                vec![
                    knob("Depth", 0.25),
                    knob("Rate", 0.5),
                    choice("Sync", 0, &["off", "on"]),
                    choice(
                        "Division",
                        5,
                        &[
                            "1/1", "1/2.", "1/2", "1/4.", "1/2T", "1/4", "1/8.", "1/4T", "1/8",
                            "1/16.", "1/8T", "1/16", "1/16T", "1/32",
                        ],
                    ),
                    knob("Onset", 0.3),
                    choice("Shape", 0, &["sine", "triangle"]),
                ],
            ),
            card(
                "Character",
                1,
                4,
                vec![
                    knob("Drive", 0.35),
                    knob("Crush", 0.0),
                    knob("Air", 0.65),
                    knob("Width", 0.6),
                ],
            ),
            card("Output", 1, 2, vec![knob("Output", 0.5), knob("Mix", 1.0)]),
        ],
        // A major, so the keyboard is a **scale** rather than the chromatic
        // wall a fresh config draws: seven keys lit, five dark, which is the
        // thing §7.4 says a person reads a scale off.
        mask: (1 << 9) | (1 << 11) | (1 << 1) | (1 << 2) | (1 << 4) | (1 << 6) | (1 << 8),
        root: 9,
        held: 1 << 9,
        keyboard_from: 48,
        trace,
        floor_cents: fontelle_types::cents_of_hz(100.0),
        ceiling_cents: fontelle_types::cents_of_hz(1_000.0),
        latency_ms: 21.3,
        engine: "smooth".to_string(),
        mode: "studio".to_string(),
        sources: vec![
            fontelle_ui::canvas::NO_MIDI.to_string(),
            "Melody".to_string(),
        ],
        source: 1,
    };

    let (ew, eh) = fontelle_ui::layout::TUNE_SIZE;
    let panel = fontelle_ui::layout::editor_window_layout(ew as f32, eh as f32, &theme.metrics);
    let l = tune_layout(panel.body, &view);

    let mut labels = Labels::new();
    for card in &view.cards {
        labels.ensure(&card.group.name, &theme.font, &mut text);
        labels.ensure_small(&card.group.name.to_uppercase(), &theme.font, &mut text);
        for param in &card.group.params {
            labels.ensure_small(&param.label, &theme.font, &mut text);
            labels.ensure_small(&param.display, &theme.font, &mut text);
        }
    }
    // The same two lists `app::shape_labels` uses, so what this shot shows is
    // what the window shows.
    for caption in fontelle_ui::canvas::tune_strings(&view) {
        labels.ensure_small(&caption, &theme.font, &mut text);
    }
    for name in fontelle_types::TUNE_ROOTS {
        labels.ensure_small(name, &theme.font, &mut text);
    }

    let mut scene = vello::Scene::new();
    fontelle_ui::render::draw_editor_window(
        &mut scene,
        &theme,
        &panel,
        &labels,
        &title,
        &fontelle_ui::render::EditorWindowChrome::Tune(TuneChrome {
            layout: l.clone(),
            view: &view,
            hover: None,
            active: None,
            hover_at: (f32::MIN, f32::MIN),
        }),
        None,
        None,
        None,
        None,
    );
    let pixels = shared
        .lock()
        .expect("the shared renderer")
        .render(&scene, ew, eh, theme.palette.window)
        .expect("the scene must render");
    dump_sized(&pixels, "tune", ew, eh);
    let names = view.cards.iter().map(|c| c.group.name.clone()).collect();
    Some((pixels, theme, l, ew, names))
}

#[test]
fn the_correctors_console_draws_its_trace_its_keys_and_its_cards() {
    let Some((pixels, theme, l, width, view_names)) = shoot_tune() else {
        return;
    };
    let at = |x: u32, y: u32| {
        let i = ((y * width + x) * 4) as usize;
        Color::rgb(pixels[i], pixels[i + 1], pixels[i + 2])
    };

    // **The ground is a ship's console, not a window.** `draw_tune_ground`
    // lays a near-black grade with a lattice over it, so the ink below the
    // cards is not the flat window colour it would be if the ground had been
    // skipped — the one thing the geometry tests cannot tell.
    assert!(
        !near(
            at(l.body.x as u32 + 2, l.body.bottom() as u32 - 2),
            theme.palette.window
        ),
        "the console is drawn on a bare window rather than on its ground"
    );

    // **The trace is drawn.** Not a colour match — the corrected line is a
    // glow polyline over a graded ground and the ink under it depends where
    // you sample — but the viewport must not be one flat colour, which is
    // what an empty picture would be.
    let mut inks = std::collections::HashSet::new();
    let (vx, vy) = (l.viewport.x as u32, l.viewport.y as u32);
    for y in 0..l.viewport.height as u32 {
        for x in 0..l.viewport.width as u32 {
            let c = at(vx + x, vy + y);
            inks.insert([c.0[0] / 8, c.0[1] / 8, c.0[2] / 8]);
        }
    }
    assert!(
        inks.len() > 8,
        "the viewport is drawing {} inks — a trace was not drawn",
        inks.len()
    );

    // **The keyboard says what the scale is.** Not a colour match — every key
    // is a translucent fill over a graded ground, and what a ring or a dot
    // blends to is not a palette entry — but the three states must be three
    // different inks, which is the whole of what §7.4 asks the picture to do.
    //
    // The mask here is A major, so C is out, B is in, and A is in and held.
    // Sampled low in each natural, under where an accidental reaches and
    // clear of the root ring's inset.
    let ink_of = |class: usize| {
        let k = &l.keys[class];
        at((k.x + k.width / 2.0) as u32, (k.y + k.height * 0.85) as u32)
    };
    let (out, inside, held) = (ink_of(0), ink_of(11), ink_of(9));
    assert!(
        !near(out, inside),
        "a key in the scale is drawn like one out of it: {out:?} vs {inside:?}"
    );
    assert!(
        !near(held, inside),
        "a key held on the MIDI source is drawn like an idle one: {held:?}"
    );

    // **A card is drawn in its family's ink** (`render::tune_ink`): the card
    // about notes in the colour notes are drawn in everywhere else in this
    // program, and the plumbing in the muted text. The ink goes on the edge
    // light along the card's top, at alpha, over a graded ground — so what is
    // measured is that the families **differ**, not what either blends to.
    // Fourteen in from the frame is past the chamfer the edge light starts
    // after and clear of the corner glow.
    let edge_of = |card: &fontelle_ui::canvas::CardLayout| {
        at((card.frame.x + 14.0) as u32, card.frame.y as u32)
    };
    // By **name**, not by index: the cards were reordered once already when
    // §4.8 added Character and the bands were rebalanced, and an index here
    // silently started asserting about a different card.
    let card_named = |name: &str| {
        let at = view_names
            .iter()
            .position(|other| other == name)
            .unwrap_or_else(|| panic!("no {name} card"));
        &l.cards[at]
    };
    let midi = card_named("MIDI");
    let output = card_named("Output");
    let voice = card_named("Voice");
    assert!(
        !near(edge_of(midi), edge_of(output)),
        "the MIDI card and the Output card are drawn in one ink"
    );
    assert!(
        !near(edge_of(voice), edge_of(output)),
        "the Voice card and the Output card are drawn in one ink"
    );
}

// ------------------------------------------------ a box you can type in ---

/// The name prompt's field, drawn — the one test that runs `draw_text_field`.
///
/// > *"it doesn't look like a input field it's just text on a background
/// > making it look like it's a label."*
///
/// What a geometry test cannot see is exactly what was wrong: whether the box
/// reads as somewhere text goes *in*. So this draws one with a caret and one
/// with a selection, and the PNG is the answer.
fn shoot_field(selected: bool) -> Option<(Vec<u8>, Theme, Rect, u32, u32)> {
    use fontelle_ui::canvas::TextEntry;
    use fontelle_ui::render::TextFieldChrome;

    let theme = Theme::dark_default();
    let shared = headless()?;
    let mut text = TextContext::new();
    let (w, h) = (360u32, 64u32);
    let field = Rect::new(16.0, 16.0, 328.0, 30.0);

    let mut entry = TextEntry::new("Verse Two");
    if selected {
        entry.select_all();
    }
    let mut measure = |at: usize| {
        if at == 0 {
            0.0
        } else {
            text.layout(&entry.text()[..at], &theme.font, None).width
        }
    };
    let caret_x = measure(entry.caret());
    let selection = entry.selection().map(|(a, b)| (measure(a), measure(b)));

    let mut labels = Labels::new();
    labels.ensure(entry.text(), &theme.font, &mut text);

    let mut scene = vello::Scene::new();
    // The panel it sits on, so the recess has something to be recessed into.
    // Drawn straight, since `fill_rect` is the renderer's own.
    scene.fill(
        vello::peniko::Fill::NonZero,
        vello::kurbo::Affine::IDENTITY,
        theme.palette.panel_header.to_peniko(),
        None,
        &vello::kurbo::Rect::new(0.0, 0.0, w as f64, h as f64),
    );
    fontelle_ui::render::draw_text_field(
        &mut scene,
        &theme,
        &labels,
        field,
        &TextFieldChrome {
            entry,
            caret_x,
            selection,
            placeholder: "Name the new project",
            caret_on: true,
        },
    );
    let pixels = shared
        .lock()
        .expect("the shared renderer")
        .render(&scene, w, h, theme.palette.window)
        .expect("the scene must render");
    dump_sized(
        &pixels,
        if selected {
            "field-selected"
        } else {
            "field-caret"
        },
        w,
        h,
    );
    Some((pixels, theme, field, w, h))
}

#[test]
fn a_text_field_is_a_box_with_a_caret_in_it() {
    let Some((pixels, theme, field, width, _)) = shoot_field(false) else {
        return;
    };
    let at = |x: u32, y: u32| {
        let i = ((y * width + x) * 4) as usize;
        Color::rgb(pixels[i], pixels[i + 1], pixels[i + 2])
    };
    // **A recess, not a label.** The inside of the field is darker than the
    // panel it sits on — which is the whole of what makes it read as somewhere
    // text goes in.
    let inside = at(
        (field.x + field.width - 8.0) as u32,
        (field.y + field.height / 2.0) as u32,
    );
    let outside = at(4, 4);
    assert!(
        !near(inside, outside),
        "the field is the same colour as the panel: it is still a label"
    );
    // **And a caret**, in the accent, somewhere along the text.
    let mut found = false;
    for x in (field.x as u32)..(field.right() as u32) {
        for dy in 4..(field.height as u32 - 4) {
            if near(at(x, field.y as u32 + dy), theme.palette.accent) {
                found = true;
            }
        }
    }
    assert!(found, "there is no caret drawn in the field");
}

#[test]
fn a_selection_is_washed_rather_than_inverted() {
    let Some((pixels, _theme, field, width, _)) = shoot_field(true) else {
        return;
    };
    let Some((plain, _, _, _, _)) = shoot_field(false) else {
        return;
    };
    let at = |buf: &[u8], x: u32, y: u32| {
        let i = ((y * width + x) * 4) as usize;
        Color::rgb(buf[i], buf[i + 1], buf[i + 2])
    };
    // Over the first character, the selected shot differs from the plain one:
    // there is a wash under the text.
    let (x, y) = (
        (field.x + 10.0) as u32,
        (field.y + field.height / 2.0) as u32,
    );
    assert!(
        !near(at(&pixels, x, y), at(&plain, x, y)),
        "selecting the text changed nothing under it"
    );
}

// ------------------------- a row being carried says where it is going ---

/// The window with a row from the browser held over `pointer`.
///
/// > *"i cant see any visuals of the thing being dragged ... right now theres
/// > virtually no feedback until you actually finish dragging it."*
///
/// The target is worked out by the same `canvas::carry_target` the release
/// reads, so this shot is the drop's own answer drawn — which is the property
/// the whole feature rests on.
fn shoot_carry(
    pointer: Option<(f32, f32)>,
) -> Option<(Vec<u8>, Theme, fontelle_ui::canvas::RackLayout, u32, u32)> {
    use fontelle_ui::canvas::rack_layout;
    use fontelle_ui::canvas::{Carried, CarryRack, CarryScene, carry_note, carry_target};
    use fontelle_ui::document::ChannelInfo;
    use fontelle_ui::render::{CarryChrome, RackChrome};

    let theme = Theme::dark_default();
    let shared = headless()?;
    let mut text = TextContext::new();
    let title = text.layout("Fontelle", &theme.font, None);

    let channels: Vec<ChannelInfo> = ["Bass", "Keys"]
        .iter()
        .map(|name| ChannelInfo {
            name: (*name).to_string(),
            muted: false,
            soloed: false,
            has_instrument: true,
            route: None,
        })
        .collect();

    const CARRY_H: u32 = 480;
    let layout = window_layout(
        W as f32,
        CARRY_H as f32,
        &theme.metrics,
        DEFAULT_TIMELINE_HEIGHT,
    );
    let rack = rack_layout(layout.rack.body, &theme.metrics, channels.len(), 0);

    let names: Vec<String> = channels.iter().map(|c| c.name.clone()).collect();
    let carried = "CoolBreak.wav";
    let target = pointer.map(|(x, y)| {
        carry_target(
            &CarryScene {
                carried: Carried::Audio,
                rack: Some(CarryRack {
                    frame: layout.rack.frame,
                    layout: &rack,
                }),
                panel: Some(layout.browser.frame),
                timeline: None,
                name: None,
                oscillators: &[],
                desktop: false,
            },
            x,
            y,
        )
    });
    let note = target
        .map(|target| carry_note(&target, &names, &[], 4))
        .unwrap_or_default();

    let mut labels = Labels::new();
    for channel in &channels {
        labels.ensure(&channel.name, &theme.font, &mut text);
    }
    labels.ensure("Master", &theme.font, &mut text);
    for tab in fontelle_ui::document::RackTab::ALL {
        labels.ensure(tab.label(), &theme.font, &mut text);
    }
    labels.ensure(carried, &theme.font, &mut text);
    labels.ensure_small(&note, &theme.font, &mut text);

    let mut scene = vello::Scene::new();
    fontelle_ui::render::draw_window(
        &mut scene,
        &theme,
        &layout,
        &Chrome {
            field: None,
            panel_title: &title,
            transport: TransportChrome {
                layout: transport_bar_layout(layout.transport, &theme.metrics),
                view: TransportView::unavailable(),
                meters: [Meter::new(); 2],
                readout: &text.layout("1.1.0", &theme.font, None),
                tempo: &text.layout("120.00", &theme.font, None),
                signature: &text.layout("4/4", &theme.font, None),
                mode: &text.layout("Song", &theme.font, None),
                hover: None,
                marker_sample: 0,
                clip_mode: false,
                tempo_field: None,
            },
            roll: None,
            rack: Some(RackChrome {
                panel: layout.rack,
                layout: rack.clone(),
                channels: &channels,
                selected: 0,
                hover: None,
                route_names: &["Master".to_string()],
                strips: 1,
                route_menu: None,
                route_menu_open: None,
                renaming: None,
                rename: None,
            }),
            prefabs: None,
            browser: None,
            timeline: None,
            mixer: None,
            tabs: fontelle_ui::layout::editor_tabs(layout.panel.header, &theme.metrics),
            tab: fontelle_ui::layout::EditorTab::Roll,
            hover_tab: None,
            browser_title: "Soundfonts",
            labels: &labels,
            status: "",
            toast: None,
            confirm: None,
            notices: Default::default(),
            tooltip: None,
            menu: None,
            carry: target.zip(pointer).map(|(target, at)| CarryChrome {
                label: carried,
                note: &note,
                at,
                target,
                bounds: layout.window,
                lifted: false,
            }),
            welcome: None,
            keybinds: None,
        },
    );
    let pixels = shared
        .lock()
        .expect("the shared renderer")
        .render(&scene, W, CARRY_H, theme.palette.window)
        .expect("the scene must render");
    dump_sized(
        &pixels,
        &format!("carry-{}", if pointer.is_some() { "held" } else { "none" }),
        W,
        CARRY_H,
    );
    Some((pixels, theme, rack, W, CARRY_H))
}

/// **The row a sound would land on is drawn as the row it would land on.**
///
/// Before this there was nothing at all between the press and the release: the
/// only way to find out whether a drop would work was to do it.
#[test]
fn a_carried_sound_lights_the_channel_it_is_over() {
    let Some((quiet, theme, rack, width, _)) = shoot_carry(None) else {
        return;
    };
    let row = rack.rows.get(1).expect("a second row").frame;
    // The bottom-right corner of the row: the chip hangs down and to the
    // right of the pointer, so it lands outside the row and what is counted
    // inside it is the mark and nothing else.
    let Some((held, _, _, _, _)) = shoot_carry(Some((row.right() - 2.0, row.bottom() - 2.0)))
    else {
        return;
    };
    let accent_in = |pixels: &[u8], rect: Rect| {
        let mut count = 0;
        for dy in 0..rect.height as u32 {
            for dx in 0..rect.width as u32 {
                let i = (((rect.y as u32 + dy) * width + rect.x as u32 + dx) * 4) as usize;
                if near(
                    Color(pixels[i..i + 4].try_into().expect("four bytes")),
                    theme.palette.accent,
                ) {
                    count += 1;
                }
            }
        }
        count
    };
    assert!(
        accent_in(&held, row) > accent_in(&quiet, row) + 8,
        "the channel under the pointer should be visibly marked"
    );
    let other = rack.rows.first().expect("a first row").frame;
    assert_eq!(
        accent_in(&held, other),
        accent_in(&quiet, other),
        "and the channel the pointer is not over should be left alone"
    );
}

/// **And the thing being carried is drawn under the pointer.**
#[test]
fn a_carried_sound_is_drawn_under_the_pointer() {
    let Some((quiet, _theme, rack, width, _)) = shoot_carry(None) else {
        return;
    };
    let row = rack.rows.get(1).expect("a second row").frame;
    let (px, py) = (row.right() - 2.0, row.bottom() - 2.0);
    let Some((held, _, _, _, _)) = shoot_carry(Some((px, py))) else {
        return;
    };
    // A box just inside where the chip hangs: 14 pixels clear of the pointer,
    // and the chip is wider and taller than this.
    let mut changed = 0;
    for dy in 0..12u32 {
        for dx in 0..12u32 {
            let x = px as u32 + 16 + dx;
            let y = py as u32 + 16 + dy;
            let i = ((y * width + x) * 4) as usize;
            if quiet[i..i + 4] != held[i..i + 4] {
                changed += 1;
            }
        }
    }
    assert!(
        changed > 0,
        "there is nothing drawn under the pointer: the drag is invisible again"
    );
}

/// **A drop that would do nothing says so, in the warning ink.**
///
/// *"please also ensure that it shows a visual of where its about to go so you
/// know youre actually placing it right / that is a legal action before you do
/// it."* The half of that sentence about **legality**: over the piano roll a
/// sound has nowhere to go, and the chip has to be the thing that says it.
#[test]
fn a_carried_sound_over_nowhere_is_refused_where_you_can_see_it() {
    let Some((_quiet, theme, _rack, width, height)) = shoot_carry(None) else {
        return;
    };
    // The middle of the editor panel — a place with nothing that takes a
    // sound.
    let (px, py) = (width as f32 * 0.6, height as f32 * 0.6);
    let Some((held, _, _, _, _)) = shoot_carry(Some((px, py))) else {
        return;
    };
    let mut warned = 0;
    for dy in 0..60u32 {
        for dx in 0..200u32 {
            let x = px as u32 + 4 + dx;
            let y = py as u32 + 4 + dy;
            if x >= width || y >= height {
                continue;
            }
            let i = ((y * width + x) * 4) as usize;
            if near(
                Color(held[i..i + 4].try_into().expect("four bytes")),
                theme.palette.meter_peak,
            ) {
                warned += 1;
            }
        }
    }
    assert!(
        warned > 0,
        "a refused drop is drawn exactly like one that would work"
    );
}

// --- the start menu ---

/// Renders the start menu over an otherwise empty window, at a size the
/// real window opens at, so the card has room to be itself.
fn shoot_welcome(theme: Theme, recent: &[fontelle_ui::RecentProject]) -> Option<Shot> {
    shoot_welcome_status(
        theme,
        recent,
        fontelle_ui::UpdateStatus::Available {
            version: "9.9.9".to_string(),
        },
        "start-menu",
    )
}

fn shoot_welcome_status(
    theme: Theme,
    recent: &[fontelle_ui::RecentProject],
    status: fontelle_ui::UpdateStatus,
    suffix: &str,
) -> Option<Shot> {
    use fontelle_ui::canvas::welcome_layout;
    use fontelle_ui::render::WelcomeChrome;
    let (width, height) = (1000u32, 620u32);
    let shared = headless()?;
    let layout = window_layout(
        width as f32,
        height as f32,
        &theme.metrics,
        DEFAULT_TIMELINE_HEIGHT,
    );
    let mut text = TextContext::new();
    let title = text.layout("Fontelle", &theme.font, None);
    let big = fontelle_ui::theme::FontTokens {
        family: theme.font.family.clone(),
        size: theme.font.size * 2.0,
        line_height: theme.font.line_height,
    };
    let big_title = text.layout("Fontelle", &big, None);
    let bar = transport_bar_layout(layout.transport, &theme.metrics);
    let readout = text.layout("0", &theme.font, None);

    let (line, button) = fontelle_ui::canvas::update_line(&status, "0.1.0");
    let progress = fontelle_ui::canvas::update_progress(&status);
    let welcome = welcome_layout(
        layout.window,
        &theme.metrics,
        recent.len(),
        button.is_some() || progress.is_some(),
    );
    // Everything the menu will look up, shaped — the same contract the
    // window keeps in `shape_labels`.
    let mut labels = Labels::new();
    for s in [
        fontelle_ui::canvas::NEW_PROJECT_LABEL,
        fontelle_ui::canvas::OPEN_PROJECT_LABEL,
        fontelle_ui::canvas::RECENT_HEADING,
        fontelle_ui::canvas::NOTHING_RECENT,
        fontelle_ui::canvas::FOOTER_TEXT,
        fontelle_ui::canvas::WEBSITE_LABEL,
        fontelle_ui::canvas::REPOSITORY_LABEL,
        "Version 0.1.0",
        "\u{00d7}",
    ] {
        labels.ensure(s, &theme.font, &mut text);
    }
    let line = text.layout(&line, &theme.font, Some(welcome.update.width));
    let message = text.layout("", &theme.font, None);
    if let Some(button) = button {
        labels.ensure(button, &theme.font, &mut text);
    }
    for project in recent {
        labels.ensure(&project.name, &theme.font, &mut text);
        labels.ensure_small(&project.path.display().to_string(), &theme.font, &mut text);
    }

    let mut scene = vello::Scene::new();
    draw_window(
        &mut scene,
        &theme,
        &layout,
        &Chrome {
            field: None,
            panel_title: &title,
            transport: TransportChrome {
                layout: bar,
                view: TransportView::unavailable(),
                meters: [Meter::new(); 2],
                readout: &readout,
                tempo: &readout,
                signature: &readout,
                mode: &readout,
                hover: None,
                marker_sample: 0,
                clip_mode: false,
                tempo_field: None,
            },
            roll: None,
            rack: None,
            prefabs: None,
            browser: None,
            timeline: None,
            mixer: None,
            tabs: fontelle_ui::layout::editor_tabs(layout.panel.header, &theme.metrics),
            tab: fontelle_ui::layout::EditorTab::Roll,
            hover_tab: None,
            browser_title: "Soundfonts",
            labels: &labels,
            status: "",
            toast: None,
            confirm: None,
            notices: Default::default(),
            tooltip: None,
            menu: None,
            carry: None,
            welcome: Some(WelcomeChrome {
                layout: welcome.clone(),
                title: &big_title,
                version: "Version 0.1.0",
                update: &line,
                update_button: button,
                progress,
                recent,
                hover: Some(fontelle_ui::canvas::WelcomeHit::NewProject),
                message: &message,
            }),
            keybinds: None,
        },
    );
    let pixels = shared
        .lock()
        .expect("the shared renderer")
        .render(&scene, width, height, theme.palette.window)
        .expect("rendering a scene that fits in memory");
    dump_sized(&pixels, &format!("{}-{suffix}", theme.name), width, height);
    Some(Shot {
        pixels,
        theme,
        layout,
        bar,
        width,
    })
}

fn recent_projects() -> Vec<fontelle_ui::RecentProject> {
    vec![
        fontelle_ui::RecentProject {
            name: "Night Drive".to_string(),
            path: "/home/someone/Music/Night Drive.fontelle".into(),
            exists: true,
        },
        fontelle_ui::RecentProject {
            name: "Moved Away".to_string(),
            path: "/media/gone/Moved Away.fontelle".into(),
            exists: false,
        },
    ]
}

#[test]
fn the_start_menu_covers_the_studio_and_wears_the_logo_in_the_themes_ink() {
    for theme in [Theme::dark_default(), Theme::light_default()] {
        let Some(shot) = shoot_welcome(theme, &recent_projects()) else {
            return;
        };
        let layout =
            fontelle_ui::canvas::welcome_layout(shot.layout.window, &shot.theme.metrics, 2, true);
        // The card is a panel on the window's ground, not the studio: the
        // top-left corner, where the transport bar would be, is the ground.
        assert!(near(shot.at(2, 2), shot.theme.palette.window));
        // The logo is drawn in the theme's text colour. Somewhere in its
        // square a pixel is that ink — the mark is a monogram, so no one
        // point is certain, but a square with none of it is a logo not
        // drawn.
        let ink = shot.theme.palette.text;
        let mut inked = 0;
        for y in (layout.logo.y as u32)..(layout.logo.bottom() as u32) {
            for x in (layout.logo.x as u32)..(layout.logo.right() as u32) {
                if near(shot.at(x, y), ink) {
                    inked += 1;
                }
            }
        }
        assert!(
            inked > 200,
            "{}: {inked} pixels of ink in the logo",
            shot.theme.name
        );
        // The hovered button is lit in the accent.
        let (bx, by) = (
            layout.new_button.x as u32 + 3,
            layout.new_button.y as u32 + 3,
        );
        assert!(
            near(shot.at(bx, by), shot.theme.palette.accent),
            "{}: the New project button under the pointer is not lit",
            shot.theme.name
        );
    }
}

#[test]
fn the_start_menu_with_nothing_recent_still_renders() {
    let Some(shot) = shoot_welcome(Theme::dark_default(), &[]) else {
        return;
    };
    assert!(near(shot.at(2, 2), shot.theme.palette.window));
}

/// *"make it so theres a progress bar when installing an update"*: while the
/// archive comes down the offer's slot holds a bar, filled in the accent to
/// the fraction done.
#[test]
fn the_start_menu_shows_a_progress_bar_while_an_update_downloads() {
    let Some(shot) = shoot_welcome_status(
        Theme::dark_default(),
        &recent_projects(),
        fontelle_ui::UpdateStatus::Downloading {
            version: "9.9.9".to_string(),
            done: 6_000_000,
            total: Some(8_000_000),
        },
        "start-menu-downloading",
    ) else {
        return;
    };
    let layout =
        fontelle_ui::canvas::welcome_layout(shot.layout.window, &shot.theme.metrics, 2, true);
    let rect = layout.update_button.expect("a slot for the bar");
    // Three-quarters along, the fill is the accent; past the end, it is not.
    let y = rect.y as u32 + rect.height as u32 / 2;
    let lit = rect.x as u32 + (rect.width * 0.7) as u32;
    let past = rect.x as u32 + rect.width as u32 - 3;
    assert!(
        near(shot.at(lit, y), shot.theme.palette.accent),
        "the filled part of the bar is not the accent"
    );
    assert!(
        !near(shot.at(past, y), shot.theme.palette.accent),
        "the bar is full past the fraction done"
    );
}

/// The settings tab, drawn as controls rather than click-to-step values: a
/// slider's groove, a choice's caret, a switch's pill. A look at Phase B
/// through the real pipeline — `FONTELLE_UI_DUMP=<dir>` writes it out.
#[test]
fn shoot_settings_controls() {
    use fontelle_ui::canvas::{BrowserMode, SettingControl, browser_layout_for};
    use fontelle_ui::document::LibraryEntry;
    use fontelle_ui::render::BrowserChrome;

    let Some(shared) = headless() else {
        return;
    };
    let theme = Theme::dark_default();
    let mut text = TextContext::new();
    let title = text.layout("Fontelle", &theme.font, None);

    // One of each kind of control, so the dump shows a groove, a caret, a pill
    // and a plain button side by side.
    let rows: Vec<(LibraryEntry, SettingControl)> = vec![
        (
            LibraryEntry::file("MIDI input", ""),
            SettingControl::Heading,
        ),
        (
            LibraryEntry::file("Velocity curve", "Linear"),
            SettingControl::Choice {
                options: ["Linear", "Soft", "Hard", "Fixed"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
                chosen: 0,
            },
        ),
        (
            LibraryEntry::file("Fixed velocity", "100"),
            SettingControl::Slider {
                fraction: 100.0 / 127.0,
            },
        ),
        (
            LibraryEntry::file("Velocity min", "0"),
            SettingControl::Slider { fraction: 0.0 },
        ),
        (
            LibraryEntry::file("Velocity max", "127"),
            SettingControl::Slider { fraction: 1.0 },
        ),
        (
            LibraryEntry::file("Keyboard transpose", "+3 st"),
            SettingControl::Slider {
                fraction: (3.0 + 24.0) / 48.0,
            },
        ),
        (
            LibraryEntry::file("Channel", "All"),
            SettingControl::Choice {
                options: std::iter::once("All".to_string())
                    .chain((1..=16).map(|n| n.to_string()))
                    .collect(),
                chosen: 0,
            },
        ),
        (LibraryEntry::file("Updates", ""), SettingControl::Heading),
        (
            LibraryEntry::file("Check at launch", "On"),
            SettingControl::Switch { on: true },
        ),
        (
            LibraryEntry::file("Add plugin folder", "Not set \u{2014} click"),
            SettingControl::Button,
        ),
    ];
    let entries: Vec<LibraryEntry> = rows.iter().map(|(e, _)| e.clone()).collect();
    let controls: Vec<SettingControl> = rows.iter().map(|(_, c)| c.clone()).collect();

    // Taller than the file's usual frame so the whole settings list has room —
    // a 360-pixel window leaves the sidebar list only a couple of rows.
    const SETTINGS_H: u32 = 760;
    let layout = window_layout(
        W as f32,
        SETTINGS_H as f32,
        &theme.metrics,
        DEFAULT_TIMELINE_HEIGHT,
    );
    let bl = browser_layout_for(
        layout.browser.body,
        &theme.metrics,
        BrowserMode::Settings,
        entries.len(),
        0,
        0,
        0,
    );

    let mut labels = Labels::new();
    for entry in &entries {
        labels.ensure(&entry.name, &theme.font, &mut text);
        if !entry.detail.is_empty() {
            labels.ensure(&entry.detail, &theme.font, &mut text);
        }
    }
    for mode in BrowserMode::ALL {
        labels.ensure(mode.label(), &theme.font, &mut text);
    }
    labels.ensure(
        fontelle_ui::render::OPEN_CONFIG_FOLDER,
        &theme.font,
        &mut text,
    );

    let mut scene = vello::Scene::new();
    draw_window(
        &mut scene,
        &theme,
        &layout,
        &Chrome {
            field: None,
            panel_title: &title,
            transport: TransportChrome {
                layout: transport_bar_layout(layout.transport, &theme.metrics),
                view: TransportView::unavailable(),
                meters: [Meter::new(); 2],
                readout: &text.layout("1.1.0", &theme.font, None),
                tempo: &text.layout("120.00", &theme.font, None),
                signature: &text.layout("4/4", &theme.font, None),
                mode: &text.layout("Song", &theme.font, None),
                hover: None,
                marker_sample: 0,
                clip_mode: false,
                tempo_field: None,
            },
            roll: None,
            rack: None,
            prefabs: None,
            browser: Some(BrowserChrome {
                panel: layout.browser,
                layout: bl,
                mode: BrowserMode::Settings,
                import_kind: fontelle_types::FolderKind::Midi,
                query: "",
                files: &entries,
                presets: &[],
                selected_file: None,
                selected_preset: None,
                searching: false,
                focus_preset: None,
                hover: None,
                settings_controls: &controls,
                focus_setting: Some(5),
            }),
            timeline: None,
            mixer: None,
            tabs: fontelle_ui::layout::editor_tabs(layout.panel.header, &theme.metrics),
            tab: fontelle_ui::layout::EditorTab::Roll,
            hover_tab: None,
            browser_title: "Settings",
            labels: &labels,
            status: "",
            toast: None,
            confirm: None,
            notices: Default::default(),
            tooltip: None,
            menu: None,
            carry: None,
            welcome: None,
            keybinds: None,
        },
    );
    let pixels = shared
        .lock()
        .expect("the shared renderer")
        .render(&scene, W, SETTINGS_H, theme.palette.window)
        .expect("the settings scene must render");
    dump_sized(&pixels, "settings-controls", W, SETTINGS_H);

    // The controls actually drew: a slider fill, a switch and the focus outline
    // are all painted in the accent, so the panel is not the old plain list of
    // values. A count rather than one pixel, so a stray accent edge is not
    // enough to pass.
    let [ar, ag, ab, _] = theme.palette.accent.0;
    let accent_pixels = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|px| {
            (px[0] as i32 - ar as i32).abs() <= 6
                && (px[1] as i32 - ag as i32).abs() <= 6
                && (px[2] as i32 - ab as i32).abs() <= 6
        })
        .count();
    assert!(
        accent_pixels > 200,
        "the settings controls did not paint (only {accent_pixels} accent pixels)"
    );
}

// --- the keyboard shortcuts sheet ---

/// The sheet over an otherwise empty studio, at a size the real window
/// opens at, with every string it looks up shaped — the same contract the
/// window keeps in `shape_labels`.
fn shoot_keybinds(theme: Theme, scroll: f32) -> Option<(Vec<u8>, Theme, u32, u32)> {
    use fontelle_ui::canvas::{
        KEYBIND_SECTIONS, KEYBINDS_CLOSE, KEYBINDS_HINT, KEYBINDS_LISTENING, KEYBINDS_PRESS,
        KEYBINDS_RESET, KEYBINDS_TITLE, Keymap,
    };
    let keymap = Keymap::default();
    let (width, height) = (1100u32, 700u32);
    let shared = headless()?;
    let layout = window_layout(
        width as f32,
        height as f32,
        &theme.metrics,
        DEFAULT_TIMELINE_HEIGHT,
    );
    let mut text = TextContext::new();
    let title = text.layout("Fontelle", &theme.font, None);
    let bar = transport_bar_layout(layout.transport, &theme.metrics);
    let readout = text.layout("0", &theme.font, None);

    let mut labels = Labels::new();
    labels.ensure(KEYBINDS_TITLE, &theme.font, &mut text);
    labels.ensure(KEYBINDS_CLOSE, &theme.font, &mut text);
    for small in [
        KEYBINDS_HINT,
        KEYBINDS_LISTENING,
        KEYBINDS_PRESS,
        KEYBINDS_RESET,
    ] {
        labels.ensure_small(small, &theme.font, &mut text);
    }
    for section in KEYBIND_SECTIONS {
        labels.ensure(section.title, &theme.font, &mut text);
        for bind in section.binds {
            labels.ensure_small(&bind.keys(&keymap), &theme.font, &mut text);
            labels.ensure_small(bind.does(), &theme.font, &mut text);
        }
    }

    let mut scene = vello::Scene::new();
    draw_window(
        &mut scene,
        &theme,
        &layout,
        &Chrome {
            field: None,
            panel_title: &title,
            transport: TransportChrome {
                layout: bar,
                view: TransportView::unavailable(),
                meters: [Meter::new(); 2],
                readout: &readout,
                tempo: &readout,
                signature: &readout,
                mode: &readout,
                hover: None,
                marker_sample: 0,
                clip_mode: false,
                tempo_field: None,
            },
            roll: None,
            rack: None,
            prefabs: None,
            browser: None,
            timeline: None,
            mixer: None,
            tabs: fontelle_ui::layout::editor_tabs(layout.panel.header, &theme.metrics),
            tab: fontelle_ui::layout::EditorTab::Roll,
            hover_tab: None,
            browser_title: "",
            labels: &labels,
            status: "",
            toast: None,
            confirm: None,
            notices: Default::default(),
            tooltip: None,
            menu: None,
            carry: None,
            welcome: None,
            // With a row listening, so the dump shows what that looks like.
            keybinds: Some(fontelle_ui::render::KeybindsChrome {
                scroll,
                keymap: &keymap,
                listening: Some(fontelle_ui::canvas::Action::Undo),
                hover: Some(fontelle_ui::canvas::Action::Save),
                note: "",
            }),
        },
    );
    let pixels = shared
        .lock()
        .expect("the shared renderer")
        .render(&scene, width, height, theme.palette.window)
        .expect("rendering a scene that fits in memory");
    let suffix = if scroll > 0.0 {
        "keybinds-scrolled"
    } else {
        "keybinds"
    };
    dump_sized(&pixels, &format!("{}-{suffix}", theme.name), width, height);
    Some((pixels, theme, width, height))
}

#[test]
fn the_shortcuts_sheet_is_drawn_over_the_studio_with_its_headings_in_the_accent() {
    for theme in [Theme::dark_default(), Theme::light_default()] {
        let Some((pixels, theme, width, height)) = shoot_keybinds(theme, 0.0) else {
            return;
        };
        let l = fontelle_ui::canvas::keybinds_layout(
            fontelle_ui::layout::Rect::new(0.0, 0.0, width as f32, height as f32),
            &theme.metrics,
            0.0,
        );
        // The card is the panel colour, not the window's: the sheet is there.
        let at = |x: f32, y: f32| {
            let i = ((y as u32) * width + x as u32) as usize * 4;
            Color([pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]])
        };
        assert!(
            near(at(l.frame.x + 4.0, l.frame.y + 4.0), theme.palette.panel),
            "{}: the card's corner is not the panel colour",
            theme.name
        );
        // Every heading drew something in the accent along its row.
        for row in &l.rows {
            let fontelle_ui::canvas::KeybindRow::Heading { rect, section } = row else {
                continue;
            };
            let [ar, ag, ab, _] = theme.palette.accent.0;
            let mut accent = 0;
            for y in rect.y as u32..rect.bottom() as u32 {
                for x in rect.x as u32..(rect.x + 120.0) as u32 {
                    let i = (y * width + x) as usize * 4;
                    if pixels[i].abs_diff(ar) < 40
                        && pixels[i + 1].abs_diff(ag) < 40
                        && pixels[i + 2].abs_diff(ab) < 40
                    {
                        accent += 1;
                    }
                }
            }
            assert!(
                accent > 20,
                "{}: heading {section} drew {accent} accent pixels — its caption is missing",
                theme.name
            );
        }
    }
    // And scrolled, so the dump shows the list moving.
    let _ = shoot_keybinds(Theme::dark_default(), 300.0);
}

/// The notepad's window, drawn through the real pipeline
/// (`docs/effects-catalogue.md` §2.8) — `FONTELLE_UI_DUMP=<dir>` writes it
/// out.
///
/// One shot per theme, because a theme is the whole of what this window's
/// design is: a look that came out unreadable would be invisible to every
/// geometry test in `tests/notepad.rs`, which knows where the words go and
/// not what colour they are.
fn shoot_notepad(
    theme_name: fontelle_types::NotepadTheme,
) -> Option<(
    Vec<u8>,
    fontelle_ui::canvas::NotepadLayout,
    fontelle_ui::theme::NotepadInk,
    u32,
)> {
    let shared = headless()?;
    let theme = Theme::dark_default();
    let mut text = TextContext::new();

    let page = "when the lights go down\nand the room goes quiet\n\n\
                i can hear the tape run on\nlonger than it should";
    let view = fontelle_ui::canvas::NotepadView {
        track: "Vocal".to_string(),
        theme: theme_name,
        size: fontelle_types::NotepadSize::Medium,
        page: 1,
        pages: 3,
        text: page.to_string(),
        captions: vec![
            Some("when the lights go down".to_string()),
            Some("i can hear the tape run on".to_string()),
            None,
        ],
    };

    let (ew, eh) = fontelle_ui::layout::NOTEPAD_SIZE;
    let panel = fontelle_ui::layout::editor_window_layout(ew as f32, eh as f32, &theme.metrics);
    let text_px = fontelle_ui::canvas::notepad_text_px(view.size, theme.font.size);
    let advance = text
        .layout(
            "0",
            &fontelle_ui::theme::FontTokens {
                family: "monospace".to_string(),
                size: text_px,
                line_height: 1.0,
            },
            None,
        )
        .width
        .max(1.0);
    let layout = fontelle_ui::canvas::notepad_layout(
        panel.body,
        &theme.metrics,
        &view,
        advance,
        (text_px * fontelle_ui::canvas::NOTEPAD_LEADING).round(),
    );
    let rows = fontelle_ui::canvas::notepad_rows(page, layout.columns);
    let ink = fontelle_ui::theme::notepad_ink(view.theme, &theme.palette);

    // The same strings `app::shape_labels` asks for, so what this shot shows
    // is what the window shows.
    let mut labels = Labels::new();
    for row in &rows {
        labels.ensure_mono(&page[row.from..row.to], text_px, &mut text);
    }
    for caption in [
        fontelle_ui::render::NOTEPAD_PREVIOUS,
        fontelle_ui::render::NOTEPAD_NEXT,
        fontelle_ui::render::NOTEPAD_ADD,
        fontelle_ui::render::NOTEPAD_REMOVE,
        fontelle_ui::render::notepad_size_caption(view.size),
        view.theme.label(),
    ] {
        labels.ensure(caption, &theme.font, &mut text);
    }
    labels.ensure(&view.page_label(), &theme.font, &mut text);

    let title = text.layout("Vocal — Notepad", &theme.font, None);
    let mut scene = vello::Scene::new();
    fontelle_ui::render::draw_editor_window(
        &mut scene,
        &theme,
        &panel,
        &labels,
        &title,
        &fontelle_ui::render::EditorWindowChrome::Notepad(fontelle_ui::render::NotepadChrome {
            layout: layout.clone(),
            view: &view,
            ink,
            rows: &rows,
            scroll: 0,
            text_px,
            // Caret in the middle of the second line, as if somebody were
            // typing: the one thing a still picture can say about a cursor.
            caret: Some(30),
            caret_on: true,
            selection: None,
            hover: None,
        }),
        None,
        None,
        None,
        None,
    );
    let pixels = shared
        .lock()
        .expect("the shared renderer")
        .render(&scene, ew, eh, ink.ground)
        .expect("the scene must render");
    dump_sized(&pixels, &format!("notepad-{}", theme_name.label()), ew, eh);
    Some((pixels, layout, ink, ew))
}

#[test]
fn the_notepad_draws_its_page_in_its_own_theme() {
    for theme in fontelle_types::NotepadTheme::ALL {
        let Some((pixels, layout, ink, width)) = shoot_notepad(theme) else {
            return;
        };
        let at = |x: u32, y: u32| {
            let i = ((y * width + x) * 4) as usize;
            Color::rgb(pixels[i], pixels[i + 1], pixels[i + 2])
        };
        // **The pad is painted in its own palette, not the studio's.** The
        // ground under the footer is the theme's, which is the whole claim
        // `Theme::for_notepad` makes.
        let ground = at(
            layout.footer.x as u32 + 4,
            layout.footer.bottom() as u32 - 3,
        );
        assert!(
            near(ground, ink.ground),
            "{}: the ground is {ground:?}, not {:?}",
            theme.label(),
            ink.ground
        );
        // The sheet is a different colour from the ground it sits on, or the
        // page is not a page.
        let sheet = at(
            (layout.sheet.x + layout.sheet.width / 2.0) as u32,
            (layout.sheet.y + layout.sheet.height - 8.0) as u32,
        );
        assert!(
            !near(sheet, ink.ground) || near(sheet, ink.paper),
            "{}: the sheet did not draw",
            theme.label()
        );
        // And there are **words** on it: somewhere along the first line, a
        // pixel that is neither the page nor the caret.
        let first_line = (layout.text.y + layout.line_height / 2.0) as u32;
        let inked = (0..layout.text.width as u32)
            .step_by(2)
            .any(|dx| !near(at(layout.text.x as u32 + dx, first_line), ink.paper));
        assert!(inked, "{}: the first line drew nothing", theme.label());
    }
}

/// DisgustingBeat's window, drawn through the real pipeline
/// (`docs/disgusting-beat-plan.md` §7.6) — `FONTELLE_UI_DUMP=<dir>` writes it
/// out.
///
/// **It is the test that can see.** Everything else about this window is
/// geometry checked in `tests/disgusting_beat.rs`, which knows where the grid is and
/// cannot know whether the curve landed on it.
#[test]
fn disgusting_beat_draws_its_memory_and_its_curves() {
    let Some(shared) = headless() else {
        return;
    };
    let theme = Theme::dark_default();
    let mut text = TextContext::new();

    // A scene worth looking at: a freeze over the second half of the bar, a
    // gate under it, and a memory with something in it.
    let mut lanes: Vec<fontelle_ui::canvas::LaneView> = fontelle_types::DisgustingBeatLaneKind::ALL
        .iter()
        .map(|kind| fontelle_ui::canvas::LaneView {
            kind: *kind,
            length: fontelle_types::DisgustingBeatLength::Bar,
            on: kind.on_by_default(),
            points: vec![fontelle_types::DisgustingBeatPoint::new(
                0.0,
                kind.neutral(),
                fontelle_types::CurveShape::Linear,
            )],
            open: kind.on_by_default(),
        })
        .collect();
    lanes[0].points = vec![
        fontelle_types::DisgustingBeatPoint::new(0.0, 0.0, fontelle_types::CurveShape::Stepped),
        fontelle_types::DisgustingBeatPoint::new(0.5, 0.0, fontelle_types::CurveShape::Linear),
        fontelle_types::DisgustingBeatPoint::new(1.0, -0.5, fontelle_types::CurveShape::Linear),
    ];
    lanes[1].points = vec![
        fontelle_types::DisgustingBeatPoint::new(0.0, 1.0, fontelle_types::CurveShape::Stepped),
        fontelle_types::DisgustingBeatPoint::new(0.25, 0.3, fontelle_types::CurveShape::Stepped),
        // **A vertical**: two points at one phase, silence to full between
        // two samples. In the dump because a picture is the only way to see
        // that the two handles do not land on top of each other and that the
        // line between them is upright.
        fontelle_types::DisgustingBeatPoint::new(0.5, 0.0, fontelle_types::CurveShape::Linear),
        fontelle_types::DisgustingBeatPoint::new(0.5, 1.0, fontelle_types::CurveShape::SCurve),
        fontelle_types::DisgustingBeatPoint::new(0.75, 0.0, fontelle_types::CurveShape::Linear),
    ];
    let mut scene_names: Vec<String> = (0..12).map(|_| String::new()).collect();
    scene_names[0] = "Hold".to_string();
    scene_names[2] = "Roll".to_string();
    let view = fontelle_ui::canvas::DisgustingBeatView {
        track: "Drums".to_string(),
        config: fontelle_types::DisgustingBeatConfig::new(),
        scene: 0,
        scene_names,
        scene_used: (0..12).map(|index| index < 4).collect(),
        lanes,
        phase: 0.62,
        offset: -0.12,
        rate: 0.0,
        clamped: false,
        filled_seconds: 8.0,
        // A plausible drum loop's envelope: a hit every quarter of the ring.
        memory: (0..512)
            .map(|index| {
                let beat = (index % 128) as f32 / 128.0;
                let level = (1.0 - beat * 4.0).max(0.08);
                (level, level * 0.6)
            })
            .collect(),
        // Live until the last quarter of the ring, then a freeze: the read
        // head walks away from the present one bucket per bucket, which is
        // the picture the canopy paints over the memory.
        trail: (0..512)
            .map(|index| {
                if index < 384 {
                    0.0
                } else {
                    (index - 384) as f32
                }
            })
            .collect(),
        beats_per_bar: 4,
        bpm: 120.0,
        tool: fontelle_ui::canvas::DisgustingBeatTool::Hold,
        snap: fontelle_ui::canvas::DisgustingBeatSnap::Sixteenth,
        zoom: 1.0,
        // Open on the point the freeze starts at, so the dump carries the
        // menu too: a picture of a menu is the only way to see that its rows
        // are legible and that it sits over the lane rather than under it.
        menu: Some(fontelle_ui::canvas::DisgustingBeatMenu {
            lane: 0,
            index: 1,
            at: (700.0, 230.0),
        }),
    };

    let (ew, eh) = fontelle_ui::layout::DISGUSTING_BEAT_SIZE;
    let panel = fontelle_ui::layout::editor_window_layout(ew as f32, eh as f32, &theme.metrics);
    let layout = fontelle_ui::canvas::disgusting_beat_layout(&view, panel.body);

    // Under exactly the strings `draw_disgusting_beat` looks up — the same list
    // `shape_labels` builds.
    let mut labels = Labels::default();
    for lane in &view.lanes {
        labels.ensure(lane.kind.label(), &theme.font, &mut text);
    }
    for index in 0..view.scene_names.len() {
        labels.ensure(&view.scene_label(index), &theme.font, &mut text);
    }
    for tool in fontelle_ui::canvas::DisgustingBeatTool::ALL {
        labels.ensure(tool.label(), &theme.font, &mut text);
    }
    for snap in fontelle_ui::canvas::DisgustingBeatSnap::ALL {
        labels.ensure(snap.label(), &theme.font, &mut text);
    }
    for zoom in fontelle_ui::canvas::DisgustingBeatZoom::ALL {
        labels.ensure(zoom.label(), &theme.font, &mut text);
    }
    for lane in &view.lanes {
        labels.ensure(lane.length.label(), &theme.font, &mut text);
    }
    labels.ensure("clear", &theme.font, &mut text);
    if let Some(said) = fontelle_ui::canvas::disgusting_beat_readout(
        &view,
        fontelle_ui::canvas::DisgustingBeatHit::Bend {
            lane: 1,
            carrier: 3,
        },
    ) {
        labels.ensure(&said, &theme.font, &mut text);
    }
    for row in 0..fontelle_ui::canvas::DISGUSTING_BEAT_MENU_ROWS {
        labels.ensure(
            fontelle_ui::canvas::disgusting_beat_menu_label(row),
            &theme.font,
            &mut text,
        );
    }
    labels.ensure(
        &format!(
            "memory {:.1}s  \u{b7}  {}",
            view.filled_seconds,
            view.rate_caption()
        ),
        &theme.font,
        &mut text,
    );
    let config = fontelle_types::EffectConfig::DisgustingBeat(view.config);
    for spec in config.specs() {
        labels.ensure(spec.name, &theme.font, &mut text);
        let value = config.get(spec.id).unwrap_or(spec.default);
        labels.ensure(
            &fontelle_ui::canvas::display_of(spec, value),
            &theme.font,
            &mut text,
        );
    }

    let title = text.layout("Drums — DisgustingBeat", &theme.font, None);
    let mut scene = vello::Scene::new();
    fontelle_ui::render::draw_editor_window(
        &mut scene,
        &theme,
        &panel,
        &labels,
        &title,
        &fontelle_ui::render::EditorWindowChrome::DisgustingBeat(
            fontelle_ui::render::DisgustingBeatChrome {
                layout: layout.clone(),
                view: &view,
                // Over the grip on the volume lane's last segment, so the
                // dump carries a lit handle and the read-out it puts in the
                // canopy.
                hover: Some(fontelle_ui::canvas::DisgustingBeatHit::Bend {
                    lane: 1,
                    carrier: 3,
                }),
                renaming: None,
            },
        ),
        None,
        None,
        None,
        None,
    );
    let pixels = shared
        .lock()
        .expect("the shared renderer")
        .render(&scene, ew, eh, theme.palette.window)
        .expect("the scene must render");
    dump_sized(&pixels, "disgusting_beat", ew, eh);

    // The curve is drawn in the accent ink, so the time lane has some of it
    // in the *lower* half — which is where a freeze goes and nowhere else
    // would be.
    let lane = layout.lanes[0];
    let mut accent_below = 0usize;
    for y in (lane.y + lane.height * 0.30) as u32..(lane.bottom() - 2.0) as u32 {
        for x in (lane.x + 2.0) as u32..(lane.right() - 2.0) as u32 {
            let at = ((y * ew + x) * 4) as usize;
            let (r, g, b) = (pixels[at], pixels[at + 1], pixels[at + 2]);
            // Anything appreciably brighter than the lane's own ground.
            if r as u16 + g as u16 + b as u16 > 180 {
                accent_below += 1;
            }
        }
    }
    assert!(
        accent_below > 200,
        "the freeze should be drawn across the bottom half of the time lane, found {accent_below} lit pixels"
    );
}

// --- the save notices and the job card ------------------------------------
//
// > *"the program freezes during actions instead of showing progress bars ...
// > when i save it shows a Saved! text that appears in the top center and
// > moves upwards as it fades out."*

#[test]
fn a_job_card_a_save_prompt_and_saved_are_drawn_where_their_layouts_say() {
    use fontelle_ui::canvas::{
        SAVE_PROMPT_DISCARD, SAVE_PROMPT_SAVE, SAVED_FLASH_TEXT, job_card_layout,
        save_prompt_layout, saved_flash,
    };
    use fontelle_ui::render::{CONFIRM_CANCEL, Notices};

    let Some(shared) = headless() else {
        return;
    };
    let theme = Theme::dark_default();
    let layout = window_layout(W as f32, H as f32, &theme.metrics, DEFAULT_TIMELINE_HEIGHT);
    let mut text = TextContext::new();
    let title = text.layout("Song*", &theme.font, None);
    let readout = text.layout("1.1.000", &theme.font, None);
    let question = "Save changes to Song before closing?";
    let job = "Exporting Song.wav";
    let mut labels = Labels::new();
    for word in [
        question,
        job,
        SAVED_FLASH_TEXT,
        SAVE_PROMPT_SAVE,
        SAVE_PROMPT_DISCARD,
        CONFIRM_CANCEL,
    ] {
        labels.ensure(word, &theme.font, &mut text);
    }
    let bar = transport_bar_layout(layout.transport, &theme.metrics);
    let flash = saved_flash(layout.window, &theme.metrics, 0.0).expect("up at once");

    let shoot = |notices: Notices<'_>| {
        let mut scene = vello::Scene::new();
        draw_window(
            &mut scene,
            &theme,
            &layout,
            &Chrome {
                field: None,
                panel_title: &title,
                transport: TransportChrome {
                    layout: bar,
                    view: TransportView::unavailable(),
                    meters: [Meter::new(); 2],
                    readout: &readout,
                    tempo: &readout,
                    signature: &readout,
                    mode: &readout,
                    hover: None,
                    marker_sample: 0,
                    clip_mode: false,
                    tempo_field: None,
                },
                roll: None,
                rack: None,
                prefabs: None,
                browser: None,
                timeline: None,
                mixer: None,
                tabs: fontelle_ui::layout::editor_tabs(layout.panel.header, &theme.metrics),
                tab: fontelle_ui::layout::EditorTab::Roll,
                hover_tab: None,
                browser_title: "Soundfonts",
                labels: &labels,
                status: "",
                toast: None,
                confirm: None,
                notices,
                tooltip: None,
                menu: None,
                carry: None,
                welcome: None,
                keybinds: None,
            },
        );
        shared
            .lock()
            .expect("the shared renderer")
            .render(&scene, W, H, theme.palette.window)
            .expect("rendering")
    };
    let at = |pixels: &[u8], x: f32, y: f32| {
        let i = ((y as u32 * W + x as u32) * 4) as usize;
        Color([pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]])
    };

    // The job card, half done: its bar is lit on the left and not the right.
    let card = shoot(Notices {
        job: Some((job, Some(0.5))),
        saved: Some(flash),
        save_prompt: None,
    });
    dump_sized(&card, "notices-job-and-saved", W, H);
    let l = job_card_layout(layout.window, &theme.metrics);
    let mid = l.bar.y + l.bar.height / 2.0;
    assert!(
        near(
            at(&card, l.bar.x + l.bar.width * 0.2, mid),
            theme.palette.accent
        ),
        "the done half of the bar is not lit"
    );
    assert!(
        !near(
            at(&card, l.bar.x + l.bar.width * 0.8, mid),
            theme.palette.accent
        ),
        "the undone half of the bar is lit"
    );
    // "Saved!" is ink at the top centre: something other than what is under
    // it, somewhere in a box around its centre.
    let bare = shoot(Notices::default());
    let lit = (-40..40)
        .flat_map(|dx| (-8..8).map(move |dy| (dx, dy)))
        .filter(|(dx, dy)| {
            let (x, y) = (flash.center_x + *dx as f32, flash.center_y + *dy as f32);
            at(&card, x, y) != at(&bare, x, y)
        })
        .count();
    assert!(
        lit > 20,
        "Saved! is not drawn at the top centre ({lit} pixels)"
    );

    // The save prompt: a scrim, and Save in the accent.
    let prompt = shoot(Notices {
        job: None,
        saved: None,
        save_prompt: Some(question),
    });
    dump_sized(&prompt, "notices-save-prompt", W, H);
    let l = save_prompt_layout(layout.window, &theme.metrics);
    assert!(
        near(
            at(&prompt, l.save.x + 3.0, l.save.y + 3.0),
            theme.palette.accent
        ),
        "Save is not the weighted button"
    );
    assert!(
        at(&prompt, 2.0, H as f32 - 2.0) != at(&bare, 2.0, H as f32 - 2.0),
        "no scrim over the window"
    );
}

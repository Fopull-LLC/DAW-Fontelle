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
use fontelle_ui::canvas::{RollView, roll_layout, tick_to_x};
use fontelle_ui::layout::{Rect, window_layout};
use fontelle_ui::render::RollChrome;
use fontelle_ui::render::{Chrome, Headless, TransportChrome, draw_window};
use fontelle_ui::text::TextContext;
use fontelle_ui::theme::{Color, Theme};
use fontelle_ui::transport::{
    Meter, TransportBarLayout, TransportView, format_readout, playhead_x, transport_bar_layout,
};

use std::sync::{Mutex, OnceLock};

const W: u32 = 640;
const H: u32 = 360;

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
    let layout = window_layout(W as f32, H as f32, &theme.metrics);
    let mut text = TextContext::new();
    let title = text.layout("Fontelle", &theme.font, None);

    let bar = transport_bar_layout(layout.transport, &theme.metrics);
    let readout = text.layout(&format_readout(&view, 4), &theme.font, None);

    let mut scene = vello::Scene::new();
    draw_window(
        &mut scene,
        &theme,
        &layout,
        &Chrome {
            panel_title: &title,
            transport: TransportChrome {
                layout: bar,
                view,
                meters,
                readout: &readout,
                hover: None,
            },
            roll: None,
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
    let Ok(dir) = std::env::var("FONTELLE_UI_DUMP") else {
        return;
    };
    let path = std::path::Path::new(&dir).join(format!("{}.png", name.replace(' ', "-")));
    let file = std::fs::File::create(&path).expect("somewhere to write the frame");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), W, H);
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
    }
}

struct RollShot {
    pixels: Vec<u8>,
    theme: Theme,
    layout: fontelle_ui::canvas::RollLayout,
    view: RollView,
}

impl RollShot {
    fn at(&self, x: u32, y: u32) -> Color {
        let i = ((y * W + x) * 4) as usize;
        Color(self.pixels[i..i + 4].try_into().expect("four bytes"))
    }
}

/// Renders a window whose panel is a piano roll holding `notes`.
fn shoot_roll(notes: &Arena<NoteId, Note>, selection: &[NoteId]) -> Option<RollShot> {
    let theme = Theme::dark_default();
    let shared = headless()?;
    let layout = window_layout(W as f32, H as f32, &theme.metrics);
    let mut text = TextContext::new();
    let title = text.layout("Roll", &theme.font, None);
    let view = TransportView::unavailable();
    let readout = text.layout(&format_readout(&view, 4), &theme.font, None);
    let roll_l = roll_layout(layout.panel.body, &theme.metrics);
    let roll_view = RollView {
        top_key: 72,
        ..RollView::default()
    };

    let mut scene = vello::Scene::new();
    draw_window(
        &mut scene,
        &theme,
        &layout,
        &Chrome {
            panel_title: &title,
            transport: TransportChrome {
                layout: transport_bar_layout(layout.transport, &theme.metrics),
                view,
                meters: [Meter::new(); 2],
                readout: &readout,
                hover: None,
            },
            roll: Some(RollChrome {
                layout: roll_l,
                view: roll_view,
                notes,
                selection,
                playhead_tick: None,
                beats_per_bar: 4,
            }),
        },
    );
    let pixels = shared
        .lock()
        .expect("the shared renderer")
        .render(&scene, W, H, theme.palette.window)
        .expect("rendering a scene that fits in memory");

    Some(RollShot {
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

//! Renders a theme's backdrops to PNGs, to look at them, and times them.
//!
//! ```text
//! cargo run --release -p fontelle-ui --example backdrop_probe -- <theme> [out-dir] [seconds…]
//! cargo run --release -p fontelle-ui --example backdrop_probe -- <theme> --time [width] [height]
//! ```
//!
//! `<theme>` is a built-in's name ("Lo-fi Rain") or a `.fontelletheme`
//! file. The first form writes one frame per time given (default 2 and 90
//! seconds — a frame early and one late in its loop, hub card 0366's rule),
//! at 1280×720: the window's backdrop with every section's panel laid over
//! it in the theme's own see-through inks, so the picture is what the
//! window shows behind its words. The second draws the backdrops 300 times
//! at the window size given (default 1920×1080, at the default half
//! resolution) and says what a draw costs, and what a whole frame of the
//! scene costs vello.
//!
//! The twin of Floptle's `floptle-theme/examples/backdrop_probe.rs`.

use std::time::Instant;

use fontelle_ui::backdrop::BackdropFrame;
use fontelle_ui::layout::{DEFAULT_TIMELINE_HEIGHT, Rect, window_layout};
use fontelle_ui::render::{Headless, backdrop_uses, decode_backdrops, draw_section};
use fontelle_ui::theme::{BackdropPanel, Theme};
use vello::kurbo::{Affine, RoundedRect};
use vello::peniko::Fill;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let what = args.first().cloned().unwrap_or_else(|| "Stage".to_string());
    let theme = if what.ends_with(".fontelletheme") {
        Theme::load_from_file(std::path::Path::new(&what)).unwrap_or_else(|e| panic!("{e}"))
    } else {
        Theme::builtins()
            .into_iter()
            .find(|t| t.name == what)
            .unwrap_or_else(|| panic!("no built-in look called {what:?}"))
    };
    for (panel, why) in theme.shader_problems() {
        eprintln!("{}: {why}", panel.label());
    }
    let mut headless = Headless::new().expect("a GPU");
    if args.get(1).map(String::as_str) == Some("--time") {
        let w: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1920);
        let h: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1080);
        time(&mut headless, &theme, w, h);
        return;
    }
    let out = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "target/backdrop_probe".to_string());
    let times: Vec<f32> = args.iter().skip(2).filter_map(|s| s.parse().ok()).collect();
    let times = if times.is_empty() {
        vec![2.0, 90.0]
    } else {
        times
    };
    std::fs::create_dir_all(&out).unwrap();
    for t in times {
        let (w, h) = (1280, 720);
        let pixels = frame(&mut headless, &theme, w, h, t);
        let path = std::path::Path::new(&out)
            .join(format!("{}-{t:05.1}s.png", theme.name.replace(' ', "-")));
        write_png(&path, &pixels, w, h);
        eprintln!("wrote {}", path.display());
    }
}

/// The window's backdrop with the studio's sections over it, at `t` seconds.
fn frame(headless: &mut Headless, theme: &Theme, w: u32, h: u32, t: f32) -> Vec<u8> {
    let layout = window_layout(w as f32, h as f32, &theme.metrics, DEFAULT_TIMELINE_HEIGHT);
    let uses = backdrop_uses(&layout, true, false, fontelle_ui::layout::EditorTab::Roll);
    let report = headless.prepare_backdrops(
        theme,
        &uses,
        w,
        h,
        1.0,
        0.5,
        &BackdropFrame {
            time: t,
            pointer: [0.5, 0.5, 0.0, 0.0],
            beat: t * 2.0,
            level: 0.6,
            redraw: true,
        },
    );
    for (shader, why) in &report.errors {
        eprintln!("{shader}: {why}");
    }
    let mut backdrops = decode_backdrops(theme);
    backdrops.shaders = headless.shader_frames();
    let mut scene = vello::Scene::new();
    let p = &theme.palette;
    let window = layout.window;
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        p.window.with_alpha(0xff).to_peniko(),
        None,
        &kurbo(window, 0.0),
    );
    draw_section(
        &mut scene,
        window,
        0.0,
        window,
        &backdrops,
        BackdropPanel::Window,
    );
    let r = theme.metrics.corner_radius;
    for (panel, area) in uses {
        if panel == BackdropPanel::Window {
            continue;
        }
        let ground = if panel == BackdropPanel::Transport {
            p.panel_header
        } else {
            p.panel
        };
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            ground.to_peniko(),
            None,
            &kurbo(area, r),
        );
        draw_section(&mut scene, area, r, window, &backdrops, panel);
        if theme.metrics.border_width > 0.0 {
            scene.stroke(
                &vello::kurbo::Stroke::new(theme.metrics.border_width as f64),
                Affine::IDENTITY,
                p.border.to_peniko(),
                None,
                &kurbo(area, r),
            );
        }
    }
    headless.render(&scene, w, h, p.window).unwrap()
}

fn time(headless: &mut Headless, theme: &Theme, w: u32, h: u32) {
    let layout = window_layout(w as f32, h as f32, &theme.metrics, DEFAULT_TIMELINE_HEIGHT);
    let uses = backdrop_uses(&layout, true, false, fontelle_ui::layout::EditorTab::Roll);
    let frame_at = |t: f32| BackdropFrame {
        time: t,
        pointer: [0.5, 0.5, 1.0, 0.0],
        beat: t * 2.0,
        level: 0.5,
        redraw: true,
    };
    // Warm: the pipeline compiles on the first draw.
    headless.prepare_backdrops(theme, &uses, w, h, 1.0, 0.5, &frame_at(0.0));
    let _ = headless.render(&vello::Scene::new(), 4, 4, theme.palette.window);
    const N: u32 = 300;
    let started = Instant::now();
    let mut drawn = 0;
    for i in 0..N {
        drawn += headless
            .prepare_backdrops(theme, &uses, w, h, 1.0, 0.5, &frame_at(i as f32 / 30.0))
            .drawn;
    }
    // A render waits for everything submitted before it: the draws are done.
    let _ = headless.render(&vello::Scene::new(), 4, 4, theme.palette.window);
    let per = started.elapsed().as_secs_f64() * 1000.0 / N as f64;
    println!(
        "{}: {drawn} shader draws over {N} frames at {w}x{h} (half resolution): {per:.3} ms a frame, submit to GPU done",
        theme.name
    );
    // The scene the window would draw over them, through vello, whole.
    let pixels = frame(headless, theme, w, h, 1.0);
    drop(pixels);
    let started = Instant::now();
    for _ in 0..30 {
        let _ = frame(headless, theme, w, h, 1.0);
    }
    let per = started.elapsed().as_secs_f64() * 1000.0 / 30.0;
    println!(
        "{}: a whole {w}x{h} frame of backdrops and panels, drawn and read back: {per:.2} ms",
        theme.name
    );
}

fn kurbo(r: Rect, radius: f32) -> RoundedRect {
    RoundedRect::new(
        r.x as f64,
        r.y as f64,
        r.right() as f64,
        r.bottom() as f64,
        radius as f64,
    )
}

fn write_png(path: &std::path::Path, pixels: &[u8], w: u32, h: u32) {
    let file = std::fs::File::create(path).unwrap();
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(pixels)
        .unwrap();
}

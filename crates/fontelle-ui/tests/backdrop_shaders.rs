//! Moving backdrops: the WGSL contract a theme's shader is written against,
//! when the window lets one move, and the pixels it draws.
//!
//! Hub card 0366. The contract is Floptle's (`floptle-theme/src/backdrop.rs`)
//! field for field, so one shader runs in both programs; Fontelle's own
//! uniforms come after the shared ones. The hard rule is the audio's: none of
//! this runs anywhere but the window's own thread, it caps its own rate, and
//! it stops when nobody can see it.

use std::time::{Duration, Instant};

use fontelle_ui::backdrop::{
    BUILTIN_SHADERS, BackdropClock, BackdropFrame, Effects, Motion, PRELUDE, WindowSight,
    backdrop_wake, builtin_source, song_beat, validate,
};
use fontelle_ui::theme::{BackdropPanel, Layer, ShaderLayer, Theme};

// ------------------------------------------------------------ contract ---

#[test]
fn floptles_built_in_shaders_are_all_here_by_the_same_names() {
    for name in [
        "galaxy",
        "aurora",
        "grid",
        "scanlines",
        "drift",
        "waves",
        "starfield",
    ] {
        assert!(
            builtin_source(&format!("builtin:{name}")).is_some(),
            "builtin:{name}"
        );
    }
    assert!(builtin_source("galaxy").is_none(), "only by builtin:");
    assert!(builtin_source("builtin:nope").is_none());
}

#[test]
fn every_built_in_shader_compiles() {
    for (name, body) in BUILTIN_SHADERS {
        if let Err(why) = validate(body) {
            panic!("builtin:{name} does not compile:\n{why}");
        }
    }
}

/// The shared fields in Floptle's order, then Fontelle's. A shader written
/// for Floptle reads the same offsets here.
#[test]
fn the_uniform_block_starts_with_floptles_fields_in_floptles_order() {
    let shared = [
        "resolution: vec2<f32>",
        "window: vec2<f32>",
        "time: f32",
        "scale: f32",
        "density: f32",
        "_pad0: f32",
        "color0: vec4<f32>",
        "color1: vec4<f32>",
        "color2: vec4<f32>",
        "color3: vec4<f32>",
        "params0: vec4<f32>",
        "params1: vec4<f32>",
        "pointer: vec4<f32>",
        // Fontelle's, after.
        "beat: f32",
        "level: f32",
    ];
    let mut from = 0;
    for field in shared {
        let at = PRELUDE[from..]
            .find(field)
            .unwrap_or_else(|| panic!("{field} missing or out of order"));
        from += at + field.len();
    }
    for helper in ["fn bd_hash", "fn bd_noise", "fn bd_fbm", "fn bd_rot"] {
        assert!(PRELUDE.contains(helper), "{helper}");
    }
    for binding in [
        "var<uniform> bd: Backdrop",
        "var bd_image: texture_2d<f32>",
        "var bd_sampler: sampler",
    ] {
        assert!(PRELUDE.contains(binding), "{binding}");
    }
}

#[test]
fn a_shader_written_for_floptle_compiles_here_unchanged() {
    let floptle = "fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {\n\
        let wave = 0.5 + 0.5 * sin(uv.x * 8.0 + bd.time);\n\
        let t = textureSample(bd_image, bd_sampler, uv);\n\
        return vec4<f32>(mix(bd.color2.rgb, bd.color0.rgb, wave * 0.3) * t.rgb\n\
            + bd_fbm(px * 0.01, 4) * bd.params0.x * bd.pointer.z, 1.0);\n}\n";
    validate(floptle).expect("compiles");
    // And Fontelle's own two numbers are there to read.
    validate(
        "fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {\n\
         return vec4<f32>(fract(bd.beat), bd.level, 0.0, 1.0);\n}\n",
    )
    .expect("beat and level");
}

#[test]
fn a_broken_shader_is_refused_with_nagas_message() {
    let why = validate("fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> { return oops; }")
        .expect_err("does not compile");
    assert!(why.contains("oops"), "names what is wrong: {why}");
    let why = validate("fn something_else() {}").expect_err("no backdrop()");
    assert!(why.contains("fn backdrop"), "{why}");
}

#[test]
fn a_theme_says_which_of_its_shaders_do_not_compile() {
    let mut theme = Theme::dark_default();
    theme
        .backdrops
        .layers_mut(BackdropPanel::Mixer)
        .push(Layer::Shader(ShaderLayer::new("fn backdrop(")));
    theme
        .backdrops
        .layers_mut(BackdropPanel::Window)
        .push(Layer::Shader(ShaderLayer::new("builtin:drift")));
    theme
        .backdrops
        .layers_mut(BackdropPanel::Roll)
        .push(Layer::Shader(ShaderLayer::new("builtin:not-a-shader")));
    let problems = theme.shader_problems();
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(
        problems
            .iter()
            .any(|(panel, _)| *panel == BackdropPanel::Mixer)
    );
    let (_, roll) = problems
        .iter()
        .find(|(panel, _)| *panel == BackdropPanel::Roll)
        .unwrap();
    assert!(roll.contains("not-a-shader"), "{roll}");
}

#[test]
fn every_built_in_theme_s_shaders_compile() {
    for theme in Theme::builtins() {
        assert!(
            theme.shader_problems().is_empty(),
            "{}: {:?}",
            theme.name,
            theme.shader_problems()
        );
    }
}

// --------------------------------------------------- when it may move ---

fn seen() -> WindowSight {
    WindowSight {
        focused: true,
        occluded: false,
        minimized: false,
    }
}

#[test]
fn a_moving_theme_wakes_the_window_at_its_own_rate_and_no_faster() {
    let motion = Motion::default();
    assert_eq!(motion.effects, Effects::Moving);
    assert_eq!(motion.fps, 30.0);
    assert_eq!(motion.scale, 0.5);
    let wake = backdrop_wake(&motion, true, seen(), false).expect("it moves");
    assert_eq!(wake, Duration::from_secs_f32(1.0 / 30.0));
    let fast = Motion {
        fps: 500.0,
        ..Motion::default()
    };
    assert_eq!(
        backdrop_wake(&fast, true, seen(), false),
        Some(Duration::from_secs_f32(1.0 / 60.0)),
        "capped at 60: the surface does not wait for vsync"
    );
    let slow = Motion {
        fps: 0.0,
        ..Motion::default()
    };
    assert_eq!(
        backdrop_wake(&slow, true, seen(), false),
        Some(Duration::from_secs_f32(1.0 / 5.0))
    );
}

/// Ty, watching the first build: *"im not seeing any moving animations ...
/// please make sure these can be fully animated themes"*. A studio is
/// looked at while a plugin's window or another program has the focus, so
/// losing focus does not stop it — Floptle's editor does not either. What
/// nobody can see does stop.
#[test]
fn a_window_that_can_be_seen_keeps_moving_without_the_focus() {
    let unfocused = WindowSight {
        focused: false,
        ..seen()
    };
    assert!(backdrop_wake(&Motion::default(), true, unfocused, false).is_some());
}

#[test]
fn nothing_holds_the_window_awake_when_nobody_can_see_it() {
    let motion = Motion::default();
    let occluded = WindowSight {
        occluded: true,
        ..seen()
    };
    let minimized = WindowSight {
        minimized: true,
        ..seen()
    };
    for sight in [occluded, minimized] {
        assert_eq!(
            backdrop_wake(&motion, true, sight, false),
            None,
            "{sight:?}"
        );
    }
}

#[test]
fn still_and_off_never_hold_the_window_awake() {
    for effects in [Effects::Still, Effects::Off] {
        let motion = Motion {
            effects,
            ..Motion::default()
        };
        assert_eq!(backdrop_wake(&motion, true, seen(), false), None);
    }
    assert_eq!(
        backdrop_wake(&Motion::default(), false, seen(), false),
        None,
        "a theme with nothing moving never wakes the window"
    );
}

#[test]
fn hold_still_while_playing_holds_only_while_playing() {
    let held = Motion {
        hold_while_playing: true,
        ..Motion::default()
    };
    assert_eq!(backdrop_wake(&held, true, seen(), true), None);
    assert!(backdrop_wake(&held, true, seen(), false).is_some());
    assert!(
        backdrop_wake(&Motion::default(), true, seen(), true).is_some(),
        "off by default: the music is the point"
    );
    assert!(!Motion::default().hold_while_playing);
}

#[test]
fn the_clock_runs_only_while_moving_so_a_resume_does_not_jump() {
    let start = Instant::now();
    let mut clock = BackdropClock::default();
    assert_eq!(clock.advance(start, true), 0.0);
    let t = clock.advance(start + Duration::from_millis(100), true);
    assert!((t - 0.1).abs() < 1e-4, "{t}");
    // Ten seconds held still: the clock does not move.
    let held = clock.advance(start + Duration::from_secs(10), false);
    assert!((held - 0.1).abs() < 1e-4, "{held}");
    // And picks up from there, a frame later, not ten seconds later.
    let resumed = clock.advance(start + Duration::from_millis(10_050), true);
    assert!(resumed - 0.1 < 0.3, "{resumed}");
}

#[test]
fn the_clock_wraps_within_the_hour_so_a_long_session_keeps_its_precision() {
    let start = Instant::now();
    let mut clock = BackdropClock::default();
    clock.advance(start, true);
    let mut now = start;
    // A day of frames, each well under the per-frame cap.
    for _ in 0..(24 * 3600 * 5) {
        now += Duration::from_millis(200);
        clock.advance(now, true);
    }
    assert!(clock.time() < 3600.0, "{}", clock.time());
}

#[test]
fn the_beat_follows_the_song_while_it_plays_and_a_free_clock_otherwise() {
    assert_eq!(song_beat(true, Some(17.25), 3.0), 17.25);
    // 120 BPM: two beats a second.
    assert_eq!(song_beat(false, Some(17.25), 3.0), 6.0);
    assert_eq!(song_beat(true, None, 1.5), 3.0);
}

// ------------------------------------------------------------- pixels ---

use fontelle_ui::layout::Rect;
use fontelle_ui::render::{Headless, ShaderFrames};

fn headless() -> Option<std::sync::MutexGuard<'static, Headless>> {
    if cfg!(windows) && std::env::var_os("CI").is_some() {
        eprintln!("skipping: no GPU on this runner");
        return None;
    }
    static SHARED: std::sync::OnceLock<Option<std::sync::Mutex<Headless>>> =
        std::sync::OnceLock::new();
    SHARED
        .get_or_init(|| match Headless::new() {
            Ok(h) => Some(std::sync::Mutex::new(h)),
            Err(e) => {
                eprintln!("skipping: no usable GPU adapter ({e})");
                None
            }
        })
        .as_ref()
        .map(|m| m.lock().unwrap_or_else(|e| e.into_inner()))
}

const W: u32 = 320;
const H: u32 = 180;

fn window() -> Rect {
    Rect::new(0.0, 0.0, W as f32, H as f32)
}

fn frame(time: f32) -> BackdropFrame {
    BackdropFrame {
        time,
        pointer: [0.5, 0.5, 0.0, 0.0],
        beat: time * 2.0,
        level: 0.5,
        redraw: true,
    }
}

/// The theme's window section drawn with whatever shader frames were made,
/// as the window draws it: the same `draw_backdrop` path the panels take.
fn shoot(h: &mut Headless, theme: &Theme, frames: ShaderFrames) -> Vec<u8> {
    let mut scene = vello::Scene::new();
    let mut backdrops = fontelle_ui::render::decode_backdrops(theme);
    backdrops.shaders = frames;
    fontelle_ui::render::draw_section(
        &mut scene,
        window(),
        0.0,
        window(),
        &backdrops,
        BackdropPanel::Window,
    );
    h.render(&scene, W, H, theme.palette.window).unwrap()
}

fn shader_theme(source: &str) -> Theme {
    let mut theme = Theme::dark_default();
    theme
        .backdrops
        .layers_mut(BackdropPanel::Window)
        .push(Layer::Shader(ShaderLayer::new(source)));
    theme
}

fn uses() -> Vec<(BackdropPanel, Rect)> {
    vec![(BackdropPanel::Window, window())]
}

#[test]
fn a_shader_layer_draws_its_picture_behind_the_section() {
    let Some(mut h) = headless() else { return };
    let red = shader_theme(
        "fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {\n\
         return vec4<f32>(1.0, 0.0, 0.0, 1.0);\n}\n",
    );
    let report = h.prepare_backdrops(&red, &uses(), W, H, 1.0, 1.0, &frame(0.0));
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(report.drawn, 1);
    let frames = h.shader_frames();
    let pixels = shoot(&mut h, &red, frames);
    let mid = ((H / 2 * W + W / 2) * 4) as usize;
    assert_eq!(
        &pixels[mid..mid + 3],
        &[255, 0, 0],
        "red, straight from the shader"
    );
}

#[test]
fn the_picture_spans_the_window_so_two_sections_read_as_one_scene() {
    let Some(mut h) = headless() else { return };
    // Left half black, right half white, by uv across the whole window.
    let split = shader_theme(
        "fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {\n\
         return vec4<f32>(vec3<f32>(step(0.5, uv.x)), 1.0);\n}\n",
    );
    h.prepare_backdrops(&split, &uses(), W, H, 1.0, 1.0, &frame(0.0));
    let frames = h.shader_frames();
    let pixels = shoot(&mut h, &split, frames);
    let at = |x: u32| pixels[((H / 2 * W + x) * 4) as usize];
    assert!(at(W / 4) < 16, "left is dark");
    assert!(at(3 * W / 4) > 240, "right is light");
}

#[test]
fn the_picture_moves_with_time() {
    let Some(mut h) = headless() else { return };
    let theme = shader_theme(
        "fn backdrop(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {\n\
         return vec4<f32>(vec3<f32>(fract(bd.time * 0.25)), 1.0);\n}\n",
    );
    h.prepare_backdrops(&theme, &uses(), W, H, 1.0, 1.0, &frame(0.4));
    let frames = h.shader_frames();
    let early = shoot(&mut h, &theme, frames);
    h.prepare_backdrops(&theme, &uses(), W, H, 1.0, 1.0, &frame(2.4));
    let frames = h.shader_frames();
    let late = shoot(&mut h, &theme, frames);
    assert_ne!(early, late, "two times, two pictures");
}

#[test]
fn a_broken_shader_leaves_its_section_its_plain_colour_and_says_why() {
    let Some(mut h) = headless() else { return };
    let broken = shader_theme("fn backdrop(uv: vec2<f32>) -> f32 { return nope; }");
    let report = h.prepare_backdrops(&broken, &uses(), W, H, 1.0, 1.0, &frame(0.0));
    assert_eq!(report.drawn, 0);
    assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
    assert!(report.errors[0].1.contains("nope"), "{:?}", report.errors);
    let frames = h.shader_frames();
    let pixels = shoot(&mut h, &broken, frames);
    let plain = shoot(&mut h, &Theme::dark_default(), ShaderFrames::default());
    assert_eq!(pixels, plain, "the panel shows its colour");
    // And the renderer carries on: a good theme after it still draws.
    let good = shader_theme("builtin:drift");
    let report = h.prepare_backdrops(&good, &uses(), W, H, 1.0, 1.0, &frame(0.0));
    assert!(report.errors.is_empty() && report.drawn == 1);
}

#[test]
fn a_frame_not_asked_for_draws_nothing_new() {
    let Some(mut h) = headless() else { return };
    let theme = shader_theme("builtin:drift");
    let first = h.prepare_backdrops(&theme, &uses(), W, H, 1.0, 1.0, &frame(0.0));
    assert_eq!(first.drawn, 1, "a new picture is always drawn once");
    let held = BackdropFrame {
        redraw: false,
        ..frame(5.0)
    };
    let again = h.prepare_backdrops(&theme, &uses(), W, H, 1.0, 1.0, &held);
    assert_eq!(again.drawn, 0, "Still: drawn once and held");
    // Nothing on screen uses it: nothing is drawn, however due.
    let none = h.prepare_backdrops(&theme, &[], W, H, 1.0, 1.0, &frame(6.0));
    assert_eq!(none.drawn, 0);
}

#[test]
fn every_built_in_shader_draws_without_an_error() {
    let Some(mut h) = headless() else { return };
    for (name, _) in BUILTIN_SHADERS {
        let theme = shader_theme(&format!("builtin:{name}"));
        let report = h.prepare_backdrops(&theme, &uses(), W, H, 1.0, 0.5, &frame(12.0));
        assert!(
            report.errors.is_empty(),
            "builtin:{name}: {:?}",
            report.errors
        );
        assert_eq!(report.drawn, 1, "builtin:{name}");
    }
}

/// Ty: *"keep the backgrounds moving when the window isnt focused, but make
/// it a configurable option for users according to their preference."*
#[test]
fn moving_without_the_focus_is_a_choice_on_by_default() {
    let unfocused = WindowSight {
        focused: false,
        ..seen()
    };
    assert!(Motion::default().when_unfocused);
    let only_focused = Motion {
        when_unfocused: false,
        ..Motion::default()
    };
    assert_eq!(backdrop_wake(&only_focused, true, unfocused, false), None);
    assert!(backdrop_wake(&only_focused, true, seen(), false).is_some());
}

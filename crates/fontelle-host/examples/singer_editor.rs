//! Opens an ABI 2 singer engine's own window on a real display, the way the
//! studio embeds it, and says what it asked for.
//!
//! ```text
//! cargo run -p fontelle-host --example singer_editor -- <singers folder> "<engine name>"
//! ```
//!
//! `PROBE_DUMP=<file.png>` writes what the window shows after a second and
//! a half. The part it is shown is one short line, the second note's lyric
//! `!poke` (the test singer's lever), so an engine that edits shows its
//! batch here.

use std::collections::BTreeMap;
use std::path::PathBuf;

use fontelle_sing::{PartView, PhraseNote, ViewNote};

fn main() {
    let mut args = std::env::args().skip(1);
    let folder = PathBuf::from(args.next().expect("a singers folder"));
    let name = args.next().expect("an engine's name");
    let engines = fontelle_host::SingerEngines::load(&[folder]);
    for failure in &engines.failures {
        println!("not loaded: {}: {}", failure.path.display(), failure.why);
    }
    let engine = engines
        .engine(&name)
        .expect("that engine is in the folder")
        .clone();
    println!(
        "{} {} (ABI {}), window: {}",
        engine.name(),
        engine.version(),
        engine.abi_version(),
        engine.has_editor()
    );
    let mut window = fontelle_host::PluginWindow::open(&name, fontelle_host::GuiSize::FALLBACK)
        .expect("a window (is DISPLAY set?)");
    let mut editor = fontelle_host::SingerEditor::open(&engine, window.id()).expect("its window");
    let (w, h) = editor.size();
    println!("it asked for {w}x{h}");
    window.resize(fontelle_host::GuiSize {
        width: w,
        height: h,
    });
    let voice = engine
        .singers()
        .first()
        .map(|s| s.id.clone())
        .unwrap_or_default();
    let view = PartView {
        track: "Voice".into(),
        voice,
        language: "en".into(),
        notes: ["la", "!poke", "lo"]
            .iter()
            .enumerate()
            .map(|(i, l)| ViewNote {
                id: i as u64 + 1,
                note: PhraseNote::new(i as f64 * 0.5, 0.5, 62.0 + i as f32 * 2.0, *l),
                sung: Vec::new(),
            })
            .collect(),
        curves: BTreeMap::new(),
        extra_curves: BTreeMap::new(),
        settings: BTreeMap::new(),
        state: None,
        selection: vec![2],
        playhead: 0.0,
        playing: false,
    };
    editor.set_part(&view);
    let until = std::time::Instant::now() + std::time::Duration::from_millis(1500);
    while std::time::Instant::now() < until {
        let asked = editor.tick();
        for batch in &asked.edits {
            println!("edit batch: {batch:?}");
        }
        for t in &asked.transport {
            println!("transport: {t:?}");
        }
        let polled = window.poll();
        if let Some(size) = polled.resized {
            editor.resized(size.width, size.height);
        }
        if polled.closed {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
    if let Some(dump) = std::env::var_os("PROBE_DUMP")
        && let Some((w, h, rgba)) = window.grab()
    {
        let file = std::fs::File::create(&dump).expect("create");
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&rgba)
            .unwrap();
        println!("wrote {}", dump.to_string_lossy());
    }
    drop(editor);
}

//! Opens one installed plugin's **own** editor, for as long as it is told to.
//!
//! `cargo run -p fontelle-host --example plugin_editor -- <bundle> [id] [secs]`
//!
//! There is no unit test for a window somebody else draws: what is being
//! checked is that a plugin's editor appears, paints and takes a click, and
//! all three of those are the plugin's code and the desktop's. So this is the
//! §2.5 "seen once by a human" tool for [`fontelle_host::gui`] — the same role
//! `FONTELLE_UI_DUMP` plays for the studio's own renderer.
//!
//! `PROBE_DUMP=<file.png>` writes out what the plugin actually drew, taken off
//! the window itself rather than off the screen, so it works on a desktop with
//! no screenshot tool and under a compositor that will not give one up.
//!
//! `PROBE_THREADED=1` runs the processor on a thread of its own, at roughly
//! real-time pace, the way the studio does — so a plugin whose editor and
//! audio half disagree only when they are on different threads can be caught
//! here rather than in the window.
fn main() {
    let path = std::path::PathBuf::from(std::env::args().nth(1).expect("bundle"));
    let seconds: u64 = std::env::args()
        .nth(3)
        .and_then(|s| s.parse().ok())
        .unwrap_or(6);
    let plugins = fontelle_host::scan_bundle(&path).expect("scan");
    let want = std::env::args().nth(2);
    let info = match &want {
        Some(id) => plugins
            .iter()
            .find(|p| p.key.id == *id)
            .expect("no such plugin"),
        None => plugins.first().expect("empty bundle"),
    };
    let mut host = fontelle_host::PluginHost::new();
    let mut plugin = host.open(&info.path, &info.key).expect("open");
    println!("has_editor: {}", plugin.has_editor());
    if !plugin.has_editor() {
        return;
    }
    let mut _processor = Some(plugin.activate(48_000.0, 512).expect("activate"));
    let mut window =
        fontelle_host::PluginWindow::open(&info.name, fontelle_host::GuiSize::FALLBACK)
            .expect("window");
    let wanted = plugin.open_editor(&window, 1.0).expect("editor");
    println!(
        "plugin wants {wanted:?}, resizable {}",
        plugin.editor_resizable()
    );
    window.resize(wanted);
    plugin.resize_editor(wanted);

    // A real processor, so a plugin that answers its editor has somewhere to
    // answer from — an LV2 sampler's "I loaded that file" is an atom out of a
    // `run`, and with nothing running there is no run.
    let mut scratch = vec![vec![0.0f32; 512]; plugin.audio_outputs().max(1) as usize];
    let threaded = std::env::var_os("PROBE_THREADED").is_some();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let audio = threaded.then(|| {
        let stop = std::sync::Arc::clone(&stop);
        let mut processor = _processor.take().expect("activated");
        let mut scratch = scratch.clone();
        std::thread::spawn(move || {
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                processor.process_instrument(&mut scratch, 512);
                std::thread::sleep(std::time::Duration::from_micros(10_000));
            }
            processor
        })
    });
    let until = std::time::Instant::now() + std::time::Duration::from_secs(seconds);
    while std::time::Instant::now() < until {
        if let Some(processor) = _processor.as_mut() {
            processor.process_instrument(&mut scratch, 512);
        }
        plugin.tick_editor();
        let polled = window.poll();
        if let Some(size) = polled.resized {
            plugin.resize_editor(size);
        }
        if polled.closed {
            println!("closed by the desktop");
            break;
        }
        let asked = plugin.take_editor_requests();
        if let Some(size) = asked.resize {
            println!("the plugin asked for {size:?}");
            window.resize(size);
        }
        if asked.closed {
            println!("the plugin closed its own editor");
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
    if let Some(dump) = std::env::var_os("PROBE_DUMP")
        && let Some((w, h, rgba)) = window.grab()
    {
        let lit = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[0] > 8 || p[1] > 8 || p[2] > 8)
            .count();
        println!(
            "grabbed {w}x{h}, {lit} non-black pixels of {}",
            rgba.len() / 4
        );
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
    let (to_plugin, to_editor) = plugin.editor_traffic();
    println!("atoms carried: {to_plugin} to the plugin, {to_editor} to the editor");
    plugin.close_editor();
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    if let Some(audio) = audio {
        _processor = Some(audio.join().expect("the audio thread"));
    }
    if let Some(processor) = _processor {
        plugin.deactivate(processor);
    }
    println!("done");
}

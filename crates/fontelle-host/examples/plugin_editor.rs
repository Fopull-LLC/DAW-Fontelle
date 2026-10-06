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
//! `PROBE_HEADER=<pixels>` opens the window the studio does: a strip across
//! the top, and the plugin in a window of its own under it.
//!
//! `PROBE_COMPATIBLE=1` starts as the studio does with Settings → Compatible
//! plugin graphics on: EGL is Mesa's (`fontelle_host::gui::egl_vendor_for`).
//!
//! `PROBE_GATE=<helper>` asks what the studio asks before it opens an editor
//! (`fontelle_host::alpha_egl`), with `helper` — a `fontelle` or
//! `fontelle-scan-probe` binary — as the probe, and opens nothing if refused.
//!
//! `PROBE_MENU=<file.png>` puts that picture up over the window a second
//! in, under the strip, as the studio puts its preset drop-down up
//! (`PluginWindow::show_overlay`), and prints what is done to it.
//!
//! `PROBE_THREADED=1` runs the processor on a thread of its own, at roughly
//! real-time pace, the way the studio does — so a plugin whose editor and
//! audio half disagree only when they are on different threads can be caught
//! here rather than in the window.
fn main() {
    // As the studio does, first thing (`fontelle_host::gui::gdk_scale_for`).
    #[cfg(target_os = "linux")]
    // SAFETY: one thread so far.
    unsafe {
        fontelle_host::gui::steady_gdk_scale();
    }
    #[cfg(target_os = "linux")]
    if std::env::var_os("PROBE_COMPATIBLE").is_some() {
        // SAFETY: one thread so far.
        let set = unsafe { fontelle_host::gui::steady_egl_vendor(true) };
        println!("compatible plugin graphics: {set}");
    }
    let path = std::path::PathBuf::from(std::env::args().nth(1).expect("bundle"));
    let seconds: u64 = std::env::args()
        .nth(3)
        .and_then(|s| s.parse().ok())
        .unwrap_or(6);
    let plugins = fontelle_host::scan_bundle(&path).expect("scan");
    let want = std::env::args().nth(2).filter(|id| !id.is_empty());
    let info = match &want {
        Some(id) => plugins
            .iter()
            .find(|p| p.key.id == *id)
            .expect("no such plugin"),
        None => plugins.first().expect("empty bundle"),
    };
    if let Some(helper) = std::env::var_os("PROBE_GATE") {
        let gate = fontelle_host::EditorGate::probing(
            helper.into(),
            fontelle_host::gui::compatible_graphics_active(),
        );
        if let Some(refusal) = gate.refusal(&info.key, &info.name) {
            println!("refused: {refusal}");
            return;
        }
        println!("the gate lets it open ({} probe)", gate.probes());
    }
    let mut host = fontelle_host::PluginHost::new();
    let mut plugin = host.open(&info.path, &info.key).expect("open");
    println!("has_editor: {}", plugin.has_editor());
    if !plugin.has_editor() {
        return;
    }
    let mut _processor = Some(plugin.activate(48_000.0, 512).expect("activate"));
    // `PROBE_HEADER=<pixels>`: a strip across the top and the plugin in a
    // window under it, as the studio opens one.
    let header = std::env::var("PROBE_HEADER")
        .ok()
        .and_then(|h| h.parse().ok())
        .unwrap_or(0);
    let mut window = fontelle_host::PluginWindow::open_with_header(
        &info.name,
        fontelle_host::GuiSize::FALLBACK,
        header,
    )
    .expect("window");
    let wanted = plugin.open_editor(&window, window.scale()).expect("editor");
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
    let menu = std::env::var_os("PROBE_MENU").map(|path| {
        let decoder = png::Decoder::new(std::io::BufReader::new(
            std::fs::File::open(&path).expect("the menu's picture"),
        ));
        let mut reader = decoder.read_info().expect("a PNG");
        let mut rgba = vec![0; reader.output_buffer_size().expect("a size")];
        let info = reader.next_frame(&mut rgba).expect("its pixels");
        (rgba, info.width, info.height)
    });
    let mut menu_at = Some(std::time::Instant::now() + std::time::Duration::from_secs(1));
    let until = std::time::Instant::now() + std::time::Duration::from_secs(seconds);
    while std::time::Instant::now() < until {
        if let Some((rgba, width, height)) = &menu
            && menu_at.is_some_and(|at| std::time::Instant::now() >= at)
        {
            menu_at = None;
            window.show_overlay(40, header as i32, rgba, *width, *height);
            println!("menu up at {:?}", window.overlay());
        }
        if let Some(processor) = _processor.as_mut() {
            processor.process_instrument(&mut scratch, 512);
        }
        plugin.tick_editor();
        // The studio's event loop does this; the probe has none.
        fontelle_host::pump_gui_messages();
        let polled = window.poll();
        for event in &polled.overlay {
            println!("menu: {event:?}");
        }
        for press in &polled.header_presses {
            println!("strip: {press:?}");
        }
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

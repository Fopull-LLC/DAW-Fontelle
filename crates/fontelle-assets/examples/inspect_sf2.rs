//! Prints exactly what `import_sf2` extracted from a real SF2 file — every
//! layer's key/vel range, root key, tuning, loop points, gain/pan, and the
//! resulting amp envelope. No audio, no device access.
//!
//! ```text
//! cargo run -p fontelle-assets --example inspect_sf2 -- /path/to/file.sf2 [preset_index]
//! ```
//!
//! With no index it inspects preset 0 — which, since SF2 files store presets
//! in arbitrary order, is regularly *not* the instrument you want. The preset
//! list is printed first so you can pick.

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: inspect_sf2 <path.sf2> [preset_index]");
    let preset: usize = std::env::args()
        .nth(2)
        .and_then(|a| a.parse().ok())
        .unwrap_or(0);
    let path = std::path::Path::new(&path);

    let presets =
        fontelle_assets::list_presets(path).unwrap_or_else(|e| panic!("could not read: {e}"));
    println!("{} presets:", presets.len());
    for p in &presets {
        let marker = if p.index == preset { "->" } else { "  " };
        println!(
            "{marker} [{:>3}] prog={:<3} bank={:<3} {}",
            p.index, p.program, p.bank, p.name
        );
    }
    println!();

    let mut store = fontelle_core::SampleStore::new();
    let patch = fontelle_assets::import_sf2_preset(path, preset, &mut store)
        .unwrap_or_else(|e| panic!("import failed: {e}"));

    println!("layers: {}", patch.layers.len());
    for (i, l) in patch.layers.iter().enumerate() {
        let fontelle_core::Source::Sample { file } = l.source else {
            println!("  layer {i}: non-Sample source (not rendered yet)");
            continue;
        };
        let buf = store.get(file).unwrap();
        println!(
            "  layer {i}: key_range={:?} vel_range={:?} root_key={} fine_tune={} gain_db={} \
             pan={} loop_mode={:?} loop_start={} loop_end={} start_offset={} end_offset={} \
             samples={} sample_rate={}",
            l.key_range,
            l.vel_range,
            l.root_key,
            l.fine_tune_cents,
            l.gain_db,
            l.pan,
            l.playback.loop_mode,
            l.playback.loop_start,
            l.playback.loop_end,
            l.playback.start_offset,
            l.playback.end_offset,
            buf.data.len(),
            buf.sample_rate
        );
    }

    // A fingerprint of the decoded PCM, for checking our decode against an
    // independent extraction of the same file. A byte-offset or alignment
    // mistake in the sample reader shows up here immediately and is otherwise
    // very hard to distinguish from "the soundfont just sounds like that".
    if let Some(fontelle_core::Source::Sample { file }) = patch.layers.first().map(|l| &l.source) {
        let buf = store.get(*file).unwrap();
        let sum: f64 = buf.data.iter().map(|s| *s as f64).sum();
        let abs_sum: f64 = buf.data.iter().map(|s| s.abs() as f64).sum();
        let peak = buf.data.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        println!(
            "layer 0 pcm: len={} peak={peak:.6} sum={sum:.6} abs_sum={abs_sum:.6}",
            buf.data.len()
        );
        let mid = buf.data.len() / 2;
        let window: Vec<i32> = buf.data[mid..(mid + 12).min(buf.data.len())]
            .iter()
            .map(|s| (s * 32768.0).round() as i32)
            .collect();
        println!("layer 0 pcm[{mid}..]: {window:?}");
    }

    for (i, f) in patch.filters.iter().enumerate() {
        println!(
            "  filter {}: {}  mode={:?} cutoff={:.1}Hz q={:.3}",
            i + 1,
            if f.enabled { "ON " } else { "off" },
            f.mode,
            f.cutoff_hz,
            f.resonance
        );
    }

    println!("envelopes: {}", patch.envelopes.len());
    for (i, e) in patch.envelopes.iter().enumerate() {
        println!("  env {i}: {e:?}");
    }

    println!("lfos: {}", patch.lfos.len());
    for (i, l) in patch.lfos.iter().enumerate() {
        // SF2 names them: 0 is the modulation LFO, 1 the vibrato LFO.
        let name = match i {
            0 => " (mod)",
            1 => " (vib)",
            _ => "",
        };
        println!(
            "  lfo {i}{name}: {:.3} Hz  delay {:.3}s  depth {:.3}  {:?}",
            l.rate_hz, l.delay_s, l.depth, l.shape
        );
    }

    // The routes are what actually make a patch move; a file whose modulation
    // generators were dropped on the way in reads as an empty matrix here
    // rather than as a patch that merely sounds a bit flat.
    println!("mod matrix: {} route(s)", patch.mod_matrix.routes.len());
    for r in &patch.mod_matrix.routes {
        println!(
            "  {:?} -> {:?}  depth {:+.4} ({:+.1} in the destination's units)              curve={:?}{}",
            r.source,
            r.destination,
            r.depth,
            r.depth * r.destination.full_scale(),
            r.curve,
            if r.invert { " inverted" } else { "" }
        );
    }

    println!(
        "voice_config: polyphony={} steal={:?}",
        patch.voice_config.polyphony, patch.voice_config.steal_policy
    );
}

//! Prints exactly what `import_sf2` extracted from a real SF2 file — every
//! layer's key/vel range, root key, tuning, loop points, gain/pan, and the
//! resulting amp envelope. No audio, no device access.
//!
//! ```text
//! cargo run -p fontelle-assets --example inspect_sf2 -- /path/to/file.sf2
//! ```

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: inspect_sf2 <path.sf2>");
    let mut store = fontelle_core::SampleStore::new();
    let patch = fontelle_assets::import_sf2(std::path::Path::new(&path), &mut store)
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

    println!("envelopes: {}", patch.envelopes.len());
    for (i, e) in patch.envelopes.iter().enumerate() {
        println!("  env {i}: {e:?}");
    }

    println!(
        "voice_config: polyphony={} steal={:?}",
        patch.voice_config.polyphony, patch.voice_config.steal_policy
    );
}

//! The installed plugins themselves, rather than the fixtures.
//!
//! Ignored, because what they find depends on what is installed:
//!
//! `cargo test -p fontelle-host --test real_plugins -- --ignored --nocapture`
//!
//! `FONTELLE_REAL_ONLY=OB-Xf` narrows it to names containing that.

use fontelle_host::{PluginHost, PluginScan};
use fontelle_types::PluginFormat;

/// Plugins that take the whole test binary down, left out unless asked for
/// by name. Odin2 (CLAP, 2.4) dereferences null inside its own `process` on
/// the first block — with a transport and every port handed over; not
/// understood yet (2026-09-29).
const CRASHES: &[&str] = &["Odin2"];

fn search_paths() -> Vec<std::path::PathBuf> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let mut paths = vec![
        std::path::PathBuf::from("/usr/lib/clap"),
        std::path::PathBuf::from("/usr/lib/lv2"),
        std::path::PathBuf::from("/usr/lib/vst3"),
    ];
    if let Some(home) = home {
        paths.push(home.join(".clap"));
        paths.push(home.join(".lv2"));
        paths.push(home.join(".vst3"));
    }
    paths
}

fn peak(output: &[Vec<f32>]) -> f32 {
    output
        .iter()
        .flat_map(|c| c.iter())
        .fold(0.0f32, |a, b| a.max(b.abs()))
}

/// > *"i tried ob-xf and it was initially working but as soon as i tried
/// > actually encorperating it in my arrangement it would just stop
/// > producing sound or be doing pitch bends it wasnt doing before"*
///
/// The graph resets every node at a stop, a seek and each pass round a loop,
/// and a note still held there has its note-off on the far side of the cut.
/// CLAP's `reset` says it kills voices; a JUCE plugin's does not (JUCE's
/// `AudioProcessor::reset` is empty unless the plugin fills it), so each
/// held note was left sounding forever — until the voices ran out, or a new
/// note glided out of a stuck one. Whatever a plugin makes of its reset, a
/// reset processor is silent once its release has rung out.
#[test]
#[ignore]
fn every_installed_instrument_is_silent_after_a_reset() {
    let scan = PluginScan::of(&search_paths());
    let only = std::env::var("FONTELLE_REAL_ONLY").ok();
    let instruments: Vec<_> = scan
        .plugins
        .iter()
        .filter(|p| p.is_instrument() && p.key.format.hosted())
        .filter(|p| p.key.format != PluginFormat::Lv2 || only.is_some())
        .filter(|p| only.as_ref().is_none_or(|only| p.name.contains(only)))
        .filter(|p| only.is_some() || !CRASHES.iter().any(|name| p.name.contains(name)))
        .collect();
    assert!(!instruments.is_empty(), "no instruments installed");
    let mut host = PluginHost::new();
    let mut stuck = Vec::new();
    for info in instruments {
        let Ok(mut plugin) = host.open(&info.path, &info.key) else {
            continue;
        };
        let Ok(mut processor) = plugin.activate(48_000.0, 256) else {
            continue;
        };
        let mut output = vec![vec![0.0f32; 256]; plugin.audio_outputs().max(2) as usize];
        processor.note_on(0, 60, 0.8);
        let mut held = 0.0f32;
        for _ in 0..40 {
            processor.process_instrument(&mut output, 256);
            held = held.max(peak(&output));
        }
        if held < 1e-3 {
            eprintln!("{:<40} silent when played, skipped", info.name);
            continue;
        }
        processor.reset();
        // Four seconds for a release to ring out, then one more to listen.
        for _ in 0..750 {
            processor.process_instrument(&mut output, 256);
        }
        let mut after = 0.0f32;
        for _ in 0..190 {
            processor.process_instrument(&mut output, 256);
            after = after.max(peak(&output));
        }
        eprintln!("{:<40} held {held:.3}, after a reset {after:.4}", info.name);
        if after > held * 0.01 {
            stuck.push(info.name.clone());
        }
    }
    assert!(stuck.is_empty(), "still sounding after a reset: {stuck:?}");
}

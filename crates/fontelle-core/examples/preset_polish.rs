//! Prints the readings `tests/preset_polish.rs` gates the bank on, for
//! every preset (or one shelf, or one name) — the table a sound-design pass
//! tunes against, the way `preset_probe` is the table for loudness.
//!
//! > *"the synth is capable of a lot and making really polished sounds but
//! > many still sound unpolished or janky please have an agent do a full
//! > audit on all of the built in presets"* — Ty, 2026-11-12
//!
//! Per preset, middle C at velocity 100, held two seconds:
//!
//! - `click` — the worst 2 ms burst of high-passed energy after the first
//!   5 ms, in dB over the loudest such window in the 30 ms either side,
//!   how far under the note it is, and when (the gate: over 10 dB *and*
//!   within 40 dB of the note);
//! - `onset` — the same inside the first 5 ms;
//! - `dc`, `sub` — the body's mean against its RMS, and its energy under
//!   30 Hz, both in dB (the gate: −30 and −20);
//! - `alias` — off-grid energy at C6 as written, and what 4× takes off it
//!   (the gate: within 45 dB of the tone *and* more than 10 dB);
//! - `peak` — when the loudest 10 ms arrives (the gate, for plucks and
//!   strikes: 0.12 s).
//!
//! ```text
//! cargo run -p fontelle-core --example preset_polish --release [shelf | name]
//! cargo run -p fontelle-core --example preset_polish --release -- --axes <name>…
//! ```
//!
//! `--axes` prints the bank gate's five axes (`preview::sound_vector`) for
//! the named presets and every pair's distance — what a row nudged to pass
//! one gate needs, to see that it has not walked into a neighbour in the
//! other (the gate there is 0.35). Oversampling a bright row takes its fold
//! away, and the fold was sometimes what told two rows apart.

#[allow(dead_code)]
#[path = "../tests/support/polish.rs"]
mod polish;

use fontelle_core::flopsynth::presets::FACTORY;

fn axes(names: &[String]) {
    let mut got = Vec::new();
    for name in names {
        let Some(row) = FACTORY.iter().find(|r| r.name == name) else {
            eprintln!("no preset called {name}");
            std::process::exit(1);
        };
        // The gate's render: one note held six tenths, two and a half
        // seconds in all.
        let (l, r) = polish::render((row.build)(), 60, 100, 0.6, 2.5);
        let v = fontelle_core::preview::sound_vector(&polish::mono(&l, &r));
        let bands: String = v
            .shape
            .iter()
            .map(|b| format!("{:>4.0}", b * 100.0))
            .collect();
        println!(
            "{name:<20} {:>6.2} {:>6.2} {:>6.2} {:>6.2} |{bands}",
            v.scalar[0], v.scalar[1], v.scalar[2], v.scalar[3]
        );
        got.push((name, v));
    }
    for (i, (a, x)) in got.iter().enumerate() {
        for (b, y) in &got[i + 1..] {
            let scalar = x
                .scalar
                .iter()
                .zip(&y.scalar)
                .map(|(p, q)| (p - q).abs())
                .fold(0.0f32, f32::max);
            let shape: f32 = x
                .shape
                .iter()
                .zip(&y.shape)
                .map(|(p, q)| (p - q).abs())
                .sum();
            println!(
                "  {a} / {b}: {:.2} (scalar {scalar:.2}, shape {shape:.2})",
                scalar.max(shape)
            );
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--axes") {
        axes(&args[1..]);
        return;
    }
    let only = args.first();
    println!(
        "{:<22} {:<16} {:>17}  {:>11}  {:>6} {:>6}  {:>12}  {:>5}",
        "preset", "category", "click dB/under/s", "onset", "dc", "sub", "alias", "peak"
    );
    for (row, r) in FACTORY.iter().zip(polish::bank()) {
        let wanted = only.is_none_or(|want| {
            row.category.label().eq_ignore_ascii_case(want) || row.name.eq_ignore_ascii_case(want)
        });
        if !wanted {
            continue;
        }
        println!(
            "{:<22} {:<16} {:>5.1} {:>5.1} {:>5.2}  {:>5.1} {:>5.1}  {:>6.1} {:>6.1}  {:>6.1} {:>5.1}  {:>5.2}",
            row.name,
            row.category.label(),
            r.click.0,
            r.click.1,
            r.click.2,
            r.onset.0,
            r.onset.1,
            r.dc,
            r.sub,
            r.alias.0,
            r.alias.1,
            r.peak_at
        );
    }
}

//! Which numbers two kits still share, hit by hit.
use fontelle_core::{DrumKitStyle as S, drum_slots};
fn main() {
    let names = ["Kick", "Snare", "Closed Hat", "Open Hat", "Tom Low", "Ride"];
    let args: Vec<String> = std::env::args().skip(1).collect();
    let find = |n: &str| S::ALL.into_iter().find(|s| s.label() == n).unwrap();
    let (a, b) = (find(&args[0]), find(&args[1]));
    let (sa, sb) = (drum_slots(a), drum_slots(b));
    println!("{} vs {}", a.label(), b.label());
    for n in names {
        let va = sa.iter().find(|s| s.name == n).unwrap().voice;
        let vb = sb.iter().find(|s| s.name == n).unwrap().voice;
        let f: [(&str, f32, f32); 11] = [
            ("tune", va.tune_hz, vb.tune_hz),
            ("decay", va.decay_s, vb.decay_s),
            ("tone", va.tone_hz, vb.tone_hz),
            ("noise", va.noise, vb.noise),
            ("snap", va.snap, vb.snap),
            ("drive", va.drive, vb.drive),
            ("metal", va.metal, vb.metal),
            ("crush", va.crush, vb.crush),
            ("modes", va.modes, vb.modes),
            ("tail", va.tail, vb.tail),
            ("rattle", va.rattle, vb.rattle),
        ];
        let same: Vec<&str> = f
            .iter()
            .filter(|(_, x, y)| (x - y).abs() < 1e-4 * x.abs().max(1.0))
            .map(|(n, _, _)| *n)
            .collect();
        let diff: Vec<String> = f
            .iter()
            .filter(|(_, x, y)| (x - y).abs() >= 1e-4 * x.abs().max(1.0))
            .map(|(n, x, y)| format!("{n} {x:.2}/{y:.2}"))
            .collect();
        println!("  {n:<11} SAME[{}]  {}", same.join(","), diff.join(" "));
    }
}

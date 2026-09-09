//! **Do two kits actually sound like two kits?**
//!
//! > *"we did some work on improving the drum machine built in plugin however
//! > all of the presets still sound nearly the same"*
//!
//! The third report, and the first one measured through the whole instrument.
//! `fontelle-core`'s `every_pair_of_kits_is_audibly_apart` reads three numbers
//! off three raw voices and folds them with `max` — so two kits pass it by
//! differing on **one** reading out of nine and being identical on the other
//! eight. It is a floor on the best case, which is why it stayed green through
//! two rounds of the same complaint.
//!
//! This is a floor on the **typical** case, and on the kit rather than the
//! hit. A kit is not thirty-six one-shots: it is thirty-six one-shots *and a
//! bus*, and the bus is where a rock kit becomes a room, a gated snare becomes
//! a gate and an 808 stays bone dry. That chain lives on the patch
//! ([`PatchFx`]) and is run by `SamplerNode`, so this test has to be here —
//! INVARIANT 4 forbids core from seeing `fontelle-fx`, and core therefore
//! cannot render the thing that makes a kit a kit.
//!
//! # What the fingerprint is
//!
//! Six hits anybody auditions, each as twenty log-spaced bands over ten 43 ms
//! frames, in decibels, **with each hit's own loudness taken out** — two kits
//! are not different because one is turned up. Two kits are as far apart as
//! the RMS decibel difference between their fingerprints, which is a reading
//! of how differently they are shaped over the whole first half second rather
//! than of whether they differ anywhere at all.

use fontelle_core::{SampleStore, Sampler};
use fontelle_engine::{
    AudioNode, PrepareContext, ProcessContext, SamplerNode, TransportSnapshot, TransportState,
};
use fontelle_types::{EventPayload, NodeId, TimedEvent};
use std::sync::Arc;

const SR: f32 = 48_000.0;
const BLOCK: usize = 512;

/// Kick, Snare, Closed Hat, Open Hat, Tom Low, Ride — General MIDI's keys for
/// the six hits somebody plays to find out what a kit is.
const HITS: [u8; 6] = [36, 38, 42, 46, 43, 51];

const BANDS: usize = 20;
const FRAMES: usize = 20;
/// 2048 at 48 kHz is 43 ms.
const FRAME: usize = 2048;
/// **Half a frame, and the frames therefore overlap.**
///
/// Not a detail. With frames laid end to end the first one starts exactly at
/// the hit, and a Hann window is *zero* at its start — so the attack, which is
/// the most recognisable 5 ms of any drum, was windowed away to nothing. The
/// reading said a hit with its click at full and the same hit with no click at
/// all were identical, which is false about drums and would have sent the
/// whole of this round tuning the wrong knobs. Overlapping by half, with the
/// signal padded by half a frame first, puts the onset at the centre of the
/// first window where the window is one.
const HOP: usize = FRAME / 2;

/// One hit through the whole instrument — voice sum *and* the patch's chain —
/// summed to mono, because this measures timbre and not stereo image.
fn render_hit(patch: &fontelle_core::Patch, key: u8) -> Vec<f32> {
    let store = Arc::new(SampleStore::new());
    let mut node = SamplerNode::new(Sampler::new(patch.clone()), store);
    node.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: BLOCK as u32,
    });
    let blocks = (FRAMES * HOP + FRAME).div_ceil(BLOCK);
    let mut out = Vec::with_capacity(blocks * BLOCK);
    for block in 0..blocks {
        let mut left = vec![0.0f32; BLOCK];
        let mut right = vec![0.0f32; BLOCK];
        let at = (block * BLOCK) as i64;
        let events: Vec<TimedEvent> = if block == 0 {
            vec![TimedEvent {
                sample: 0,
                target: NodeId::default(),
                payload: EventPayload::NoteOn {
                    key,
                    velocity: 100,
                    pan: 0,
                    fine_pitch: 0,
                    release: 0,
                    mod_x: 0,
                    mod_y: 0,
                    voice_context: 0,
                },
            }]
        } else {
            Vec::new()
        };
        {
            let (l, r) = (&mut left[..], &mut right[..]);
            let mut outputs: [&mut [f32]; 2] = [l, r];
            let mut ctx = ProcessContext {
                inputs: &[],
                outputs: &mut outputs,
                all_events: &events,
                live_events: &[],
                audio: &[],
                node: NodeId::default(),
                transport: TransportSnapshot {
                    state: TransportState::Playing,
                    position_sample: at,
                    bpm: 120.0,
                },
                sample_range: at..at + BLOCK as i64,
            };
            node.process(&mut ctx);
        }
        out.extend(left.iter().zip(&right).map(|(l, r)| 0.5 * (l + r)));
    }
    out.truncate(FRAMES * HOP + FRAME);
    out
}

/// In-place radix-2 FFT. A whole spectrum rather than probes at band centres:
/// a sparse probe misses a line that falls between two of them, and a drum's
/// body is exactly such a line.
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -std::f32::consts::TAU / len as f32;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (wr, wi) = (ang * k as f32).cos_sin_pair();
                let (ur, ui) = (re[start + k], im[start + k]);
                let (vr, vi) = (
                    re[start + k + len / 2] * wr - im[start + k + len / 2] * wi,
                    re[start + k + len / 2] * wi + im[start + k + len / 2] * wr,
                );
                re[start + k] = ur + vr;
                im[start + k] = ui + vi;
                re[start + k + len / 2] = ur - vr;
                im[start + k + len / 2] = ui - vi;
            }
        }
        len <<= 1;
    }
}

trait CosSin {
    fn cos_sin_pair(self) -> (f32, f32);
}
impl CosSin for f32 {
    fn cos_sin_pair(self) -> (f32, f32) {
        (self.cos(), self.sin())
    }
}

/// One hit's fingerprint: `BANDS * FRAMES` decibels with the mean removed.
fn fingerprint(patch: &fontelle_core::Patch, key: u8) -> Vec<f32> {
    fingerprint_of(&render_hit(patch, key))
}

/// The same reading, of a signal from wherever.
fn fingerprint_of(signal: &[f32]) -> Vec<f32> {
    // Half a frame of silence in front, so the hit lands at the centre of the
    // first window rather than at its zero.
    let mut padded = vec![0.0f32; HOP];
    padded.extend_from_slice(signal);
    padded.resize(FRAMES * HOP + FRAME + HOP, 0.0);
    let mut out = Vec::with_capacity(BANDS * FRAMES);
    for f in 0..FRAMES {
        let frame = &padded[f * HOP..f * HOP + FRAME];
        let mut re: Vec<f32> = frame
            .iter()
            .enumerate()
            .map(|(i, x)| {
                let w = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / FRAME as f32).cos();
                x * w
            })
            .collect();
        let mut im = vec![0.0f32; FRAME];
        fft(&mut re, &mut im);
        let mut power = [0.0f32; BANDS];
        // 40 Hz to 16 kHz over twenty bands, every bin counted once.
        for bin in 1..FRAME / 2 {
            let hz = bin as f32 * SR / FRAME as f32;
            if !(40.0..16_000.0).contains(&hz) {
                continue;
            }
            let b = ((hz / 40.0).ln() / (16_000f32 / 40.0).ln() * BANDS as f32) as usize;
            power[b.min(BANDS - 1)] += re[bin] * re[bin] + im[bin] * im[bin];
        }
        out.extend(
            power
                .iter()
                .map(|p| 10.0 * (p / FRAME as f32).max(1e-12).log10()),
        );
    }
    // **Floored sixty decibels under this hit's loudest band.** Without it the
    // reading is dominated by the parts of the spectrum that are silence: a
    // band at -118 dB against one at -104 is fourteen decibels of "difference"
    // nobody can hear, and there are far more silent bands in a drum's last
    // frames than there are loud ones in its first. Sixty down is the range a
    // hit actually occupies.
    let top = out.iter().copied().fold(f32::MIN, f32::max);
    for v in &mut out {
        *v = v.max(top - 60.0);
    }
    // And the loudness out: two kits are not different because one is
    // turned up.
    let mean = out.iter().sum::<f32>() / out.len() as f32;
    for v in &mut out {
        *v -= mean;
    }
    out
}

fn kit_fingerprint(style: fontelle_core::DrumKitStyle) -> Vec<f32> {
    let patch = fontelle_core::drum_kit(style);
    HITS.iter()
        .flat_map(|key| fingerprint(&patch, *key))
        .collect()
}

fn distance(a: &[f32], b: &[f32]) -> f32 {
    (a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum::<f32>() / a.len() as f32).sqrt()
}

/// How far apart two kits have to be, in RMS decibels over the fingerprint.
///
/// **Seven and a half, and the number is read off a pair rather than chosen.**
/// Boom Bap and Lo-Fi are the catalogue's deliberate cousins — both are a
/// sampler with the top eaten off it, and nobody would file a complaint that
/// they resemble one another. They measure 7.67 apart. So that is what this
/// gate forbids: not *neighbours*, which a catalogue of twenty-two kits
/// legitimately has, but **duplicates** — no pair may be closer than the two
/// that are meant to be cousins.
///
/// For scale, on the same reading: 808 against 909 is 8.6, Rock against Trap
/// 10.9, Studio against Chiptune 14.6. Nine — the 808/909 gap — was the first
/// number tried and it is the wrong one: it sits near the far edge of the
/// space, and requiring every pair to be as unlike as those two is requiring
/// a catalogue with no families in it.
///
/// Setting the floor from a pair, rather than from whatever the kits happen to
/// manage, is the point. A threshold fitted to the current numbers is the same
/// mistake as folding nine readings with `max`, which is what let this ship
/// twice.
const APART_DB: f32 = 7.5;

#[test]
fn every_pair_of_kits_is_a_different_kit() {
    let kits: Vec<(&'static str, Vec<f32>)> = fontelle_core::DrumKitStyle::ALL
        .iter()
        .map(|s| (s.label(), kit_fingerprint(*s)))
        .collect();
    let mut close = Vec::new();
    for (i, (a, fa)) in kits.iter().enumerate() {
        for (b, fb) in &kits[i + 1..] {
            let d = distance(fa, fb);
            if d < APART_DB {
                close.push((d, *a, *b));
            }
        }
    }
    close.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
    let report: Vec<String> = close
        .iter()
        .map(|(d, a, b)| format!("  {d:5.2} dB  {a} / {b}"))
        .collect();
    assert!(
        close.is_empty(),
        "{} pairs of kits are the same kit (floor {APART_DB} dB):\n{}",
        close.len(),
        report.join("\n")
    );
}

/// The whole distribution, for tuning kits against rather than guessing.
///
/// ```text
/// cargo test -p fontelle-engine --test drum_kit_bus -- --ignored --nocapture
/// ```
#[test]
#[ignore = "a reporting tool, not a gate"]
fn how_far_apart_every_pair_is() {
    let kits: Vec<(&'static str, Vec<f32>)> = fontelle_core::DrumKitStyle::ALL
        .iter()
        .map(|s| (s.label(), kit_fingerprint(*s)))
        .collect();
    let mut pairs: Vec<(f32, &str, &str)> = Vec::new();
    for (i, (a, fa)) in kits.iter().enumerate() {
        for (b, fb) in &kits[i + 1..] {
            pairs.push((distance(fa, fb), a, b));
        }
    }
    pairs.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
    println!("== closest twelve ==");
    for (d, a, b) in pairs.iter().take(12) {
        println!("  {d:5.2}  {a} / {b}");
    }
    println!("== each kit's nearest neighbour ==");
    for (a, fa) in &kits {
        let (d, b) = kits
            .iter()
            .filter(|(b, _)| b != a)
            .map(|(b, fb)| (distance(fa, fb), *b))
            .fold((f32::MAX, ""), |m, x| if x.0 < m.0 { x } else { m });
        println!("  {a:<14} {d:5.2} from {b}");
    }
    let at = |q: f32| pairs[((pairs.len() - 1) as f32 * q) as usize].0;
    println!(
        "min {:.2}  p10 {:.2}  p25 {:.2}  median {:.2}  p75 {:.2}  max {:.2}",
        pairs[0].0,
        at(0.10),
        at(0.25),
        at(0.50),
        at(0.75),
        pairs.last().unwrap().0
    );
}

/// **A bus is a gain stage, and a gain stage can ruin a kit.**
///
/// `fontelle-core`'s `a_kit_does_not_clip_when_a_whole_bar_of_it_lands_at_once`
/// holds the voices, but it cannot see the chain — and the chain is where the
/// danger now is: Industrial drives eighteen decibels into a diode, Ambient is
/// seventy percent a reverb eight seconds long, and a compressor with
/// auto-makeup puts back whatever its threshold took. So the same promise is
/// made again, this time through everything.
#[test]
fn no_kit_leaves_full_scale_through_its_own_bus() {
    let mut worst: Vec<(f32, &str)> = Vec::new();
    for style in fontelle_core::DrumKitStyle::ALL {
        let patch = fontelle_core::drum_kit(style);
        // The six loudest hits of the kit at once, which is a crash landing on
        // the downbeat of a bar that already has everything else in it.
        let peak = HITS
            .iter()
            .map(|key| {
                render_hit(&patch, *key)
                    .iter()
                    .fold(0.0f32, |m, s| m.max(s.abs()))
            })
            .sum::<f32>();
        worst.push((peak, style.label()));
    }
    worst.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    for (p, n) in &worst {
        println!(
            "{n:<14} {p:.3}  (trim {:+.1} dB to reach 0.80)",
            20.0 * (0.80 / p).log10()
        );
    }
    let over: Vec<String> = worst
        .iter()
        .filter(|(p, _)| *p >= 1.0)
        .map(|(p, n)| format!("  {n} peaks at {p:.2}"))
        .collect();
    assert!(
        over.is_empty(),
        "kits that clip through their own bus:\n{}\nloudest that does not: {} at {:.2}",
        over.join("\n"),
        worst
            .iter()
            .find(|(p, _)| *p < 1.0)
            .map(|(_, n)| *n)
            .unwrap_or("none"),
        worst
            .iter()
            .find(|(p, _)| *p < 1.0)
            .map(|(p, _)| *p)
            .unwrap_or(0.0),
    );
}

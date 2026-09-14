//! **An organ's key click is a burst, not a bed.**
//!
//! > *"the rock organ sounds good but the rest of the flopsynth organ presets
//! > sound very noisy."*
//!
//! Measured, and the report is exact. The `organ` archetype's key click was a
//! noise layer at −30 dB, high-passed, *always on*, with an envelope adding a
//! burst on top — so every preset built on it carried a hiss bed for as long
//! as the key was down, and on a thin registration the hiss was the larger
//! half of the sound. Harmonic-to-noise over the sustain, before:
//!
//! ```text
//! Percussive   -4.9 dB     Combo         4.5 dB
//! Drawbar Jazz -4.0 dB     Reed Organ    4.6 dB
//! Farfisa      -3.6 dB     Bass Pedals   6.3 dB
//! Church       -1.9 dB     Gospel        7.9 dB
//! Theatre      -1.7 dB     Rock Organ   17.0 dB
//! Drawbar 888  -1.2 dB
//! Pipe Flute   -0.2 dB
//! ```
//!
//! With the noise layer muted the same presets read 16–50 dB. The Rock Organ
//! reads 17 only because 22 dB of tube drive and a ladder filter sit on top
//! of its hiss; it is the one Ty did not complain about.
//!
//! Two claims, then. The sustain of every organ is **mostly its note** — the
//! same reading as `wind_breath.rs`, against a comb the presets' sub-octave
//! and pedal layers all sit on. And on the Hammonds, the noise layer is a
//! **click**: what it does in the first ten milliseconds is far louder than
//! what it does for the rest of the note, which is what the contacts on a
//! tone-wheel organ actually do.

use fontelle_core::flopsynth::presets::FACTORY;
use fontelle_core::{ModDest, NoteTrigger, Patch, PrepareContext, SILENT_DB, SampleStore, Sampler};

const SR: f32 = 48_000.0;
const N: usize = 16_384;

fn render(patch: Patch, key: u8, frames: usize) -> Vec<f32> {
    let store = SampleStore::new();
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 128,
    });
    sampler.trigger(NoteTrigger::new(key, 100));
    let mut out = Vec::with_capacity(frames);
    while out.len() < frames {
        let n = 128.min(frames - out.len());
        let mut l = vec![0.0f32; n];
        let mut r = vec![0.0f32; n];
        sampler.render(&store, &mut [&mut l[..], &mut r[..]]);
        for (a, b) in l.iter().zip(&r) {
            out.push((a + b) * 0.5);
        }
    }
    out
}

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
                let (wr, wi) = ((ang * k as f32).cos(), (ang * k as f32).sin());
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

/// Energy on the harmonics of `f0` against everything else, in dB.
///
/// `f0` is *given*, not detected: an organ is the one instrument whose
/// registration puts real energy an octave and two octaves under the key
/// (the 16′ bar, the pedals), and autocorrelation lands on a different one of
/// them for each preset. Every layer the shelf builds sits on a harmonic of
/// two octaves under the key, so that is the comb.
fn hnr_db(signal: &[f32], f0: f32) -> f32 {
    let mut re: Vec<f32> = signal
        .iter()
        .take(N)
        .enumerate()
        .map(|(i, x)| {
            let w = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / N as f32).cos();
            x * w
        })
        .collect();
    re.resize(N, 0.0);
    let mut im = vec![0.0f32; N];
    fft(&mut re, &mut im);

    let bin_hz = SR / N as f32;
    let slack = 4i64;
    let mut harmonic = vec![false; N / 2];
    let mut h = 1;
    while (h as f32) * f0 < SR * 0.45 {
        let centre = ((h as f32 * f0) / bin_hz).round() as i64;
        for d in -slack..=slack {
            let b = centre + d;
            if b > 0 && (b as usize) < N / 2 {
                harmonic[b as usize] = true;
            }
        }
        h += 1;
    }
    let (mut tone, mut noise) = (0.0f64, 0.0f64);
    for bin in (50.0 / bin_hz) as usize..N / 2 {
        let p = (re[bin] * re[bin] + im[bin] * im[bin]) as f64;
        if harmonic[bin] {
            tone += p;
        } else {
            noise += p;
        }
    }
    if noise <= 0.0 {
        return 99.0;
    }
    (10.0 * (tone / noise).log10()) as f32
}

fn rms_db(samples: &[f32]) -> f32 {
    let mean = samples.iter().map(|x| x * x).sum::<f32>() / samples.len().max(1) as f32;
    10.0 * mean.max(1e-12).log10()
}

/// Flopsynth's slot order is fixed — A, B, C, sub, **noise**.
const NOISE_LAYER: usize = 4;

/// The key: A4, so the comb two octaves under it is A2.
const KEY: u8 = 69;
const COMB_HZ: f32 = 110.0;

/// Read off the shelf, not chosen. With the bed gone the shelf's lowest
/// readings are the Rock Organ and the Combo at 30 dB — neither has a noise
/// layer in its sustain at all; it is the fast Leslie's sidebands on the
/// ladder's drive, and the Vox's squares through its filter, falling outside
/// the comb. Twenty-five leaves them five; nothing with a hiss bed came
/// within fifteen of it.
const FLOOR_DB: f32 = 25.0;

#[test]
fn every_organ_preset_is_more_note_than_hiss() {
    let mut bad = Vec::new();
    let mut checked = 0;
    for row in FACTORY {
        if row.category.label() != "Organ" {
            continue;
        }
        checked += 1;
        let signal = render((row.build)(), KEY, N + 4_096);
        // Past the attack: the click and the chiff are *meant* to be noise.
        let steady = &signal[2_048..];
        let hnr = hnr_db(steady, COMB_HZ);
        if hnr < FLOOR_DB {
            bad.push(format!("  {:6.1} dB  {}", hnr, row.name));
        }
    }
    assert!(checked >= 10, "only {checked} organs were checked");
    bad.sort();
    assert!(
        bad.is_empty(),
        "organs whose sustain is more hiss than note (floor {FLOOR_DB} dB):\n{}",
        bad.join("\n")
    );
}

/// The tone-wheel presets — the ones whose noise layer is a Hammond's key
/// contacts. The transistor combos and the pipe organs have no click and are
/// not asked.
const HAMMONDS: [&str; 6] = [
    "Drawbar 888",
    "Drawbar Jazz",
    "Percussive",
    "Rock Organ",
    "Gospel",
    "Bass Pedals",
];

/// The patch with only its noise layer left in, so the click can be heard on
/// its own.
fn noise_alone(mut patch: Patch) -> Patch {
    for (index, layer) in patch.layers.iter_mut().enumerate() {
        if index != NOISE_LAYER {
            layer.gain_db = SILENT_DB;
        }
    }
    patch.mod_matrix.routes.retain(|route| {
        !matches!(route.destination, ModDest::LayerGain(i) if usize::from(i) != NOISE_LAYER)
    });
    patch
}

#[test]
fn the_key_click_is_a_burst_not_a_bed() {
    let mut bad = Vec::new();
    for name in HAMMONDS {
        let row = FACTORY
            .iter()
            .find(|r| r.name == name)
            .unwrap_or_else(|| panic!("no {name}"));
        let whole = render((row.build)(), KEY, N);
        let signal = render(noise_alone((row.build)()), KEY, N);
        let click = rms_db(&signal[..480]);
        // From a tenth of a second on: whatever the layer does here it does
        // for the whole note.
        let bed = rms_db(&signal[4_800..]);
        // The other half of the claim — that the click was not simply
        // deleted — is read against the preset's own tone rather than an
        // absolute level, because the Rock Organ's ladder drive puts its
        // tone twenty decibels over the shelf's and its trim takes the
        // click down with everything else.
        let tone = rms_db(&whole[4_800..]);
        if click - bed < 30.0 || click < tone - 30.0 {
            bad.push(format!(
                "  {name}: click {click:.1} dB, bed {bed:.1} dB, tone {tone:.1} dB"
            ));
        }
    }
    assert!(
        bad.is_empty(),
        "these clicks are beds — the first 10 ms should be 30 dB over the rest, \
         and within 30 dB of the tone:\n{}",
        bad.join("\n")
    );
}

//! Cuts the factory Grand Piano's stereo recordings out of the Salamander
//! Grand Piano's own files.
//!
//! > *"the default piano is also extremely clicky sounding and not very
//! > realistic help me make the default grand piano preset for flopsynth
//! > just sound a lot more natural and professional and clean like a real
//! > grand piano"* — Ty, 2026-10-19
//!
//! The first cut (`grand_samples.rs`) was made to fit in six megabytes: two
//! strengths, a key every major third, summed to mono, 24–32 kHz, and four
//! seconds at most — so a held middle C was gone in four, and every key was
//! a recording pulled up to two semitones off its own pitch. This one is cut
//! for the piano to sound like the piano:
//!
//! - **Every recording the C5 was sampled at** — the bottom A and every
//!   minor third up, thirty keys, so no key is more than a semitone and a
//!   half from a recording of its own.
//! - **Three strengths**, Salamander's layers 3, 7 and 14 of sixteen: a
//!   soft touch, a firm one and a hard one, evenly apart in tone. The C5
//!   brightens steeply over its softest four layers and hardly at all over
//!   its top six, so layers evenly apart in *number* (2, 10, 15 was tried)
//!   put a step of fifteen decibels of brightness at the first change and
//!   one at the second; these three are three to seven apart at each.
//! - **Stereo**, as the two microphones over the strings heard it, at the
//!   48 kHz it was recorded at — **each key's balance set along the
//!   keyboard**: a pair a hand above the strings hears a note far louder in
//!   whichever microphone its strings were under, up to fifteen decibels,
//!   so played as it was the piano jumps from side to side. Each recording
//!   keeps its own width and is levelled to lean from three decibels left
//!   in the bass to three right at the top, as a player hears it.
//! - **Long**: six seconds in the bass, falling to under two at the top,
//!   each ending in a slow fade rather than a cut.
//! - **Level-matched by strength**: each layer is scaled to the hard one's
//!   loudness at its key, so velocity is what makes a note loud and the step
//!   from one recording to the next is a change of tone, never a jump.
//!
//! Written as Ogg Vorbis (`ffmpeg`, quality 6), which the binary decodes once
//! (`fontelle_core::factory_samples`); a raw cut would be some forty
//! megabytes of the binary.
//!
//! ```text
//! cargo run -p fontelle-app --example grand_stereo_samples --release -- \
//!     <SalamanderGrandPiano-SFZ+FLAC folder>/samples assets/flopsynth/samples/grand-stereo
//! ```

use std::path::Path;

/// Salamander's layer for each of ours, and what we call it.
const LAYERS: [(&str, u32); 3] = [("soft", 3), ("medium", 7), ("hard", 14)];

/// Salamander's keys: the bottom A and every minor third up to C8.
fn keys() -> Vec<u8> {
    (21..=108).step_by(3).collect()
}

/// Salamander names a key `A0`, `D#1`, `F#3`; we write `Ds1`, `Fs3`.
fn salamander_name(key: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!(
        "{}{}",
        NAMES[usize::from(key % 12)],
        i32::from(key / 12) - 1
    )
}

fn file_name(key: u8) -> String {
    salamander_name(key).replace('#', "s")
}

/// How long a key's recording is kept: as long as its string still sings
/// at a level a held note is heard at, in a room. The bass rings for half a
/// minute on the C5, the top for two seconds; past these the amp
/// envelope's own fade is what is heard.
fn seconds(key: u8) -> f32 {
    match key {
        0..=42 => 6.0,
        43..=66 => 4.5,
        67..=78 => 3.2,
        79..=90 => 2.4,
        _ => 1.6,
    }
}

/// The level of the first `seconds` after the onset, both channels.
fn rms(frames: &[[f32; 2]], seconds: f32, rate: u32) -> f32 {
    let n = ((seconds * rate as f32) as usize).min(frames.len()).max(1);
    let sum: f32 = frames[..n].iter().map(|f| f[0] * f[0] + f[1] * f[1]).sum();
    (sum / (2 * n) as f32).sqrt()
}

/// The note's frames from just before its onset, faded in over a
/// millisecond and out over the last part, `seconds` long at most.
fn cut(asset: &fontelle_assets::AudioAsset, seconds: f32) -> Vec<[f32; 2]> {
    let rate = asset.sample_rate as f32;
    let frames: Vec<[f32; 2]> = (0..asset.frames)
        .map(|i| {
            [
                asset.sample(i, 0),
                asset.sample(i, 1.min(asset.channels - 1)),
            ]
        })
        .collect();
    let peak = frames
        .iter()
        .fold(0f32, |m, f| m.max(f[0].abs()).max(f[1].abs()));
    // Two milliseconds before the first sample within 60 dB of the note's
    // own peak: the room before the hammer is kept short, never cut into.
    let onset = frames
        .iter()
        .position(|f| f[0].abs().max(f[1].abs()) > peak * 1e-3)
        .unwrap_or(0)
        .saturating_sub((rate * 0.002) as usize);
    let mut note: Vec<[f32; 2]> = frames[onset..]
        .iter()
        .copied()
        .take((rate * seconds) as usize)
        .collect();
    let fade_in = (rate * 0.001) as usize;
    for (i, f) in note.iter_mut().take(fade_in).enumerate() {
        let g = i as f32 / fade_in as f32;
        f[0] *= g;
        f[1] *= g;
    }
    // The last third, or a second at most, on a raised cosine: a note held
    // past its recording dies away into the release instead of stopping.
    let len = note.len();
    let fade = (len / 3).min(rate as usize);
    for (i, f) in note[len - fade..].iter_mut().enumerate() {
        let t = i as f32 / fade as f32;
        let g = 0.5 + 0.5 * (std::f32::consts::PI * t).cos();
        f[0] *= g;
        f[1] *= g;
    }
    note
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(src), Some(out)) = (args.get(1), args.get(2)) else {
        eprintln!("usage: grand_stereo_samples <salamander samples folder> <out-dir>");
        std::process::exit(2);
    };
    let (src, out) = (Path::new(src), Path::new(out));
    let scratch = std::env::temp_dir().join(format!("grand-stereo-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();

    // Every cut first, so the gains can be set against each other.
    let mut cuts: Vec<(usize, u8, u32, Vec<[f32; 2]>)> = Vec::new();
    for (at, (_, layer)) in LAYERS.iter().enumerate() {
        for key in keys() {
            let path = src.join(format!("{}v{layer}.flac", salamander_name(key)));
            let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let asset = fontelle_assets::read_audio(&bytes, &path.display().to_string())
                .expect("Salamander's files decode");
            cuts.push((at, key, asset.sample_rate, cut(&asset, seconds(key))));
        }
    }
    // Each key's soft and medium recordings to the hard one's loudness over
    // its first four tenths: velocity makes the level, the layer the tone.
    let hard = LAYERS.len() - 1;
    let reference: Vec<(u8, f32)> = cuts
        .iter()
        .filter(|(at, ..)| *at == hard)
        .map(|(_, key, rate, frames)| (*key, rms(frames, 0.4, *rate)))
        .collect();
    for (at, key, rate, frames) in &mut cuts {
        if *at == hard {
            continue;
        }
        let target = reference.iter().find(|(k, _)| k == key).unwrap().1;
        let gain = target / rms(frames, 0.4, *rate).max(1e-9);
        for f in frames.iter_mut() {
            f[0] *= gain;
            f[1] *= gain;
        }
    }
    // Each key's sides levelled to its place along the keyboard, the
    // power of the two together kept.
    for (_, key, rate, frames) in &mut cuts {
        let n = ((0.4 * *rate as f32) as usize).min(frames.len()).max(1);
        let side =
            |c: usize| (frames[..n].iter().map(|f| f[c] * f[c]).sum::<f32>() / n as f32).sqrt();
        let (l, r) = (side(0).max(1e-9), side(1).max(1e-9));
        let lean_db = -3.0 + 6.0 * (f32::from(*key) - 21.0) / 87.0;
        let want = 10f32.powf(lean_db / 20.0); // right over left
        let total = (l * l + r * r).sqrt();
        let l_to = total / (1.0 + want * want).sqrt();
        let r_to = l_to * want;
        let (gl, gr) = (l_to / l, r_to / r);
        for f in frames.iter_mut() {
            f[0] *= gl;
            f[1] *= gr;
        }
    }
    // One gain for everything, so the loudest moment of any cut is a
    // decibel under full scale and the keyboard's balance is the piano's.
    let peak = cuts
        .iter()
        .flat_map(|(.., frames)| frames.iter())
        .fold(0f32, |m, f| m.max(f[0].abs()).max(f[1].abs()));
    let gain = 10f32.powf(-1.0 / 20.0) / peak;

    let mut total = 0u64;
    for (at, key, rate, frames) in &cuts {
        let dir = out.join(LAYERS[*at].0);
        std::fs::create_dir_all(&dir).unwrap();
        let wav = scratch.join("cut.wav");
        let mut writer = fontelle_assets::WavWriter::create(&wav, *rate, 2).unwrap();
        let interleaved: Vec<f32> = frames
            .iter()
            .flat_map(|f| [f[0] * gain, f[1] * gain])
            .collect();
        writer.write(&interleaved).unwrap();
        writer.finish().unwrap();
        let ogg = dir.join(format!("{}.ogg", file_name(*key)));
        let status = std::process::Command::new("ffmpeg")
            .args(["-loglevel", "error", "-y", "-i"])
            .arg(&wav)
            .args(["-c:a", "libvorbis", "-q:a", "6", "-map_metadata", "-1"])
            .arg(&ogg)
            .status()
            .expect("ffmpeg runs");
        assert!(status.success(), "ffmpeg failed on {}", ogg.display());
        let size = std::fs::metadata(&ogg).unwrap().len();
        total += size;
        println!(
            "  {:<6} {:<4} {:.1}s {} kB",
            LAYERS[*at].0,
            file_name(*key),
            frames.len() as f32 / *rate as f32,
            size / 1024
        );
    }
    std::fs::remove_dir_all(&scratch).ok();
    println!("{} files, {:.1} MB", cuts.len(), total as f64 / 1e6);
}

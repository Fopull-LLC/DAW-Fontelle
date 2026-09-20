//! SFZ import (`docs/flopsynth-next.md` §4.3): the subset every library
//! uses — `<region>`, `sample`, `lokey`/`hikey`/`pitch_keycenter`,
//! `lovel`/`hivel`, `loop_mode`/`loop_start`/`loop_end`, `tune`, `volume`
//! — read into a `UserSample` whose zones carry a velocity range, so a
//! multisample with two velocity layers plays the soft one on a soft note.

use fontelle_core::{NoteTrigger, Patch, PrepareContext, SampleStore, Sampler, Source, flopsynth};
use fontelle_dsp::SynthSource;

const SR: f32 = 48_000.0;

/// A sine at `hz` for `seconds`, written as a 16-bit WAV to `path`.
fn write_tone(path: &std::path::Path, hz: f32, seconds: f32, level: f32) {
    let frames = (seconds * SR) as usize;
    let samples: Vec<f32> = (0..frames)
        .map(|i| (std::f32::consts::TAU * hz * i as f32 / SR).sin() * level)
        .collect();
    let mut writer = fontelle_assets::WavWriter::create(path, SR as u32, 1).expect("a wav");
    writer.write(&samples).expect("written");
    writer.finish().expect("finished");
}

/// Six regions: two velocity layers for each of three keys, as a hand
/// would write them — plus a `<global>` the subset ignores and comments.
fn a_library(dir: &std::path::Path) -> std::path::PathBuf {
    for (name, hz) in [("c3", 130.81), ("c4", 261.63), ("c5", 523.25)] {
        write_tone(&dir.join(format!("{name}_soft.wav")), hz, 0.5, 0.25);
        write_tone(&dir.join(format!("{name}_hard.wav")), hz, 0.5, 0.5);
    }
    let text = r#"// A hand-written library
<control> default_path=
<global> ampeg_release=0.3
<group> loop_mode=one_shot
<region> sample=c3_soft.wav lokey=0 hikey=54 pitch_keycenter=48 lovel=0 hivel=80
<region> sample=c3_hard.wav lokey=0 hikey=54 pitch_keycenter=48 lovel=81 hivel=127 volume=-3
<region> sample=c4_soft.wav lokey=55 hikey=66 pitch_keycenter=60 lovel=0 hivel=80 tune=-20
<region> sample=c4_hard.wav lokey=55 hikey=66 pitch_keycenter=60 lovel=81 hivel=127
<region> sample=c5_soft.wav lokey=67 hikey=127 pitch_keycenter=72 lovel=0 hivel=80 loop_mode=loop_continuous loop_start=1000 loop_end=9000
<region> sample=c5_hard.wav lokey=67 hikey=127 pitch_keycenter=72 lovel=81 hivel=127
"#;
    let path = dir.join("piano.sfz");
    std::fs::write(&path, text).unwrap();
    path
}

fn render(patch: Patch, key: u8, velocity: u8, frames: usize) -> Vec<f32> {
    let mut sampler = Sampler::new(patch);
    sampler.prepare(&PrepareContext {
        sample_rate: SR,
        max_block_size: 512,
    });
    sampler.trigger(NoteTrigger::new(key, velocity));
    let store = SampleStore::new();
    let mut out = Vec::with_capacity(frames);
    while out.len() < frames {
        let n = 512.min(frames - out.len());
        let mut left = vec![0.0f32; n];
        let mut right = vec![0.0f32; n];
        sampler.render(&store, &mut [&mut left[..], &mut right[..]]);
        out.extend_from_slice(&left);
    }
    out
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |a, s| a.max(s.abs()))
}

#[test]
fn a_six_region_file_reads_as_a_multisample_with_velocity_layers() {
    let dir = std::env::temp_dir().join(format!("fontelle-sfz-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = a_library(&dir);
    let sample = fontelle_assets::import_sfz(&path).expect("the file reads");
    assert_eq!(sample.name, "piano");
    assert_eq!(sample.zones.len(), 6);
    let soft_c4 = sample
        .zones
        .iter()
        .find(|z| z.name == "c4_soft")
        .expect("named after the file");
    assert_eq!(soft_c4.root_key, 60);
    assert_eq!(soft_c4.key_range, (55, 66));
    assert_eq!(soft_c4.vel_range, (0, 80));
    assert!((soft_c4.fine_cents - -20.0).abs() < 1e-6, "tune is cents");
    assert_eq!(soft_c4.sample_rate, SR as u32);
    assert_eq!(soft_c4.samples.len(), (0.5 * SR) as usize);
    let hard_c3 = sample.zones.iter().find(|z| z.name == "c3_hard").unwrap();
    assert_eq!(hard_c3.vel_range, (81, 127));
    assert!((hard_c3.gain_db - -3.0).abs() < 1e-6, "volume is decibels");
    let looped = sample.zones.iter().find(|z| z.name == "c5_soft").unwrap();
    assert_eq!(looped.loop_frames, Some((1000, 9000)));
    assert_eq!(hard_c3.loop_frames, None, "one_shot has no loop");
    // The zone for a note: by key, and by velocity within the key.
    assert_eq!(
        sample.zone_for_note(60, 40).map(|z| z.name.as_str()),
        Some("c4_soft")
    );
    assert_eq!(
        sample.zone_for_note(60, 100).map(|z| z.name.as_str()),
        Some("c4_hard")
    );
    assert_eq!(
        sample.zone_for_note(70, 100).map(|z| z.name.as_str()),
        Some("c5_hard")
    );
    // A key nobody covers still plays the nearest root, as a folder does.
    assert_eq!(sample.zone_for(60).map(|z| z.root_key), Some(60));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_patch_playing_an_sfz_plays_the_layer_the_velocity_asks_for() {
    let dir = std::env::temp_dir().join(format!("fontelle-sfz-play-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let sample = fontelle_assets::import_sfz(&a_library(&dir)).unwrap();
    let mut patch = flopsynth::flopsynth_init();
    patch.samples.push(sample);
    if let Source::Synth(osc) = &mut patch.layers[0].source {
        osc.source = SynthSource::Sample(0);
        osc.unison.voices = 1;
    }
    // The velocity's own gain is the same for both, so the two layers'
    // levels — soft 0.25, hard 0.5 — are what tells them apart: a note at
    // 100 over one at 60 is the hard recording's 6 dB plus the curve's.
    let soft = peak(&render(patch.clone(), 60, 60, 4_800));
    let hard = peak(&render(patch.clone(), 60, 100, 4_800));
    let curve = fontelle_core::velocity_to_gain(100) / fontelle_core::velocity_to_gain(60);
    let ratio = hard / soft / curve;
    assert!(
        (1.7..=2.3).contains(&ratio),
        "the hard layer is twice the soft one's level: {ratio:.2}"
    );
    // And the file round-trips with its velocity ranges and loop.
    let data = patch.to_data(&Default::default()).unwrap();
    let back = Patch::from_data(&data, |_| None).unwrap().patch;
    assert_eq!(back.samples[0].zones[1].vel_range, (81, 127));
    assert_eq!(back.samples[0].zones[4].loop_frames, Some((1000, 9000)));
    assert!((back.samples[0].zones[1].gain_db - -3.0).abs() < 1e-6);
    // Only what a zone has is written: c4_hard has a window and nothing
    // else, and a zone from a folder — the whole keyboard, no loop, no
    // trim — writes what it always wrote.
    let hard_c4 = &data.body["samples"][0]["zones"][3];
    assert!(hard_c4.get("vel_range").is_some());
    assert!(hard_c4.get("loop_frames").is_none() && hard_c4.get("gain_db").is_none());
    let mut folder = patch.clone();
    folder.samples[0].zones.truncate(1);
    folder.samples[0].zones[0].vel_range = (0, 127);
    let data = folder.to_data(&Default::default()).unwrap();
    let plain = &data.body["samples"][0]["zones"][0];
    assert!(plain.get("vel_range").is_none() && plain.get("loop_frames").is_none());
    assert!(plain.get("gain_db").is_none());
    std::fs::remove_dir_all(&dir).ok();
}

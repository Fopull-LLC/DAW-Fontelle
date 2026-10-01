//! The tour's song (`docs/ux-routing-and-learning-plan.md` §5, answer D).
//!
//! > *"the demo project can be extremely simple"* — only as much song as it
//! > takes to teach the necessary things well: a few lanes, an instrument or
//! > two, one audio clip, a mixer track with an effect and a send.
//!
//! Built here rather than shipped as a file, so it needs no download and can
//! never be out of date with the format: four bars of drums, bass and chords
//! on the factory instruments (none of which reads a file), a short pad as
//! the audio clip ([`write_pad`]), the drums through a bus with a compressor
//! and a send to a reverb track.

use fontelle_model::{
    AddChannel, AddClip, AddInsert, AddMixerTrack, AddSend, Arena, Clip, ClipSource, Command, Lane,
    Note, NoteData, Project, SetChannelKind, SetChannelRoute, TempoMap,
};
use fontelle_types::{ChannelId, EffectKind, InstrumentKind, LaneId, PPQN};

/// The song's name, and the bundle's.
pub const TOUR_NAME: &str = "Fontelle tour";
/// The row the pad lands on — the session imports it there once the song is
/// a bundle, the way a dropped file is imported.
pub const TOUR_AUDIO_LANE: &str = "Vocal";
const BPM: f64 = 100.0;
const BARS: i64 = 4;

fn note(start: i64, length: i64, key: u8, velocity: u8) -> Note {
    Note {
        start,
        length,
        key,
        velocity,
        pan: 0,
        fine_pitch: 0,
        release: 0,
        mod_x: 0,
        mod_y: 0,
        slide: false,
        channel: None,
    }
}

fn channel(
    project: &mut Project,
    name: &str,
    kind: InstrumentKind,
    patch: fontelle_core::Patch,
) -> ChannelId {
    let mut add = AddChannel::new(name, patch.to_data(&Default::default()).ok());
    add.apply(project).expect("a fresh song takes a channel");
    let id = add.channel().expect("just applied");
    SetChannelKind::new(id, kind)
        .apply(project)
        .expect("a channel that was just made takes a kind");
    id
}

fn clip(project: &mut Project, lane: LaneId, channel: ChannelId, notes: Vec<Note>) {
    let mut arena = Arena::default();
    for n in notes {
        arena.insert(n);
    }
    AddClip::new(Clip {
        lane,
        start: 0,
        length: PPQN * 4 * BARS,
        source: ClipSource::Notes(NoteData {
            channel,
            notes: arena,
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    })
    .apply(project)
    .expect("a fresh song takes a clip");
}

/// The tour's song, less its audio clip.
pub fn tour_project(sample_rate: u32) -> Project {
    let mut project = Project::new(TOUR_NAME);
    project.tempo_map = TempoMap::new(BPM, sample_rate as f64);
    let beat = PPQN;
    let bar = PPQN * 4;

    let piano = fontelle_core::flopsynth::presets::FACTORY
        .iter()
        .find(|row| row.name == crate::STARTING_PRESET)
        .map_or_else(fontelle_core::flopsynth::flopsynth_init, |row| {
            (row.build)()
        });
    let bass = fontelle_core::flopsynth::presets::FACTORY
        .iter()
        .find(|row| row.category == fontelle_core::flopsynth::presets::FlopsynthCategory::Bass)
        .map_or_else(fontelle_core::flopsynth::flopsynth_init, |row| {
            (row.build)()
        });
    let drums = channel(
        &mut project,
        "Drums",
        InstrumentKind::DrumMachine,
        fontelle_core::drum_kit(fontelle_core::DrumKitStyle::default()),
    );
    let bass = channel(&mut project, "Bass", InstrumentKind::Flopsynth, bass);
    let keys = channel(&mut project, "Keys", InstrumentKind::Flopsynth, piano);

    let mut rows = Vec::new();
    for (at, name) in ["Drums", "Bass", "Keys", TOUR_AUDIO_LANE, "Lane 5", "Lane 6"]
        .into_iter()
        .enumerate()
    {
        rows.push(project.lanes.insert(Lane {
            name: name.to_string(),
            height: 32.0,
            color: [0x4f, 0x8f, 0xd0, 0xff],
            muted: false,
            locked: false,
            soloed: false,
            order: at as u32,
        }));
    }

    // A plain beat: kick on one and three, snare on two and four, eighths on
    // the hat.
    let mut beat_notes = Vec::new();
    for b in 0..BARS {
        let at = b * bar;
        for q in 0..4 {
            let key = if q % 2 == 0 { 36 } else { 38 };
            beat_notes.push(note(at + q * beat, beat / 2, key, 110));
        }
        for e in 0..8 {
            beat_notes.push(note(
                at + e * beat / 2,
                beat / 4,
                42,
                if e % 2 == 0 { 90 } else { 70 },
            ));
        }
    }
    clip(&mut project, rows[0], drums, beat_notes);

    // C, A minor, F, G: roots on the bass, triads on the keys.
    let chords: [(u8, [u8; 3]); 4] = [
        (36, [60, 64, 67]),
        (33, [57, 60, 64]),
        (29, [53, 57, 60]),
        (31, [55, 59, 62]),
    ];
    let mut bass_notes = Vec::new();
    let mut key_notes = Vec::new();
    for (b, (root, triad)) in chords.iter().enumerate() {
        let at = b as i64 * bar;
        bass_notes.push(note(at, beat * 2 - beat / 4, *root, 100));
        bass_notes.push(note(at + beat * 2, beat * 2 - beat / 4, *root, 90));
        for key in triad {
            key_notes.push(note(at, bar - beat / 4, *key, 80));
        }
    }
    clip(&mut project, rows[1], bass, bass_notes);
    clip(&mut project, rows[2], keys, key_notes);

    // The drums through a bus with a compressor on it, and a send from the
    // bus to a reverb — the mixer's three ideas on one strip.
    let mut add = AddMixerTrack::new("Drum bus");
    add.apply(&mut project).expect("a track");
    let bus = add.track().expect("just applied");
    let mut add = AddMixerTrack::new("Reverb");
    add.apply(&mut project).expect("a track");
    let reverb = add.track().expect("just applied");
    SetChannelRoute::new(drums, Some(bus))
        .apply(&mut project)
        .expect("a route to a track that exists");
    AddInsert::new(bus, EffectKind::Compressor)
        .apply(&mut project)
        .expect("an effect");
    AddInsert::new(reverb, EffectKind::Reverb)
        .apply(&mut project)
        .expect("an effect");
    AddSend::new(bus, reverb)
        .apply(&mut project)
        .expect("a send");
    project
}

/// Writes the tour's audio clip: two bars of a soft pad (an A-minor-ish
/// cluster of sines with a slow swell), as a WAV at `path`.
pub fn write_pad(path: &std::path::Path, sample_rate: u32) -> Result<(), String> {
    let seconds = 2.0 * 4.0 * 60.0 / BPM;
    let frames = (seconds * sample_rate as f64) as usize;
    let mut writer =
        fontelle_assets::WavWriter::create(path, sample_rate, 2).map_err(|e| e.to_string())?;
    let freqs = [220.0_f64, 261.63, 329.63, 440.0];
    let mut block = Vec::with_capacity(2 * 1024);
    for start in (0..frames).step_by(1024) {
        block.clear();
        for i in start..(start + 1024).min(frames) {
            let t = i as f64 / sample_rate as f64;
            let swell = (std::f64::consts::PI * t / seconds).sin().powi(2);
            let v: f64 = freqs
                .iter()
                .enumerate()
                .map(|(k, f)| (2.0 * std::f64::consts::PI * f * t + k as f64).sin())
                .sum::<f64>()
                / freqs.len() as f64;
            let s = (v * swell * 0.35) as f32;
            block.push(s);
            block.push(s);
        }
        writer.write(&block).map_err(|e| e.to_string())?;
    }
    writer.finish().map_err(|e| e.to_string())
}

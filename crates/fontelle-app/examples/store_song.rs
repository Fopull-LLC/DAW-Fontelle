//! The song the store's screenshots and trailer are taken on (hub card 0429):
//! sixteen bars in A minor at 96 BPM, an intro, a full eight and a
//! breakdown, on the factory instruments only — so it needs no file, belongs
//! to nobody but us, and can be made again whenever the studio's look
//! changes and the pictures go stale.
//!
//! ```text
//! cargo run --release -p fontelle-app --example store_song -- "<projects dir>/Night Drive"
//! fontelle --open "<projects dir>/Night Drive"
//! ```
//!
//! The tour's song is four bars on purpose (it teaches); a store picture has
//! to look like a song somebody is making, so this one has sections, clips
//! that start and stop, a bus, two returns and an effect on most strips.

use fontelle_model::{
    AddChannel, AddClip, AddInsert, AddMixerTrack, AddSend, Arena, Clip, ClipSource, Command, Lane,
    Note, NoteData, Project, SetChannelKind, SetChannelRoute, SetSendLevel, TempoMap,
};
use fontelle_types::{ChannelId, EffectKind, InstrumentKind, LaneId, MixerTrackId, PPQN};

const NAME: &str = "Night Drive";
const BPM: f64 = 96.0;
const RATE: f64 = 48_000.0;
const BEAT: i64 = PPQN;
const BAR: i64 = PPQN * 4;

/// A minor's four: Am, F, C, G — the root for the bass, the voicing for
/// the piano and the pad.
const CHORDS: [(u8, [u8; 4]); 4] = [
    (45, [57, 60, 64, 69]),
    (41, [57, 60, 65, 69]),
    (48, [55, 60, 64, 67]),
    (43, [55, 59, 62, 67]),
];

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
        path: Vec::new(),
        channel: None,
        lyric: String::new(),
        voice: None,
        muted: false,
    }
}

fn preset(name: &str) -> fontelle_core::Patch {
    fontelle_core::flopsynth::presets::FACTORY
        .iter()
        .find(|row| row.name == name)
        .map_or_else(fontelle_core::flopsynth::flopsynth_init, |row| {
            (row.build)()
        })
}

fn channel(
    project: &mut Project,
    name: &str,
    kind: InstrumentKind,
    patch: fontelle_core::Patch,
) -> ChannelId {
    let mut add = AddChannel::new(name, patch.to_data(&Default::default()).ok());
    add.apply(project).expect("a song takes a channel");
    let id = add.channel().expect("just applied");
    SetChannelKind::new(id, kind)
        .apply(project)
        .expect("a new channel takes a kind");
    id
}

/// A clip on `lane` from bar `from` for `bars` bars, its notes counted from
/// its own start.
fn clip(
    project: &mut Project,
    lane: LaneId,
    channel: ChannelId,
    from: i64,
    bars: i64,
    name: &str,
    notes: Vec<Note>,
) {
    let mut arena = Arena::default();
    for n in notes {
        arena.insert(n);
    }
    AddClip::new(Clip {
        name: Some(name.to_string()),
        lane,
        start: from * BAR,
        length: bars * BAR,
        source: ClipSource::Notes(NoteData {
            channel,
            notes: arena,
            vocal: None,
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    })
    .apply(project)
    .expect("a song takes a clip");
}

fn track(project: &mut Project, name: &str) -> MixerTrackId {
    let mut add = AddMixerTrack::new(name);
    add.apply(project).expect("a track");
    add.track().expect("just applied")
}

/// `bars` bars of beat: kick, snare on two and four, sixteenth hats with an
/// accent on the beat, a ghost snare and an open hat at the end of each
/// second bar. `full` adds the second kick and the clap.
fn beat(bars: i64, full: bool) -> Vec<Note> {
    let mut notes = Vec::new();
    for b in 0..bars {
        let at = b * BAR;
        notes.push(note(at, BEAT / 2, 36, 118));
        notes.push(note(at + 2 * BEAT + BEAT / 2, BEAT / 2, 36, 100));
        if full {
            notes.push(note(at + 3 * BEAT + BEAT / 2, BEAT / 4, 36, 84));
            notes.push(note(at + 3 * BEAT, BEAT / 2, 39, 92));
        }
        notes.push(note(at + BEAT, BEAT / 2, 38, 112));
        notes.push(note(at + 3 * BEAT, BEAT / 2, 38, 116));
        notes.push(note(at + 2 * BEAT - BEAT / 4, BEAT / 4, 38, 46));
        for s in 0..16 {
            if b % 2 == 1 && s == 14 {
                notes.push(note(at + s * BEAT / 4, BEAT / 2, 46, 88));
                continue;
            }
            if b % 2 == 1 && s == 15 {
                continue;
            }
            let velocity = match s % 4 {
                0 => 96,
                2 => 74,
                _ => 52,
            };
            notes.push(note(at + s * BEAT / 4, BEAT / 8, 42, velocity));
        }
    }
    if full {
        notes.push(note(0, BEAT, 49, 110));
    }
    notes
}

fn main() -> Result<(), String> {
    let out = std::env::args()
        .nth(1)
        .ok_or("usage: store_song <bundle folder to write>")?;
    let out = std::path::PathBuf::from(out);

    let mut project = Project::new(NAME);
    project.tempo_map = TempoMap::new(BPM, RATE);

    let drums = channel(
        &mut project,
        "Drums",
        InstrumentKind::DrumMachine,
        fontelle_core::drum_kit(fontelle_core::DrumKitStyle::LoFi),
    );
    let bass = channel(
        &mut project,
        "Bass",
        InstrumentKind::Flopsynth,
        preset("Warm Round"),
    );
    let piano = channel(
        &mut project,
        "Piano",
        InstrumentKind::Flopsynth,
        preset("Grand Piano"),
    );
    let pad = channel(
        &mut project,
        "Pad",
        InstrumentKind::Flopsynth,
        preset("Warm Analog"),
    );
    let arp = channel(
        &mut project,
        "Arp",
        InstrumentKind::Flopsynth,
        preset("Arp Bells"),
    );
    let lead = channel(
        &mut project,
        "Lead",
        InstrumentKind::Flopsynth,
        preset("Soft Sine"),
    );

    let names = ["Drums", "Bass", "Piano", "Pad", "Arp", "Lead", "Lane 7"];
    let lanes: Vec<LaneId> = names
        .iter()
        .enumerate()
        .map(|(at, name)| {
            project.lanes.insert(Lane {
                name: name.to_string(),
                height: 40.0,
                color: [0x4f, 0x8f, 0xd0, 0xff],
                muted: false,
                locked: false,
                soloed: false,
                order: at as u32,
            })
        })
        .collect();

    // Drums: in at bar 5, the full beat for eight, the light one for the
    // breakdown.
    clip(&mut project, lanes[0], drums, 4, 8, "Beat", beat(8, true));
    clip(
        &mut project,
        lanes[0],
        drums,
        12,
        4,
        "Beat (light)",
        beat(4, false),
    );

    // Bass: the root on one, an octave push on the "and" of two, the fifth
    // walking into the next bar.
    let mut bass_notes = Vec::new();
    for b in 0..8 {
        let (root, _) = CHORDS[b as usize % 4];
        let at = b * BAR;
        bass_notes.push(note(at, BEAT + BEAT / 2, root, 108));
        bass_notes.push(note(at + BEAT + BEAT / 2, BEAT / 2, root + 12, 86));
        bass_notes.push(note(at + 2 * BEAT, BEAT, root, 98));
        bass_notes.push(note(at + 3 * BEAT, BEAT / 2, root + 7, 90));
        bass_notes.push(note(at + 3 * BEAT + BEAT / 2, BEAT / 2, root + 5, 84));
    }
    clip(&mut project, lanes[1], bass, 4, 8, "Bass line", bass_notes);

    // Piano: the whole song — held chords in the intro, a pulse in the
    // middle, the chords again at the end.
    let held = |bars: i64| {
        let mut notes = Vec::new();
        for b in 0..bars {
            let (_, voicing) = CHORDS[b as usize % 4];
            for (i, key) in voicing.iter().enumerate() {
                notes.push(note(b * BAR + i as i64 * 20, BAR - BEAT / 4, *key, 76));
            }
        }
        notes
    };
    clip(&mut project, lanes[2], piano, 0, 4, "Intro chords", held(4));
    let mut pulse = Vec::new();
    for b in 0..8 {
        let (_, voicing) = CHORDS[b as usize % 4];
        for hit in [0, 3, 6, 10, 12] {
            let at = b * BAR + hit * BEAT / 4;
            for key in &voicing[1..] {
                pulse.push(note(at, BEAT / 2, *key, if hit == 0 { 84 } else { 66 }));
            }
        }
    }
    clip(&mut project, lanes[2], piano, 4, 8, "Piano pulse", pulse);
    clip(
        &mut project,
        lanes[2],
        piano,
        12,
        4,
        "Outro chords",
        held(4),
    );

    // Pad: under everything but the intro's first two bars.
    let mut pad_notes = Vec::new();
    for b in 0..14 {
        let (root, voicing) = CHORDS[b as usize % 4];
        pad_notes.push(note(b * BAR, BAR, root + 12, 64));
        pad_notes.push(note(b * BAR, BAR, voicing[2], 58));
    }
    clip(&mut project, lanes[3], pad, 2, 14, "Pad", pad_notes);

    // Arp: sixteenths up the chord, in for the second half of the full part.
    let mut arp_notes = Vec::new();
    for b in 0..4 {
        let (_, voicing) = CHORDS[b as usize % 4];
        let run = [
            voicing[0],
            voicing[1],
            voicing[2],
            voicing[3],
            voicing[2] + 12,
            voicing[3],
            voicing[2],
            voicing[1],
        ];
        for s in 0..16 {
            arp_notes.push(note(
                b * BAR + s * BEAT / 4,
                BEAT / 4,
                run[s as usize % 8] + 12,
                70 + (s % 4 == 0) as u8 * 20,
            ));
        }
    }
    clip(&mut project, lanes[4], arp, 8, 4, "Arp", arp_notes);

    // Lead: a tune in the pentatonic, two four-bar phrases.
    let phrase: [(i64, i64, u8); 14] = [
        (0, 6, 76),
        (6, 2, 74),
        (8, 4, 72),
        (12, 4, 69),
        (16, 8, 72),
        (24, 4, 74),
        (28, 4, 76),
        (32, 6, 79),
        (38, 2, 76),
        (40, 8, 74),
        (48, 4, 72),
        (52, 4, 74),
        (56, 6, 69),
        (62, 2, 67),
    ];
    let mut lead_notes = Vec::new();
    for half in 0..2 {
        for (at, length, key) in phrase {
            let shift = if half == 1 && at >= 56 { 3 } else { 0 };
            lead_notes.push(note(
                half * 4 * BAR + at * BEAT / 4,
                length * BEAT / 4 - BEAT / 16,
                key + shift,
                92,
            ));
        }
    }
    clip(&mut project, lanes[5], lead, 4, 8, "Tune", lead_notes);

    // The mixer: drums through a bus with a compressor, a reverb and a delay
    // return, an effect on most strips, a limiter on the master.
    let bus = track(&mut project, "Drum bus");
    let mut routes = vec![(drums, bus)];
    let mut strips = Vec::new();
    for (channel, name) in [
        (bass, "Bass"),
        (piano, "Piano"),
        (pad, "Pad"),
        (arp, "Arp"),
        (lead, "Lead"),
    ] {
        let strip = track(&mut project, name);
        routes.push((channel, strip));
        strips.push(strip);
    }
    let reverb = track(&mut project, "Reverb");
    let delay = track(&mut project, "Delay");
    for (channel, strip) in &routes {
        SetChannelRoute::new(*channel, Some(*strip))
            .apply(&mut project)
            .expect("a route to a track that exists");
        let color = project.mixer.tracks.get(*strip).expect("just made").color;
        if let Some(ch) = project.channels.get_mut(*channel) {
            ch.color = color;
        }
    }
    let inserts = [
        (bus, EffectKind::Compressor),
        (strips[0], EffectKind::Eq),
        (strips[1], EffectKind::Eq),
        (strips[2], EffectKind::Chorus),
        (strips[3], EffectKind::Filter),
        (strips[4], EffectKind::Delay),
        (reverb, EffectKind::Reverb),
        (delay, EffectKind::Delay),
    ];
    for (strip, kind) in inserts {
        AddInsert::new(strip, kind)
            .apply(&mut project)
            .expect("an effect");
    }
    if let Some(master) = project.mixer.master {
        AddInsert::new(master, EffectKind::Limiter)
            .apply(&mut project)
            .expect("an effect on the master");
    }
    for (from, to, db) in [
        (bus, reverb, -18.0),
        (strips[1], reverb, -12.0),
        (strips[2], reverb, -9.0),
        (strips[4], reverb, -10.0),
        (strips[3], delay, -14.0),
        (strips[4], delay, -16.0),
    ] {
        let at = project.mixer.tracks.get(from).map_or(0, |t| t.sends.len());
        AddSend::new(from, to).apply(&mut project).expect("a send");
        SetSendLevel::new(from, at, db)
            .apply(&mut project)
            .expect("the send just made");
    }

    for (lane, channel) in lanes.iter().zip([drums, bass, piano, pad, arp, lead]) {
        let color = project.channels.get(channel).expect("made above").color;
        if let Some(lane) = project.lanes.get_mut(*lane) {
            lane.color = color;
        }
    }

    fontelle_app::save_project(&project, &out).map_err(|e| e.to_string())?;
    println!("wrote {}", out.display());
    Ok(())
}

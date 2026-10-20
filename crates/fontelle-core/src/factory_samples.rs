//! The recordings the factory bank ships, compiled into the binary.
//!
//! > *"maybe you could use the osc sampling feature to make the piano sound
//! > more realistic if you can find a grand piano one shot to use."* — Ty,
//! > 2026-09-16
//!
//! > *"use flopsynths new sampling features to make a variety of new complex
//! > presets that can be experimental, synthy, modulating, instruments,
//! > percussion kits, growls, dubstep sounds"* — Ty, later the same day
//!
//! A `SynthSource::Sample` oscillator plays one of the patch's own
//! recordings (`Patch::samples`), which the patch carries whole so a preset
//! made from a drop opens anywhere. The factory Grand Piano plays a sampled
//! grand — forty-six recordings of a Yamaha C5, six megabytes — and a patch
//! that *carried* those would make every project with a piano in it six
//! megabytes of base64 and the shipped preset file the same. So a recording
//! the bank ships is named, not carried: `UserSample::factory` says which
//! set it is, the patch file stores the name, and this module is the
//! set. The audio is decoded from the WAVs under `assets/flopsynth/samples/`
//! the first time a set is asked for and shared from then on — a project
//! with four piano channels holds one piano.
//!
//! The grand's recordings are the Salamander Grand Piano (Alexander Holm,
//! CC BY 3.0); `assets/flopsynth/samples/grand/README.md` is the credit and
//! says how they were cut (`fontelle-app/examples/grand_samples.rs`). The
//! **kits** are this program's own drum machine, every General MIDI hit of
//! its Studio and 808 kits played once through the kit's bus
//! (`fontelle-app/examples/kit_samples.rs`) — one recording per key, each
//! named as the roll names it, so a preset can be a kit and a zone lock
//! can make one hit an instrument.
//!
//! A new set needs an entry in [`crate::bundled_sounds`] before it
//! compiles (`FactorySampleSet::bundled` is an exhaustive match): whose it
//! is, and a licence that lets us ship it.

use std::sync::{Arc, OnceLock};

use crate::patch::{SampleZone, UserSample, key_ranges};

/// One set of recordings in the binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FactorySampleSet {
    /// The grand played softly (velocity 40): the layer a light touch gets.
    GrandSoft,
    /// The grand played hard (velocity 120).
    GrandHard,
    /// The drum machine's Studio kit: acoustic, every GM hit on its own key.
    KitStudio,
    /// The drum machine's 808.
    Kit808,
    /// The grand's left microphone: every key it was recorded at, at three
    /// strengths — what the Grand Piano plays on its left.
    GrandLeft,
    /// Its right microphone, laid out zone for zone as the left is.
    GrandRight,
}

impl FactorySampleSet {
    pub const ALL: [Self; 6] = [
        Self::GrandSoft,
        Self::GrandHard,
        Self::KitStudio,
        Self::Kit808,
        Self::GrandLeft,
        Self::GrandRight,
    ];

    /// The name a patch file stores. Stable: a file written by this build
    /// names the set by it, so it can never change without a migration.
    pub fn id(self) -> &'static str {
        match self {
            Self::GrandSoft => "grand-soft",
            Self::GrandHard => "grand-hard",
            Self::KitStudio => "kit-studio",
            Self::Kit808 => "kit-808",
            Self::GrandLeft => "grand-left",
            Self::GrandRight => "grand-right",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|set| set.id() == id)
    }

    /// What the window calls it.
    pub fn label(self) -> &'static str {
        match self {
            Self::GrandSoft => "Grand (soft)",
            Self::GrandHard => "Grand (hard)",
            Self::KitStudio => "Studio kit",
            Self::Kit808 => "808 kit",
            Self::GrandLeft => "Grand (left)",
            Self::GrandRight => "Grand (right)",
        }
    }

    /// The set as a recording a patch can play. Decoded once per process;
    /// the zones' audio is shared, so this is a handful of `Arc` clones.
    ///
    /// Each set on its own, the first time it is asked for: the stereo grand
    /// is ninety Vorbis files and a second or so of work, which a project
    /// with no piano in it should never pay.
    pub fn sample(self) -> UserSample {
        static SETS: [OnceLock<UserSample>; FactorySampleSet::ALL.len()] =
            [const { OnceLock::new() }; FactorySampleSet::ALL.len()];
        SETS[self as usize].get_or_init(|| self.decode()).clone()
    }

    fn decode(self) -> UserSample {
        let zones = match self {
            Self::GrandSoft => Self::keyboard(grand::SOFT),
            Self::GrandHard => Self::keyboard(grand::HARD),
            Self::KitStudio => Self::kit(kit::STUDIO),
            Self::Kit808 => Self::kit(kit::EIGHT_OH_EIGHT),
            Self::GrandLeft => grand_stereo::zones().0.clone(),
            Self::GrandRight => grand_stereo::zones().1.clone(),
        };
        UserSample {
            name: self.label().to_string(),
            factory: Some(self),
            zones,
        }
    }

    /// An instrument sampled every few keys: the zones tile the keyboard
    /// between their roots, so every key is a recording transposed by at
    /// most a couple of semitones.
    fn keyboard(files: &[(u8, &[u8])]) -> Vec<SampleZone> {
        let mut zones: Vec<SampleZone> = files
            .iter()
            .filter_map(|(root, wav)| {
                let (sample_rate, samples) = read_wav_mono16(wav)?;
                Some(SampleZone {
                    name: String::new(),
                    root_key: *root,
                    fine_cents: 0.0,
                    key_range: (0, 127),
                    sample_rate,
                    samples: crate::ZoneAudio::F32(Arc::from(samples)),
                    vel_range: (0, 127),
                    gain_db: 0.0,
                    loop_frames: None,
                })
            })
            .collect();
        zones.sort_by_key(|zone| zone.root_key);
        let roots: Vec<u8> = zones.iter().map(|zone| zone.root_key).collect();
        for (zone, range) in zones.iter_mut().zip(key_ranges(&roots)) {
            zone.key_range = range;
        }
        zones
    }

    /// A kit: one hit per key, each its own key alone and named for the
    /// roll — the shape `drum_kit` gives a kit's layers, which is what lets
    /// `fontelle_app::key_map` write the names on the rows.
    fn kit(files: &[(&str, &[u8])]) -> Vec<SampleZone> {
        crate::GM_DRUM_MAP
            .iter()
            .zip(files)
            .filter_map(|(slot, (name, wav))| {
                debug_assert_eq!(slot.name, *name, "the kit's files are in GM order");
                let (sample_rate, samples) = read_wav_mono16(wav)?;
                Some(SampleZone {
                    name: slot.name.to_string(),
                    root_key: slot.key,
                    fine_cents: 0.0,
                    key_range: (slot.key, slot.key),
                    sample_rate,
                    samples: crate::ZoneAudio::F32(Arc::from(samples)),
                    vel_range: (0, 127),
                    gain_db: 0.0,
                    loop_frames: None,
                })
            })
            .collect()
    }
}

/// A sixteen-bit mono RIFF WAV — the one shape the files under `assets`
/// are, written by `fontelle_assets::WavWriter`. Anything else is `None`,
/// and the zone is left out rather than played wrong.
fn read_wav_mono16(bytes: &[u8]) -> Option<(u32, Vec<f32>)> {
    let u16_at = |at: usize| Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?));
    let u32_at = |at: usize| Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?));
    if bytes.get(0..4)? != b"RIFF" || bytes.get(8..12)? != b"WAVE" {
        return None;
    }
    let mut at = 12;
    let mut format = None;
    while at + 8 <= bytes.len() {
        let id = bytes.get(at..at + 4)?;
        let size = u32_at(at + 4)? as usize;
        let body = at + 8;
        match id {
            b"fmt " => {
                let channels = u16_at(body + 2)?;
                let rate = u32_at(body + 4)?;
                let bits = u16_at(body + 14)?;
                if channels != 1 || bits != 16 {
                    return None;
                }
                format = Some(rate);
            }
            b"data" => {
                let rate = format?;
                let data = bytes.get(body..body + size)?;
                let samples = data
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| f32::from(i16::from_le_bytes(*pair)) / 32_768.0)
                    .collect();
                return Some((rate, samples));
            }
            _ => {}
        }
        // Chunks are word-aligned.
        at = body + size + (size & 1);
    }
    None
}

/// The grand's files, one per key: the bottom A and every four semitones
/// from C1 to C8, so middle C is a recording.
mod grand {
    macro_rules! layer {
        ($dir:literal: $(($key:literal, $name:literal)),* $(,)?) => {
            &[$(($key, include_bytes!(concat!(
                "../../../assets/flopsynth/samples/grand/", $dir, "/", $name, ".wav"
            )))),*]
        };
    }
    macro_rules! keys {
        ($dir:literal) => {
            layer!($dir:
                (21, "A0"), (24, "C1"), (28, "E1"), (32, "Gs1"), (36, "C2"),
                (40, "E2"), (44, "Gs2"), (48, "C3"), (52, "E3"), (56, "Gs3"),
                (60, "C4"), (64, "E4"), (68, "Gs4"), (72, "C5"), (76, "E5"),
                (80, "Gs5"), (84, "C6"), (88, "E6"), (92, "Gs6"), (96, "C7"),
                (100, "E7"), (104, "Gs7"), (108, "C8"),
            )
        };
    }
    pub(super) static SOFT: &[(u8, &[u8])] = keys!("soft");
    pub(super) static HARD: &[(u8, &[u8])] = keys!("hard");
}

/// The kits' files, one per General MIDI hit in `GM_DRUM_MAP`'s order,
/// named as `kit_samples` names them (the roll's name, spaces as dashes).
mod kit {
    macro_rules! kit {
        ($dir:literal: $(($name:literal, $file:literal)),* $(,)?) => {
            &[$(($name, include_bytes!(concat!(
                "../../../assets/flopsynth/samples/kit/", $dir, "/", $file, ".wav"
            )))),*]
        };
    }
    macro_rules! hits {
        ($dir:literal) => {
            kit!($dir:
                ("Kick 2", "Kick-2"), ("Kick", "Kick"), ("Rim", "Rim"),
                ("Snare", "Snare"), ("Clap", "Clap"), ("Snare 2", "Snare-2"),
                ("Floor Tom", "Floor-Tom"), ("Closed Hat", "Closed-Hat"),
                ("Tom Low", "Tom-Low"), ("Pedal Hat", "Pedal-Hat"),
                ("Tom Mid", "Tom-Mid"), ("Open Hat", "Open-Hat"),
                ("Tom Mid 2", "Tom-Mid-2"), ("Tom High", "Tom-High"),
                ("Crash", "Crash"), ("Tom Top", "Tom-Top"), ("Ride", "Ride"),
                ("China", "China"), ("Ride Bell", "Ride-Bell"),
                ("Tambourine", "Tambourine"), ("Splash", "Splash"),
                ("Cowbell", "Cowbell"), ("Crash 2", "Crash-2"),
                ("Vibraslap", "Vibraslap"), ("Ride 2", "Ride-2"),
                ("Bongo High", "Bongo-High"), ("Bongo Low", "Bongo-Low"),
                ("Conga Mute", "Conga-Mute"), ("Conga High", "Conga-High"),
                ("Conga Low", "Conga-Low"), ("Timbale High", "Timbale-High"),
                ("Timbale Low", "Timbale-Low"), ("Agogo High", "Agogo-High"),
                ("Agogo Low", "Agogo-Low"), ("Cabasa", "Cabasa"),
                ("Maraca", "Maraca"),
            )
        };
    }
    pub(super) static STUDIO: &[(&str, &[u8])] = hits!("studio");
    pub(super) static EIGHT_OH_EIGHT: &[(&str, &[u8])] = hits!("808");
}

/// The stereo grand: the Salamander C5's own recordings, every key it was
/// sampled at (the bottom A and each minor third up), at three strengths —
/// its layers 3, 7 and 14 of sixteen — as `grand_stereo_samples` cuts them
/// into `assets/flopsynth/samples/grand-stereo/`. One Ogg Vorbis file per
/// key and strength holds both microphones; decoding it once gives the
/// left set and the right, laid out alike.
mod grand_stereo {
    use std::sync::{Arc, OnceLock};

    use crate::patch::{SampleZone, ZoneAudio, key_ranges};

    /// The velocities each strength answers: where Salamander's own split
    /// falls between the layers this cut keeps.
    pub(super) const STRENGTHS: [(&str, (u8, u8)); 3] =
        [("soft", (0, 44)), ("medium", (45, 80)), ("hard", (81, 127))];

    macro_rules! strength {
        ($dir:literal) => {
            strength!(@ $dir:
                (21, "A0"), (24, "C1"), (27, "Ds1"), (30, "Fs1"), (33, "A1"), (36, "C2"),
                (39, "Ds2"), (42, "Fs2"), (45, "A2"), (48, "C3"), (51, "Ds3"), (54, "Fs3"),
                (57, "A3"), (60, "C4"), (63, "Ds4"), (66, "Fs4"), (69, "A4"), (72, "C5"),
                (75, "Ds5"), (78, "Fs5"), (81, "A5"), (84, "C6"), (87, "Ds6"), (90, "Fs6"),
                (93, "A6"), (96, "C7"), (99, "Ds7"), (102, "Fs7"), (105, "A7"), (108, "C8"),
            )
        };
        (@ $dir:literal: $(($key:literal, $name:literal)),* $(,)?) => {
            &[$(($key, include_bytes!(concat!(
                "../../../assets/flopsynth/samples/grand-stereo/", $dir, "/", $name, ".ogg"
            )) as &[u8])),*]
        };
    }

    static FILES: [&[(u8, &[u8])]; 3] = [strength!("soft"), strength!("medium"), strength!("hard")];

    /// Both sides' zones, decoded on as many threads as there are cores.
    pub(super) fn zones() -> &'static (Vec<SampleZone>, Vec<SampleZone>) {
        static ZONES: OnceLock<(Vec<SampleZone>, Vec<SampleZone>)> = OnceLock::new();
        ZONES.get_or_init(|| {
            let jobs: Vec<(usize, u8, &[u8])> = FILES
                .iter()
                .enumerate()
                .flat_map(|(strength, files)| {
                    files.iter().map(move |(key, ogg)| (strength, *key, *ogg))
                })
                .collect();
            let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
            let chunk = jobs.len().div_ceil(threads.max(1));
            let decoded: Vec<Option<Decoded>> = std::thread::scope(|scope| {
                let handles: Vec<_> = jobs
                    .chunks(chunk.max(1))
                    .map(|part| {
                        scope.spawn(move || {
                            part.iter()
                                .map(|(_, _, ogg)| decode_stereo(ogg))
                                .collect::<Vec<_>>()
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .flat_map(|h| h.join().unwrap_or_default())
                    .collect()
            });
            let mut left = Vec::new();
            let mut right = Vec::new();
            for ((strength, key, _), audio) in jobs.iter().zip(decoded) {
                let Some((rate, l, r)) = audio else { continue };
                let zone = |samples: Vec<i16>| SampleZone {
                    name: String::new(),
                    root_key: *key,
                    fine_cents: 0.0,
                    key_range: (0, 127),
                    sample_rate: rate,
                    samples: ZoneAudio::I16(Arc::from(samples)),
                    vel_range: STRENGTHS[*strength].1,
                    gain_db: 0.0,
                    loop_frames: None,
                };
                left.push(zone(l));
                right.push(zone(r));
            }
            for side in [&mut left, &mut right] {
                side.sort_by_key(|zone| (zone.root_key, zone.vel_range.0));
                let mut roots: Vec<u8> = side.iter().map(|zone| zone.root_key).collect();
                roots.dedup();
                let ranges = key_ranges(&roots);
                for zone in side.iter_mut() {
                    let at = roots.iter().position(|r| *r == zone.root_key).unwrap_or(0);
                    zone.key_range = ranges[at];
                }
            }
            (left, right)
        })
    }

    /// A file's rate and its two channels as sixteen-bit frames.
    type Decoded = (u32, Vec<i16>, Vec<i16>);

    /// One file, decoded.
    fn decode_stereo(ogg: &[u8]) -> Option<Decoded> {
        use symphonia::core::codecs::audio::AudioDecoderOptions;
        use symphonia::core::formats::probe::Hint;
        use symphonia::core::formats::{FormatOptions, TrackType};
        use symphonia::core::io::MediaSourceStream;
        use symphonia::core::meta::MetadataOptions;

        let stream = MediaSourceStream::new(
            Box::new(std::io::Cursor::new(ogg.to_vec())),
            Default::default(),
        );
        let mut hint = Hint::new();
        hint.with_extension("ogg");
        let mut format = symphonia::default::get_probe()
            .probe(
                &hint,
                stream,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .ok()?;
        let track = format.default_track(TrackType::Audio)?;
        let track_id = track.id;
        let params = track.codec_params.as_ref()?.audio()?.clone();
        let rate = params.sample_rate?;
        let channels = params.channels.as_ref()?.count();
        let mut decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&params, &AudioDecoderOptions::default())
            .ok()?;
        let (mut left, mut right) = (Vec::new(), Vec::new());
        let mut buffer: Vec<f32> = Vec::new();
        while let Ok(Some(packet)) = format.next_packet() {
            if packet.track_id != track_id {
                continue;
            }
            let Ok(decoded) = decoder.decode(&packet) else {
                continue;
            };
            buffer.resize(decoded.samples_interleaved(), 0.0);
            decoded.copy_to_slice_interleaved(&mut buffer);
            for frame in buffer.chunks_exact(channels.max(1)) {
                let to_i16 = |v: f32| (v * 32_768.0).round().clamp(-32_768.0, 32_767.0) as i16;
                left.push(to_i16(frame[0]));
                right.push(to_i16(*frame.get(1).unwrap_or(&frame[0])));
            }
        }
        (!left.is_empty()).then_some((rate, left, right))
    }
}

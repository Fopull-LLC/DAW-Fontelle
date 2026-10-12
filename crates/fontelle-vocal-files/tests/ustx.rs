//! OpenUtau's `.ustx`: tracks and voice parts, notes and lyrics, the glide
//! points, vibrato, VEL, phoneme overrides, curves to lanes, tempo and time
//! signature — and back out, equal within what the file can say.

mod common;

use std::collections::BTreeMap;

use common::{assert_near, lane};
use fontelle_model::{LaneCurve, NoteVoice, TempoSegment};
use fontelle_sing::{GlideShape, Lane, PhonemeEdit, Portamento, Vibrato};
use fontelle_vocal_files::{SongPart, VocalSong, ustx, vocal_note};

const DEMO: &str = r#"name: Demo
comment: ''
ustx_version: '0.6'
resolution: 480
bpm: 120
beat_per_bar: 3
beat_unit: 4
time_signatures:
- bar_position: 0
  beat_per_bar: 3
  beat_unit: 4
tempos:
- position: 0
  bpm: 120
- position: 1920
  bpm: 90
tracks:
- singer: Somebody
  phonemizer: OpenUtau.Core.DefaultPhonemizer
  track_name: Lead
voice_parts:
- name: Verse
  comment: ''
  track_no: 0
  position: 480
  duration: 3840
  notes:
  - position: 0
    duration: 480
    tone: 60
    lyric: la
    pitch:
      data:
      - {x: -40, y: 0, shape: io}
      - {x: 40, y: 0, shape: io}
      snap_first: true
    vibrato: {length: 0, period: 175, depth: 25, in: 10, out: 10, shift: 0, drift: 0}
    phoneme_expressions: []
    phoneme_overrides: []
  - position: 480
    duration: 960
    tone: 64
    lyric: +
    pitch:
      data:
      - {x: -60, y: 0, shape: l}
      - {x: 0, y: 0, shape: l}
      snap_first: true
    vibrato: {length: 60, period: 200, depth: 30, in: 20, out: 25, shift: 0, drift: 0}
    phoneme_expressions:
    - {index: 0, abbr: vel, value: 200}
    phoneme_overrides:
    - {index: 0, phoneme: l}
    - {index: 1, phoneme: a, offset: 30}
  curves:
  - abbr: dyn
    xs: [0, 480, 960]
    ys: [0, -60, 0]
  - abbr: pitd
    xs: [500, 505, 510]
    ys: [0, 50, 0]
  - abbr: ten
    xs: [0, 960]
    ys: [50, 50]
  - abbr: xyz
    xs: [0, 480]
    ys: [5, 7]
wave_parts: []
"#;

#[test]
fn a_voice_part_lands_in_song_ticks_with_its_tempo() {
    let song = ustx::read(DEMO, "file").unwrap();
    assert_eq!(song.name, "Demo");
    assert_eq!(song.beats_per_bar, 3);
    let tempo: Vec<(i64, f64)> = song.tempo.iter().map(|s| (s.start_tick, s.bpm)).collect();
    assert_eq!(tempo, vec![(0, 120.0), (3840, 90.0)]);
    assert_eq!(song.parts.len(), 1);
    let part = &song.parts[0];
    assert_eq!(part.name, "Lead");
    assert_eq!(part.singer.as_deref(), Some("Somebody"));
    let notes: Vec<(i64, i64, u8, &str)> = part
        .notes
        .iter()
        .map(|n| (n.start, n.length, n.key, n.lyric.as_str()))
        .collect();
    // The part starts a beat in; OpenUtau's "+" is the slur, "-" here.
    assert_eq!(notes, vec![(960, 960, 60, "la"), (1920, 1920, 64, "-")]);
}

#[test]
fn glide_points_vibrato_and_vel_are_the_notes_voice() {
    let song = ustx::read(DEMO, "file").unwrap();
    let part = &song.parts[0];
    // OpenUtau's own default glide is the backend's own here.
    assert!(
        part.notes[0]
            .voice
            .as_ref()
            .is_none_or(|v| v.portamento.is_none() && v.vibrato.is_none())
    );
    let voice = part.notes[1].voice.as_ref().expect("a voice");
    let glide = voice.portamento.expect("a glide");
    assert_near("glide length", glide.length, 0.06, 1e-4);
    assert_near("ends on the note", glide.position, 0.0, 1e-4);
    assert_eq!(glide.shape, GlideShape::Linear);
    let v = voice.vibrato.expect("vibrato");
    assert_near("delay", v.delay, 0.4, 1e-4);
    assert_near("rate", v.rate, 5.0, 1e-4);
    assert_near("depth", v.depth, 30.0, 1e-4);
    assert_near("in", v.fade_in, 0.2, 1e-4);
    assert_near("out", v.fade_out, 0.25, 1e-4);
    // VEL 200 is twice the consonant speed.
    assert_eq!(part.notes[1].velocity, 127);
}

#[test]
fn phoneme_overrides_are_hand_placed_phonemes() {
    let song = ustx::read(DEMO, "file").unwrap();
    let phonemes = song.parts[0].notes[1]
        .voice
        .as_ref()
        .and_then(|v| v.phonemes.clone())
        .expect("phonemes");
    let symbols: Vec<&str> = phonemes.iter().map(|p| p.symbol.as_str()).collect();
    assert_eq!(symbols, vec!["l", "a"]);
    // An override's offset moves a phoneme from where the phonemizer put
    // it; without that phonemizer there is nothing to move it from.
    assert!(
        song.unmapped.iter().any(|u| u.contains("timing")),
        "{:?}",
        song.unmapped
    );
}

#[test]
fn curves_are_lanes_and_unknown_ones_are_kept() {
    let song = ustx::read(DEMO, "file").unwrap();
    let part = &song.parts[0];
    // dyn is tenths of a dB; part ticks from the part's start.
    assert_near("dyn", lane(part, Lane::Dynamics, 1920), -6.0, 1e-3);
    assert_near("pitd", lane(part, Lane::PitchBend, 1970), 50.0, 1e-3);
    assert_near("ten", lane(part, Lane::Tension, 1500), 0.5, 1e-3);
    let xyz = part
        .vocal
        .extra
        .get("openutau")
        .and_then(|lanes| lanes.get("xyz"))
        .expect("an unknown curve is kept for OpenUtau");
    assert_near("xyz", xyz.at(960, 0.0), 5.0, 1e-3);
}

/// A part with everything USTX can say about it.
fn full_song() -> VocalSong {
    let mut song = VocalSong::new("Round");
    song.tempo = vec![
        TempoSegment {
            start_tick: 0,
            bpm: 100.0,
        },
        TempoSegment {
            start_tick: 7680,
            bpm: 140.0,
        },
    ];
    let mut part = SongPart::new("Vox");
    let mut a = vocal_note(960, 960, 60, "ka");
    a.velocity = 80;
    a.voice = Some(Box::new(NoteVoice {
        phonemes: Some(vec![
            PhonemeEdit {
                symbol: "k".into(),
                offset: None,
            },
            PhonemeEdit {
                symbol: "a".into(),
                offset: None,
            },
        ]),
        vibrato: Some(Vibrato {
            depth: 35.0,
            rate: 6.0,
            delay: 0.3,
            fade_in: 0.25,
            fade_out: 0.1,
        }),
        portamento: Some(Portamento {
            length: 0.1,
            position: 0.5,
            shape: GlideShape::S,
        }),
        options: BTreeMap::new(),
    }));
    let b = vocal_note(1920, 1920, 64, "na-");
    let c = vocal_note(3840, 960, 62, "-");
    let d = vocal_note(8640, 480, 67, "ru");
    part.notes = vec![a, b, c, d];
    let lanes = &mut part.vocal.lanes;
    lanes.insert(
        Lane::Dynamics,
        LaneCurve::from_points(vec![(960, 0.0), (1920, -3.0), (2880, 0.0)]),
    );
    lanes.insert(
        Lane::PitchBend,
        LaneCurve::from_points(vec![(1920, 0.0), (2000, 40.0), (2080, 0.0)]),
    );
    lanes.insert(
        Lane::Breathiness,
        LaneCurve::from_points(vec![(960, 0.3), (3840, 0.5)]),
    );
    lanes.insert(Lane::Tension, LaneCurve::from_points(vec![(960, -0.4)]));
    lanes.insert(Lane::Voicing, LaneCurve::from_points(vec![(960, -0.2)]));
    lanes.insert(Lane::Gender, LaneCurve::from_points(vec![(960, 120.0)]));
    part.vocal.extra.insert(
        "openutau".into(),
        BTreeMap::from([(
            "xyz".to_string(),
            LaneCurve::from_points(vec![(960, 3.0), (1920, 9.0)]),
        )]),
    );
    song.parts.push(part);
    song
}

#[test]
fn written_and_read_back_it_is_the_same_song() {
    let song = full_song();
    let text = ustx::write(&song);
    let back = ustx::read(&text, "x").unwrap_or_else(|e| panic!("{e}\n{text}"));
    let tempo: Vec<(i64, f64)> = back.tempo.iter().map(|s| (s.start_tick, s.bpm)).collect();
    assert_eq!(tempo, vec![(0, 100.0), (7680, 140.0)]);
    assert_eq!(back.beats_per_bar, 4);
    assert_eq!(back.parts.len(), 1);
    let (want, got) = (&song.parts[0], &back.parts[0]);
    assert_eq!(got.name, "Vox");
    assert_eq!(got.notes.len(), want.notes.len());
    for (w, g) in want.notes.iter().zip(&got.notes) {
        assert_eq!((g.start, g.length, g.key), (w.start, w.length, w.key));
        assert_eq!(g.lyric, w.lyric);
        assert!(
            (i32::from(g.velocity) - i32::from(w.velocity)).abs() <= 1,
            "velocity {} vs {}",
            g.velocity,
            w.velocity
        );
    }
    let voice = got.notes[0].voice.as_ref().expect("voice");
    let want_voice = want.notes[0].voice.as_ref().unwrap();
    let (v, wv) = (voice.vibrato.unwrap(), want_voice.vibrato.unwrap());
    assert_near("depth", v.depth, wv.depth, 0.5);
    assert_near("rate", v.rate, wv.rate, 0.05);
    assert_near("delay", v.delay, wv.delay, 0.01);
    assert_near("in", v.fade_in, wv.fade_in, 0.01);
    assert_near("out", v.fade_out, wv.fade_out, 0.01);
    let (p, wp) = (voice.portamento.unwrap(), want_voice.portamento.unwrap());
    assert_near("glide", p.length, wp.length, 0.002);
    assert_near("glide position", p.position, wp.position, 0.02);
    assert_eq!(p.shape, wp.shape);
    assert_eq!(voice.phonemes, want_voice.phonemes);
    for t in [960, 1500, 1920, 2000, 2040, 2880, 3000, 3840] {
        for (l, tolerance) in [
            (Lane::Dynamics, 0.06),
            (Lane::PitchBend, 1.0),
            (Lane::Breathiness, 0.011),
            (Lane::Tension, 0.011),
            (Lane::Voicing, 0.011),
            (Lane::Gender, 6.1),
        ] {
            assert_near(
                &format!("{l:?} at {t}"),
                lane(got, l, t),
                lane(want, l, t),
                tolerance,
            );
        }
    }
    let xyz = got.vocal.extra["openutau"]["xyz"].clone();
    assert_near("xyz", xyz.at(1440, 0.0), 6.0, 0.6);
}

#[test]
fn an_older_ustx_with_one_bpm_reads() {
    let song = ustx::read(
        "name: Old\nresolution: 480\nbpm: 150\nbeat_per_bar: 4\nbeat_unit: 4\ntracks:\n- singer: X\nvoice_parts:\n- name: P\n  track_no: 0\n  position: 0\n  notes:\n  - {position: 0, duration: 240, tone: 70, lyric: a}\n",
        "x",
    )
    .unwrap();
    assert_eq!(song.tempo[0].bpm, 150.0);
    assert_eq!(song.parts[0].notes[0].length, 480);
    assert_eq!(song.parts[0].name, "P");
}

#[test]
fn garbage_says_so() {
    assert!(ustx::read("{{{{", "x").is_err());
    assert!(ustx::read("name: nothing\n", "x").is_err());
}

/// `vel` on a later phoneme of a note (OpenUtau sets one per phoneme) is
/// what is left out, and the toast says that — "the per-note expression
/// vel" read as if consonant speed itself were not brought in (found
/// importing the demo song shipped with Joule's bank).
#[test]
fn vel_on_a_later_phoneme_is_named_as_that() {
    let text = DEMO.replace(
        "    - {index: 0, abbr: vel, value: 200}",
        "    - {index: 0, abbr: vel, value: 200}\n    - {index: 2, abbr: vel, value: 60}",
    );
    let song = ustx::read(&text, "file").unwrap();
    let said = song.unmapped.join("; ");
    assert!(said.contains("later phoneme"), "{said}");
    assert!(!said.contains("expression \u{201c}vel"), "{said}");
}

//! What a phrase carries beyond notes and words: each note's expression
//! (phonemes, vibrato, glide, consonant speed, the singing backend's own
//! options) and the part's lanes, sampled into the phrase's own time —
//! only the lanes drawn, only the backend's own extras, settings and state
//! — and how all of it reaches the render key.

mod common;

use std::collections::BTreeMap;

use common::{BEAT, note, song};
use fontelle_model::{ClipSource, LaneCurve, NoteVoice, VocalPart, VocalVoice};
use fontelle_sing::{
    Capabilities, GlideShape, Lane, LaneInfo, PhonemeEdit, Portamento, Quality, Vibrato,
};
use fontelle_vocal::{CURVE_LEAD, phrases_of, phrases_with, render_key};

const BACKEND: &str = "Engine";

fn part_mut(s: &mut common::Song) -> &mut VocalPart {
    let ClipSource::Notes(data) = &mut s.project.clips[s.clip].source else {
        panic!("a note clip");
    };
    data.vocal.get_or_insert_with(Default::default)
}

fn vibrato() -> Vibrato {
    Vibrato {
        depth: 50.0,
        rate: 6.0,
        delay: 0.2,
        fade_in: 0.1,
        fade_out: 0.1,
    }
}

#[test]
fn a_notes_expression_rides_on_its_phrase_note() {
    let mut s = song(
        "en",
        vec![note(0, BEAT, 60, "la"), note(BEAT, BEAT, 62, "lo")],
    );
    let ids = s.notes.clone();
    {
        let notes = s.notes_mut();
        notes[ids[1]].velocity = 127;
        notes[ids[1]].voice = Some(Box::new(NoteVoice {
            phonemes: Some(vec![PhonemeEdit {
                symbol: "l".into(),
                offset: Some(-0.03),
            }]),
            vibrato: Some(vibrato()),
            portamento: Some(Portamento {
                length: 0.1,
                position: 0.5,
                shape: GlideShape::Linear,
            }),
            options: BTreeMap::from([
                (
                    BACKEND.to_string(),
                    BTreeMap::from([("colour".to_string(), "soft".to_string())]),
                ),
                (
                    "Another".to_string(),
                    BTreeMap::from([("x".to_string(), "y".to_string())]),
                ),
            ]),
        }));
    }
    let p = &phrases_of(&s.project, s.voice, &s.tempo())[0];
    let (a, b) = (&p.sing.notes[0], &p.sing.notes[1]);
    assert_eq!(a.velocity, 1.0);
    assert!(a.vibrato.is_none() && a.phonemes.is_none() && a.options.is_empty());
    assert!(
        (b.velocity - 2.0).abs() < 1e-3,
        "velocity 127 is twice as quick"
    );
    assert_eq!(b.vibrato, Some(vibrato()));
    assert_eq!(b.portamento.map(|p| p.shape), Some(GlideShape::Linear));
    assert_eq!(b.phonemes.as_ref().unwrap()[0].symbol, "l");
    // Only the singing backend's own options.
    assert_eq!(
        b.options,
        BTreeMap::from([("colour".to_string(), "soft".to_string())])
    );
}

#[test]
fn a_drawn_lane_is_sampled_into_the_phrases_own_time() {
    // Notes on beats 2 and 3 (1.0 s to 2.0 s at 120 bpm).
    let mut s = song(
        "en",
        vec![
            note(BEAT * 2, BEAT, 60, "la"),
            note(BEAT * 3, BEAT, 60, "la"),
        ],
    );
    let mut breath = LaneCurve::default();
    // 0.5 over beat 3 only.
    breath.write(BEAT * 3, BEAT * 4, 20, |_| 0.5, 0.0);
    part_mut(&mut s).lanes.insert(Lane::Breathiness, breath);
    let p = &phrases_of(&s.project, s.voice, &s.tempo())[0];
    assert!((p.start - 1.0).abs() < 1e-9);
    let curve = &p.sing.curves[&Lane::Breathiness];
    // From a little before the first note, so a consonant has a value too.
    assert!((curve.start + CURVE_LEAD).abs() < 1e-9);
    assert_eq!(curve.step, fontelle_sing::CURVE_STEP);
    assert!(
        curve.at(0.25, 9.0).abs() < 1e-6,
        "neutral over the first note"
    );
    assert!(
        (curve.at(0.75, 9.0) - 0.5).abs() < 1e-6,
        "drawn over the second"
    );
    // A lane drawn nowhere near the phrase is not in it.
    assert!(!p.sing.curves.contains_key(&Lane::Tension));
}

#[test]
fn a_lane_drawn_only_away_from_a_phrase_is_left_out_of_it() {
    let mut s = song("en", vec![note(0, BEAT, 60, "la")]);
    let mut far = LaneCurve::default();
    far.write(BEAT * 40, BEAT * 41, 20, |_| 3.0, 0.0);
    part_mut(&mut s).lanes.insert(Lane::Dynamics, far);
    let p = &phrases_of(&s.project, s.voice, &s.tempo())[0];
    assert!(p.sing.curves.is_empty(), "{:?}", p.sing.curves.keys());
}

#[test]
fn only_the_singing_backends_lanes_settings_and_state_reach_it() {
    let mut s = song("en", vec![note(0, BEAT * 2, 60, "la")]);
    let mut growl = LaneCurve::default();
    growl.write(0, BEAT * 2, 20, |_| 0.8, 0.0);
    let part = part_mut(&mut s);
    part.extra.insert(
        BACKEND.into(),
        BTreeMap::from([("growl".to_string(), growl.clone())]),
    );
    part.extra.insert(
        "Another".into(),
        BTreeMap::from([("air".to_string(), growl)]),
    );
    part.settings.insert(
        BACKEND.into(),
        BTreeMap::from([("brightness".to_string(), 0.25)]),
    );
    part.settings.insert(
        "Another".into(),
        BTreeMap::from([("steps".to_string(), 9.0)]),
    );
    part.state.insert(BACKEND.into(), vec![1, 2, 3]);
    part.state.insert("Another".into(), vec![9]);
    let p = &phrases_of(&s.project, s.voice, &s.tempo())[0];
    assert_eq!(
        p.sing.extra_curves.keys().collect::<Vec<_>>(),
        vec!["growl"]
    );
    assert!((p.sing.extra_curves["growl"].at(0.5, 0.0) - 0.8).abs() < 1e-6);
    assert_eq!(
        p.sing.settings,
        BTreeMap::from([("brightness".to_string(), 0.25)])
    );
    assert_eq!(p.sing.state, Some(vec![1, 2, 3]));
    // Another singer on the same part sees its own.
    s.project.channels[s.voice].vocal = Some(VocalVoice::new("Another", "x", "en"));
    let p = &phrases_of(&s.project, s.voice, &s.tempo())[0];
    assert_eq!(p.sing.extra_curves.keys().collect::<Vec<_>>(), vec!["air"]);
    assert_eq!(p.sing.state, Some(vec![9]));
}

#[test]
fn an_extra_lane_left_undrawn_is_its_backends_own_default_and_left_out() {
    let mut s = song("en", vec![note(0, BEAT, 60, "la")]);
    let mut half = LaneCurve::default();
    half.write(BEAT * 10, BEAT * 11, 20, |_| 0.9, 0.5);
    part_mut(&mut s)
        .extra
        .insert(BACKEND.into(), BTreeMap::from([("air".to_string(), half)]));
    let caps = Capabilities {
        extra_lanes: vec![LaneInfo {
            id: "air".into(),
            name: "Air".into(),
            min: 0.0,
            max: 1.0,
            default: 0.5,
            unit: String::new(),
        }],
        ..Capabilities::default()
    };
    let p = &phrases_with(&s.project, s.voice, &s.tempo(), &caps)[0];
    assert!(p.sing.extra_curves.is_empty(), "all at its default here");
}

#[test]
fn a_looped_clips_lanes_repeat_with_its_notes() {
    let mut s = song("en", vec![note(0, BEAT, 60, "la")]);
    s.project.clips[s.clip].loop_length = Some(BEAT * 2);
    s.project.clips[s.clip].length = BEAT * 8;
    let mut dyn_lane = LaneCurve::default();
    dyn_lane.write(0, BEAT, 20, |_| -6.0, 0.0);
    part_mut(&mut s).lanes.insert(Lane::Dynamics, dyn_lane);
    let phrases = phrases_of(&s.project, s.voice, &s.tempo());
    assert_eq!(phrases.len(), 4);
    for p in &phrases {
        let c = &p.sing.curves[&Lane::Dynamics];
        assert!((c.at(0.2, 0.0) + 6.0).abs() < 1e-6, "every pass");
    }
}

#[test]
fn everything_a_phrase_carries_is_in_its_key_and_a_move_keeps_it() {
    let mut s = song("en", vec![note(0, BEAT, 60, "la")]);
    let mut lane = LaneCurve::default();
    lane.write(0, BEAT, 20, |_| 0.3, 0.0);
    part_mut(&mut s).lanes.insert(Lane::Tension, lane);
    let key = |s: &common::Song| {
        render_key(
            BACKEND,
            "1",
            &phrases_of(&s.project, s.voice, &s.tempo())[0].sing,
            Quality::Preview,
        )
    };
    let base = key(&s);
    // Moved later, lanes and all: the same take.
    let mut moved = s.clone_song();
    moved.project.clips[moved.clip].start += BEAT * 7;
    assert_eq!(key(&moved), base);
    // A lane value, a vibrato, a setting, a state, an option: each sings
    // again.
    let mut a = s.clone_song();
    let mut other = LaneCurve::default();
    other.write(0, BEAT, 20, |_| 0.31, 0.0);
    part_mut(&mut a).lanes.insert(Lane::Tension, other);
    assert_ne!(key(&a), base);
    let mut b = s.clone_song();
    let id = b.notes[0];
    b.notes_mut()[id].voice = Some(Box::new(NoteVoice {
        vibrato: Some(vibrato()),
        ..NoteVoice::default()
    }));
    assert_ne!(key(&b), base);
    let mut c = s.clone_song();
    part_mut(&mut c)
        .settings
        .insert(BACKEND.into(), BTreeMap::from([("x".to_string(), 1.0)]));
    assert_ne!(key(&c), base);
    let mut d = s.clone_song();
    part_mut(&mut d).state.insert(BACKEND.into(), vec![1]);
    assert_ne!(key(&d), base);
    let mut e = s.clone_song();
    let id = e.notes[0];
    e.notes_mut()[id].voice = Some(Box::new(NoteVoice {
        options: BTreeMap::from([(
            BACKEND.to_string(),
            BTreeMap::from([("c".to_string(), "soft".to_string())]),
        )]),
        ..NoteVoice::default()
    }));
    assert_ne!(key(&e), base);
    let mut f = s.clone_song();
    let id = f.notes[0];
    f.notes_mut()[id].velocity = 90;
    assert_ne!(key(&f), base);
}

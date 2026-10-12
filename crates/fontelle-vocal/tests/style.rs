//! The instrument's settings reaching what a singer is asked to sing: the
//! Fontelle Singer panel's voice offsets, auto vibrato, default glide,
//! consonant speed, and the backend's own settings, lane bases and
//! note-option defaults — each on the `fontelle_sing::Phrase` the backends
//! already read, under whatever the part or a note sets for itself, and
//! each in the render key so a turned knob sings again.

mod common;

use std::collections::BTreeMap;

use common::{BEAT, note, song};
use fontelle_model::{AutoGlide, AutoVibrato, ClipSource, LaneCurve, NoteVoice, VocalStyle};
use fontelle_sing::{Capabilities, GlideShape, Lane, LaneInfo, Portamento, Quality, Vibrato};
use fontelle_vocal::{phrases_of, phrases_with, render_key};

const BACKEND: &str = "Engine";

fn wobble() -> Vibrato {
    Vibrato {
        depth: 40.0,
        rate: 6.0,
        delay: 0.3,
        fade_in: 0.2,
        fade_out: 0.1,
    }
}

fn glide() -> Portamento {
    Portamento {
        length: 0.08,
        position: 0.5,
        shape: GlideShape::Out,
    }
}

fn style_mut(s: &mut common::Song) -> &mut VocalStyle {
    &mut s.project.channels[s.voice].vocal_style
}

/// A short note (a quarter of a second), a long one (a second and a half)
/// and a long one with its own vibrato.
fn three() -> common::Song {
    song(
        "en",
        vec![
            note(0, BEAT / 2, 60, "la"),
            note(BEAT / 2, BEAT * 3, 62, "lo"),
            note(BEAT * 7 / 2, BEAT * 3, 64, "li"),
        ],
    )
}

#[test]
fn auto_vibrato_goes_on_long_notes_without_their_own() {
    let mut s = three();
    let ids = s.notes.clone();
    let own = Vibrato {
        depth: 90.0,
        ..wobble()
    };
    s.notes_mut()[ids[2]].voice = Some(Box::new(NoteVoice {
        vibrato: Some(own),
        ..NoteVoice::default()
    }));
    // Off: nothing changes.
    style_mut(&mut s).vibrato = Some(AutoVibrato {
        enabled: false,
        vibrato: wobble(),
        min_length: 0.5,
    });
    let p = &phrases_of(&s.project, s.voice, &s.tempo())[0];
    assert_eq!(p.sing.notes[1].vibrato, None, "switched off");
    // On.
    style_mut(&mut s).vibrato.as_mut().unwrap().enabled = true;
    let p = &phrases_of(&s.project, s.voice, &s.tempo())[0];
    assert_eq!(p.sing.notes[0].vibrato, None, "shorter than the minimum");
    assert_eq!(p.sing.notes[1].vibrato, Some(wobble()), "a long plain note");
    assert_eq!(p.sing.notes[2].vibrato, Some(own), "a note's own wins");
}

#[test]
fn the_default_glide_goes_into_notes_after_another_without_their_own() {
    let mut s = three();
    let ids = s.notes.clone();
    let own = Portamento {
        length: 0.2,
        position: 0.0,
        shape: GlideShape::Linear,
    };
    s.notes_mut()[ids[2]].voice = Some(Box::new(NoteVoice {
        portamento: Some(own),
        ..NoteVoice::default()
    }));
    style_mut(&mut s).glide = Some(AutoGlide {
        enabled: true,
        portamento: glide(),
    });
    let p = &phrases_of(&s.project, s.voice, &s.tempo())[0];
    assert_eq!(p.sing.notes[0].portamento, None, "nothing to glide from");
    assert_eq!(p.sing.notes[1].portamento, Some(glide()));
    assert_eq!(p.sing.notes[2].portamento, Some(own), "a note's own wins");
    style_mut(&mut s).glide.as_mut().unwrap().enabled = false;
    let p = &phrases_of(&s.project, s.voice, &s.tempo())[0];
    assert_eq!(p.sing.notes[1].portamento, None, "switched off");
}

#[test]
fn consonant_speed_scales_every_notes_own() {
    let mut s = three();
    let ids = s.notes.clone();
    s.notes_mut()[ids[1]].velocity = 127; // twice as quick
    style_mut(&mut s).consonant_speed = 1.5;
    let p = &phrases_of(&s.project, s.voice, &s.tempo())[0];
    assert!((p.sing.notes[0].velocity - 1.5).abs() < 1e-4);
    assert!((p.sing.notes[1].velocity - 3.0).abs() < 1e-2);
}

/// The Voice section's knobs move the whole track: an offset on the lane
/// drawn, and a lane of its own where none is.
#[test]
fn voice_offsets_ride_on_the_standard_lanes() {
    let mut s = three();
    {
        let ClipSource::Notes(data) = &mut s.project.clips[s.clip].source else {
            panic!()
        };
        let part = data.vocal.get_or_insert_with(Default::default);
        part.lanes.insert(
            Lane::Breathiness,
            LaneCurve::from_points(vec![(0, 0.2), (BEAT * 8, 0.2)]),
        );
    }
    style_mut(&mut s).lanes.insert(Lane::Breathiness, 0.3);
    style_mut(&mut s).lanes.insert(Lane::Gender, 150.0);
    style_mut(&mut s).lanes.insert(Lane::Tension, 0.0);
    let p = &phrases_of(&s.project, s.voice, &s.tempo())[0];
    // Past the lead-in before the clip, where nothing is drawn: the offset
    // alone there.
    let lead = (fontelle_vocal::CURVE_LEAD / fontelle_sing::CURVE_STEP).round() as usize + 1;
    let breath = &p.sing.curves[&Lane::Breathiness];
    assert!(
        breath.values[..lead - 1]
            .iter()
            .all(|v| (v - 0.3).abs() < 1e-4)
    );
    assert!(breath.values[lead..].iter().all(|v| (v - 0.5).abs() < 1e-4));
    let gender = &p.sing.curves[&Lane::Gender];
    assert!(gender.values.iter().all(|v| (v - 150.0).abs() < 1e-3));
    assert_eq!(gender.start, -fontelle_vocal::CURVE_LEAD);
    assert!(
        !p.sing.curves.contains_key(&Lane::Tension),
        "a zero offset is no lane"
    );
}

#[test]
fn the_backends_settings_lane_bases_and_option_defaults_are_the_instruments() {
    let mut s = three();
    let ids = s.notes.clone();
    s.notes_mut()[ids[2]].voice = Some(Box::new(NoteVoice {
        options: BTreeMap::from([(
            BACKEND.to_string(),
            BTreeMap::from([("colour".to_string(), "power".to_string())]),
        )]),
        ..NoteVoice::default()
    }));
    {
        let ClipSource::Notes(data) = &mut s.project.clips[s.clip].source else {
            panic!()
        };
        let part = data.vocal.get_or_insert_with(Default::default);
        part.settings.insert(
            BACKEND.into(),
            BTreeMap::from([("steps".to_string(), 50.0)]),
        );
    }
    {
        let style = style_mut(&mut s);
        style.settings.insert(
            BACKEND.into(),
            BTreeMap::from([("steps".to_string(), 10.0), ("natural".to_string(), 0.0)]),
        );
        style
            .settings
            .insert("Another".into(), BTreeMap::from([("x".to_string(), 1.0)]));
        style.extra.insert(
            BACKEND.into(),
            BTreeMap::from([("expr".to_string(), 60.0), ("growl".to_string(), 0.0)]),
        );
        style.options.insert(
            BACKEND.into(),
            BTreeMap::from([("colour".to_string(), "soft".to_string())]),
        );
    }
    let caps = Capabilities {
        extra_lanes: vec![
            LaneInfo {
                id: "expr".into(),
                name: "Expressiveness".into(),
                min: 0.0,
                max: 100.0,
                default: 100.0,
                unit: "%".into(),
            },
            LaneInfo {
                id: "growl".into(),
                name: "Growl".into(),
                min: 0.0,
                max: 1.0,
                default: 0.0,
                unit: String::new(),
            },
        ],
        ..Capabilities::default()
    };
    let p = &phrases_with(&s.project, s.voice, &s.tempo(), &caps)[0];
    // The part's own setting wins; the instrument's fills the rest.
    assert_eq!(
        p.sing.settings,
        BTreeMap::from([("steps".to_string(), 50.0), ("natural".to_string(), 0.0)])
    );
    // A base other than the backend's default is a lane where none is drawn.
    let expr = &p.sing.extra_curves["expr"];
    assert!(expr.values.iter().all(|v| (v - 60.0).abs() < 1e-3));
    assert!(
        !p.sing.extra_curves.contains_key("growl"),
        "a base at the default is no lane"
    );
    // A note's own option wins; the others get the instrument's.
    assert_eq!(p.sing.notes[0].options["colour"], "soft");
    assert_eq!(p.sing.notes[2].options["colour"], "power");
}

/// A drawn extra lane is the drawing; the base only fills where nothing is.
#[test]
fn a_drawn_extra_lane_keeps_its_drawing_over_the_base() {
    let mut s = three();
    {
        let ClipSource::Notes(data) = &mut s.project.clips[s.clip].source else {
            panic!()
        };
        let part = data.vocal.get_or_insert_with(Default::default);
        part.extra.entry(BACKEND.into()).or_default().insert(
            "expr".into(),
            LaneCurve::from_points(vec![(0, 20.0), (BEAT * 8, 20.0)]),
        );
    }
    style_mut(&mut s)
        .extra
        .insert(BACKEND.into(), BTreeMap::from([("expr".to_string(), 60.0)]));
    let p = &phrases_of(&s.project, s.voice, &s.tempo())[0];
    let lead = (fontelle_vocal::CURVE_LEAD / fontelle_sing::CURVE_STEP).round() as usize + 1;
    let expr = &p.sing.extra_curves["expr"].values;
    assert!(expr[lead..].iter().all(|v| (v - 20.0).abs() < 1e-3));
    // Before the clip, where nothing is drawn, the base.
    assert!((expr[0] - 60.0).abs() < 1e-3);
}

/// A knob turned is a different take; turned back, the same one.
#[test]
fn the_style_is_in_the_render_key() {
    let mut s = three();
    let key = |s: &common::Song| {
        let p = &phrases_of(&s.project, s.voice, &s.tempo())[0];
        render_key(BACKEND, "1", &p.sing, Quality::Final)
    };
    let plain = key(&s);
    style_mut(&mut s).lanes.insert(Lane::Gender, 100.0);
    let gender = key(&s);
    assert_ne!(plain, gender);
    style_mut(&mut s).lanes.clear();
    style_mut(&mut s).consonant_speed = 1.25;
    assert_ne!(plain, key(&s));
    style_mut(&mut s).consonant_speed = 1.0;
    assert_eq!(plain, key(&s));
}

//! The render's decisions that need no model: reading the phrase, pitch
//! source, steps, sampling inputs, speakers, what the roll is told.

mod common;

use std::collections::BTreeMap;

use common::{scratch, write_bank};
use fontelle_diffsinger::bank::{
    Bank, LANE_SPEAKER_PREFIX, OPTION_SPEAKER, SETTING_NATURAL_PITCH, SETTING_STEPS,
};
use fontelle_diffsinger::config::DsConfig;
use fontelle_diffsinger::models::Feed;
use fontelle_diffsinger::render::{
    PREVIEW_STEPS, plan, sampling_feeds, speaker_weights, steps, sung_phonemes, uses_note_pitch,
};
use fontelle_diffsinger::timing::place;
use fontelle_sing::{Curve, Lane, Phrase, PhraseNote, Quality, Vibrato};

fn bank(name: &str) -> Bank {
    let root = scratch(name);
    let (dir, deps) = write_bank(&root);
    Bank::load(&dir, &[deps]).unwrap()
}

fn phrase(lyrics: &[&str], language: &str) -> Phrase {
    Phrase {
        voice: "Test Bank".into(),
        language: language.into(),
        notes: lyrics
            .iter()
            .enumerate()
            .map(|(i, l)| PhraseNote::new(i as f64 * 0.5, 0.5, 60.0 + i as f32, *l))
            .collect(),
        curves: BTreeMap::new(),
        extra_curves: BTreeMap::new(),
        settings: BTreeMap::new(),
        state: None,
    }
}

#[test]
fn a_phrase_is_read_in_its_language() {
    let b = bank("plan-read");
    let p = plan(&b, &phrase(&["hello", "la"], "en-GB")).unwrap();
    assert_eq!(p.language, "en");
    assert_eq!(p.per_note[0].lead[0].symbol, "en/hh");
    assert_eq!(p.per_note[1].lead[0].symbol, "en/l");
    // Japanese, from the general dictionary.
    let p = plan(&b, &phrase(&["ka"], "ja")).unwrap();
    assert_eq!(p.per_note[0].lead[0].symbol, "ja/k");
}

#[test]
fn the_notes_line_is_used_when_the_writer_shaped_the_pitch() {
    let b = bank("plan-pitch");
    let mut p = phrase(&["la", "la"], "en");
    assert!(
        !uses_note_pitch(&b, &p),
        "the model sings what is not drawn"
    );
    p.notes[1].vibrato = Some(Vibrato::default());
    assert!(uses_note_pitch(&b, &p));
    let mut p = phrase(&["la"], "en");
    p.curves.insert(
        Lane::PitchBend,
        Curve {
            start: 0.0,
            step: 0.005,
            values: vec![0.0, 30.0],
        },
    );
    assert!(uses_note_pitch(&b, &p));
    // A bend lane drawn flat is no bend.
    p.curves.get_mut(&Lane::PitchBend).unwrap().values = vec![0.0, 0.0];
    assert!(!uses_note_pitch(&b, &p));
    let mut p = phrase(&["la"], "en");
    p.settings.insert(SETTING_NATURAL_PITCH.into(), 0.0);
    assert!(uses_note_pitch(&b, &p));
}

#[test]
fn preview_samples_less_than_final() {
    let mut p = phrase(&["la"], "en");
    assert_eq!(steps(&p, Quality::Final), (20, 20, 10));
    assert_eq!(
        steps(&p, Quality::Preview),
        (PREVIEW_STEPS, PREVIEW_STEPS, PREVIEW_STEPS)
    );
    p.settings.insert(SETTING_STEPS.into(), 50.0);
    assert_eq!(steps(&p, Quality::Final).0, 50);
    p.settings.insert(SETTING_STEPS.into(), 2.0);
    assert_eq!(steps(&p, Quality::Preview).0, 2, "never more than final");
}

fn feeds_map(v: Vec<(&'static str, Feed)>) -> BTreeMap<&'static str, Feed> {
    v.into_iter().collect()
}

#[test]
fn sampling_inputs_follow_the_acceleration_kind() {
    let all = |_: &str| true;
    let mut c = DsConfig {
        use_continuous_acceleration: true,
        use_variable_depth: Some(true),
        max_depth: Some(0.6),
        ..DsConfig::default()
    };
    let f = feeds_map(sampling_feeds(&c, &all, 20, Some(0.6)));
    assert_eq!(f["steps"], Feed::I64(vec![], vec![20]));
    assert_eq!(f["depth"], Feed::F32(vec![], vec![0.6]));
    assert!(!f.contains_key("speedup"));
    // Discrete: a speedup that divides the 1000 steps, the depth out of
    // 1000 a multiple of it.
    c.use_continuous_acceleration = false;
    let f = feeds_map(sampling_feeds(&c, &all, 20, None));
    assert_eq!(f["speedup"], Feed::I64(vec![], vec![50]));
    let f = feeds_map(sampling_feeds(&c, &all, 30, Some(0.4)));
    assert_eq!(f["speedup"], Feed::I64(vec![], vec![13]));
    assert_eq!(f["depth"], Feed::I64(vec![], vec![390]));
    // A model without a depth input gets none.
    let only_steps = |n: &str| n == "steps";
    c.use_continuous_acceleration = true;
    let f = feeds_map(sampling_feeds(&c, &only_steps, 10, Some(0.6)));
    assert!(!f.contains_key("depth"));
}

#[test]
fn speakers_follow_the_note_and_the_mix_lanes() {
    let b = bank("plan-speakers");
    let mut p = phrase(&["la", "la"], "en");
    p.notes[1]
        .options
        .insert(OPTION_SPEAKER.into(), "soprano".into());
    let root = &b.acoustic; // alto, soprano
    let dur = b.dur.as_ref().unwrap(); // soprano, alto
    assert_eq!(speaker_weights(&b, root, &p, 0, 0.1), vec![1.0, 0.0]);
    assert_eq!(speaker_weights(&b, dur, &p, 0, 0.1), vec![0.0, 1.0]);
    assert_eq!(speaker_weights(&b, root, &p, 1, 0.6), vec![0.0, 1.0]);
    p.extra_curves.insert(
        format!("{LANE_SPEAKER_PREFIX}soprano"),
        Curve {
            start: 0.0,
            step: 1.0,
            values: vec![25.0],
        },
    );
    assert_eq!(speaker_weights(&b, root, &p, 0, 0.1), vec![0.75, 0.25]);
    // Mixes over 100% are scaled to it.
    p.extra_curves.insert(
        format!("{LANE_SPEAKER_PREFIX}alto"),
        Curve {
            start: 0.0,
            step: 1.0,
            values: vec![75.0],
        },
    );
    p.extra_curves
        .get_mut(&format!("{LANE_SPEAKER_PREFIX}soprano"))
        .unwrap()
        .values = vec![75.0];
    assert_eq!(speaker_weights(&b, root, &p, 0, 0.1), vec![0.5, 0.5]);
}

#[test]
fn the_roll_is_told_what_was_sung_in_its_own_symbols() {
    let b = bank("plan-sung");
    let p = phrase(&["hello", "zzzz"], "en");
    let pl = plan(&b, &p).unwrap();
    let placed = place(&p.notes, &pl.per_note, None, &|s| pl.dictionary.is_vowel(s));
    let sung = sung_phonemes(&p, &pl, &placed);
    assert_eq!(sung.len(), 2);
    assert_eq!(sung[0].note, 0);
    let symbols: Vec<&str> = sung[0].phonemes.iter().map(|x| x.symbol.as_str()).collect();
    assert_eq!(symbols, ["hh", "aa", "l", "ow"]);
    assert!(
        sung[0].phonemes[0].offset.unwrap() < 0.0,
        "the onset is before the note"
    );
    assert_eq!(sung[0].phonemes[1].offset, Some(0.0));
    assert!(!sung[0].guessed);
    assert!(sung[1].guessed);
}

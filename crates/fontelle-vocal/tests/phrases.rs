//! A vocal part cut into phrases, and the keys their renders are cached by.
//!
//! A phrase is a run of notes with no long rest inside, sung as one: the
//! singer needs the notes around a syllable to sing it, so a phrase is the
//! unit of rendering and of caching. Its key is what is sung, in
//! phrase-relative time — so moving a phrase, or an undo that puts one back,
//! finds the take already sung, and an edit sings only its own phrase again.

mod common;

use common::{BEAT, note, song};
use fontelle_model::{ClipSource, Note};
use fontelle_types::PPQN;
use fontelle_vocal::{MAX_PHRASE_SECONDS, phrases_of, render_key};

fn line(starts_in_beats: &[f64], lyric: &str) -> Vec<Note> {
    starts_in_beats
        .iter()
        .enumerate()
        .map(|(i, b)| note((*b * BEAT as f64) as i64, BEAT, 60 + i as u8, lyric))
        .collect()
}

#[test]
fn a_rest_of_a_fifth_of_a_second_or_more_ends_a_phrase() {
    // Beats 0, 1, 2 joined; a beat's rest (0.5 s); beats 4, 5.
    let s = song("en", line(&[0.0, 1.0, 2.0, 4.0, 5.0], "la"));
    let phrases = phrases_of(&s.project, s.voice, &s.tempo());
    assert_eq!(phrases.len(), 2);
    assert_eq!(phrases[0].notes.len(), 3);
    assert!((phrases[0].start - 0.0).abs() < 1e-9);
    assert!((phrases[0].end - 1.5).abs() < 1e-9);
    assert!((phrases[1].start - 2.0).abs() < 1e-9);
    // In phrase-relative seconds, as the ABI wants them.
    let starts: Vec<f64> = phrases[1].spec.notes.iter().map(|n| n.start).collect();
    assert_eq!(starts, vec![0.0, 0.5]);
    assert!((phrases[1].spec.notes[0].duration - 0.5).abs() < 1e-9);
    assert_eq!(phrases[1].spec.notes[0].pitch, 63.0);
    assert_eq!(phrases[0].spec.singer, "alto");
    assert_eq!(phrases[0].spec.language, "en");
}

#[test]
fn a_short_breath_does_not() {
    // A tenth of a second between notes (PPQN/5 at 120 bpm).
    let gap = PPQN / 5;
    let notes = vec![note(0, BEAT, 60, "a"), note(BEAT + gap, BEAT, 62, "b")];
    let s = song("en", notes);
    assert_eq!(phrases_of(&s.project, s.voice, &s.tempo()).len(), 1);
}

#[test]
fn a_long_legato_line_is_capped() {
    // 40 seconds without a rest.
    let starts: Vec<f64> = (0..80).map(|i| i as f64).collect();
    let s = song("en", line(&starts, "la"));
    let phrases = phrases_of(&s.project, s.voice, &s.tempo());
    assert!(phrases.len() >= 4, "{} phrases", phrases.len());
    for p in &phrases {
        assert!(
            p.end - p.start <= MAX_PHRASE_SECONDS + 1e-9,
            "{}..{}",
            p.start,
            p.end
        );
    }
    let total: usize = phrases.iter().map(|p| p.notes.len()).sum();
    assert_eq!(total, 80, "every note is in exactly one phrase");
}

#[test]
fn an_empty_lyric_sings_the_languages_own_syllable() {
    let s = song("ja", line(&[0.0, 1.0], ""));
    let phrases = phrases_of(&s.project, s.voice, &s.tempo());
    assert_eq!(phrases[0].spec.notes[0].lyric, "ら");
    let s = song("en", line(&[0.0], ""));
    assert_eq!(
        phrases_of(&s.project, s.voice, &s.tempo())[0].spec.notes[0].lyric,
        "la"
    );
}

#[test]
fn only_what_the_song_would_play_on_this_channel_is_sung() {
    let mut notes = line(&[0.0, 1.0], "la");
    // One note of the clip played by another channel, one by this one from
    // another channel's clip below.
    notes[1].channel = None;
    let mut s = song("en", notes);
    let other = s.other;
    let id = s.notes[1];
    s.notes_mut()[id].channel = Some(other);
    let phrases = phrases_of(&s.project, s.voice, &s.tempo());
    assert_eq!(phrases.len(), 1);
    assert_eq!(phrases[0].notes.len(), 1);

    // A muted clip, a muted channel: nothing.
    let mut s = song("en", line(&[0.0, 1.0], "la"));
    s.project.clips[s.clip].muted = true;
    assert!(phrases_of(&s.project, s.voice, &s.tempo()).is_empty());
    let mut s = song("en", line(&[0.0, 1.0], "la"));
    s.project.channels[s.voice].muted = true;
    assert!(phrases_of(&s.project, s.voice, &s.tempo()).is_empty());
}

#[test]
fn a_clip_cut_short_or_looped_sings_what_it_plays() {
    let mut s = song("en", line(&[0.0, 1.0, 4.0], "la"));
    s.project.clips[s.clip].length = BEAT * 2;
    let phrases = phrases_of(&s.project, s.voice, &s.tempo());
    assert_eq!(phrases.iter().map(|p| p.notes.len()).sum::<usize>(), 2);

    let mut s = song("en", line(&[0.0], "la"));
    s.project.clips[s.clip].length = BEAT * 8;
    s.project.clips[s.clip].loop_length = Some(BEAT * 2);
    let phrases = phrases_of(&s.project, s.voice, &s.tempo());
    assert_eq!(phrases.len(), 4, "a loop of four passes sings four times");
    // Each pass is the same phrase: one render, four places.
    let keys: Vec<u64> = phrases
        .iter()
        .map(|p| render_key("Engine", "1", &p.spec, 0).hash)
        .collect();
    assert!(keys.windows(2).all(|w| w[0] == w[1]));
}

#[test]
fn moving_a_phrase_keeps_its_key_and_an_edit_changes_only_its_own() {
    let s = song("en", line(&[0.0, 1.0, 4.0, 5.0], "la"));
    let before = phrases_of(&s.project, s.voice, &s.tempo());
    let key = |p: &fontelle_vocal::Phrase| render_key("Engine", "1", &p.spec, 0);

    // The whole clip two bars later.
    let mut moved = song("en", line(&[0.0, 1.0, 4.0, 5.0], "la"));
    moved.project.clips[moved.clip].start = BEAT * 8;
    let after = phrases_of(&moved.project, moved.voice, &moved.tempo());
    assert_eq!(key(&before[0]), key(&after[0]));
    assert_eq!(key(&before[1]), key(&after[1]));
    assert!((after[0].start - 4.0).abs() < 1e-9);

    // One word of the second phrase retyped.
    let mut edited = song("en", line(&[0.0, 1.0, 4.0, 5.0], "la"));
    let id = edited.notes[3];
    edited.notes_mut()[id].lyric = "lo".into();
    let after = phrases_of(&edited.project, edited.voice, &edited.tempo());
    assert_eq!(key(&before[0]), key(&after[0]));
    assert_ne!(key(&before[1]), key(&after[1]));
    // And the notes are said back, so the roll can find them.
    let ClipSource::Notes(_) = &edited.project.clips[edited.clip].source else {
        panic!()
    };
    assert_eq!(after[1].notes[1].note, id);
}

#[test]
fn the_key_names_engine_version_singer_language_and_quality() {
    let s = song("en", line(&[0.0, 1.0], "la"));
    let p = &phrases_of(&s.project, s.voice, &s.tempo())[0];
    let base = render_key("Engine", "1", &p.spec, 0);
    assert_ne!(base, render_key("Other", "1", &p.spec, 0));
    assert_ne!(base, render_key("Engine", "2", &p.spec, 0));
    assert_ne!(base, render_key("Engine", "1", &p.spec, 1));
    let mut spec = p.spec.clone();
    spec.singer = "soprano".into();
    assert_ne!(base, render_key("Engine", "1", &spec, 0));
    let mut spec = p.spec.clone();
    spec.language = "ja".into();
    assert_ne!(base, render_key("Engine", "1", &spec, 0));
    // The seed comes from what is sung, not the quality: preview and final
    // are the same take, the final one better.
    assert_eq!(base.seed, render_key("Engine", "1", &p.spec, 1).seed);
    assert_eq!(base, render_key("Engine", "1", &p.spec.clone(), 0));
}

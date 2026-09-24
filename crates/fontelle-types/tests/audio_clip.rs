//! What an audio clip *is* (TDD §15.1), and the arithmetic every part of the
//! program that touches one has to agree about.
//!
//! Reported from using the window:
//!
//! > *"double clicking on an audio clip should open a menu that lets me make
//! > changes to that audio like basic changes that would be widely general
//! > useful across all audio editing so for example changing the boost or the
//! > cutoff or resonance of the sound or even changing a fade in or fade out."*
//!
//! §15.1 is emphatic about the shape this takes and it is the right shape:
//! **every property is stored on the clip and applied at playback; the source
//! file is never modified.** So a clip is a reference plus a list of numbers,
//! nothing is destructive, undo is free, and two clips can point at one file
//! and sound completely different.
//!
//! This file tests the numbers rather than the sound — where in the file frame
//! *n* of a clip comes from, and how loud it is when it gets there. The sound
//! itself is `fontelle-engine`'s, and it reads these same functions, which is
//! the point of them living here.

use fontelle_types::{
    AssetKind, AssetRef, AudioClipData, ClipLoopMode, ClipStretch, Fade, FadeCurve,
};

fn an_asset() -> AssetRef {
    AssetRef {
        id: fontelle_types::AssetId::default(),
        path: "take.wav".into(),
        content_hash: 0,
        size: 0,
        kind: AssetKind::Sample,
    }
}

/// A clip over a file of `frames` frames, everything at its default.
fn clip(frames: i64) -> AudioClipData {
    AudioClipData::whole(an_asset(), frames, 48_000)
}

// ---------------------------------------------------------- the defaults ---

#[test]
fn a_fresh_clip_is_the_whole_file_and_changes_nothing_about_it() {
    // The identity, and it matters: dragging a file onto the arrangement must
    // sound exactly like the file. Every knob in the editor starts where it
    // does nothing, so opening the editor and closing it again is not an edit.
    let c = clip(48_000);
    assert_eq!(c.source_start, 0);
    assert_eq!(c.source_end, 48_000);
    assert_eq!(c.gain_db, 0.0);
    assert_eq!(c.pan, 0.0);
    assert_eq!(c.pitch_semitones, 0.0);
    assert_eq!(c.speed, 1.0);
    assert!(!c.reverse);
    assert!(!c.normalize);
    assert_eq!(c.fade_in.frames, 0);
    assert_eq!(c.fade_out.frames, 0);
    assert_eq!(c.loop_mode, ClipLoopMode::Once);
    assert_eq!(c.source_frames(), 48_000);
}

#[test]
fn a_clip_carries_the_mixer_track_it_arrives_on() {
    // The same rule a note clip follows (TDD §10.3): **the clip carries its
    // destination, the lane has none.** A lane is a visual strip and nothing
    // else, so an audio clip dragged from one row to another must not change
    // what it is routed through — which is exactly what a lane-owned route
    // would do.
    //
    // `None` is the master, matching `Channel::mixer_track`: a mixer track is
    // a destination somebody builds deliberately, not something that appears
    // when a file is dropped.
    let c = clip(100);
    assert_eq!(c.mixer_track, None);
}

#[test]
fn a_clip_remembers_the_rate_its_file_was_recorded_at() {
    // Not derivable from anything else on the clip, and needed by everything
    // that has to relate the clip's **time** to the file's **frames**: the
    // editor showing a fade in milliseconds, and — the one that made this a
    // field rather than a lookup — the cut tool deciding where in the file a
    // seam falls. A split that guessed proportionally is right exactly when
    // the clip is the same length as its audio.
    let c = AudioClipData::whole(an_asset(), 48_000, 44_100);
    assert_eq!(c.sample_rate, 44_100);
    assert!((c.seconds() - 48_000.0 / 44_100.0).abs() < 1e-9);
}

#[test]
fn a_clip_trimmed_to_a_middle_section_is_that_many_frames_long() {
    let mut c = clip(48_000);
    c.source_start = 12_000;
    c.source_end = 36_000;
    assert_eq!(c.source_frames(), 24_000);
}

#[test]
fn an_upside_down_trim_is_no_frames_rather_than_a_negative_number() {
    // A negative length reaches the RT thread as a loop bound and a capacity,
    // and both of those are a crash rather than a wrong sound.
    let mut c = clip(1000);
    c.source_start = 800;
    c.source_end = 200;
    assert_eq!(c.source_frames(), 0);
}

// ------------------------------------------------------- reading the file ---

#[test]
fn frame_n_of_a_plain_clip_is_frame_n_of_the_file() {
    let c = clip(1000);
    assert_eq!(c.source_position(0.0), 0.0);
    assert_eq!(c.source_position(250.0), 250.0);
}

#[test]
fn a_trimmed_clip_starts_where_the_trim_says() {
    let mut c = clip(1000);
    c.source_start = 400;
    c.source_end = 900;
    assert_eq!(c.source_position(0.0), 400.0);
    assert_eq!(c.source_position(100.0), 500.0);
}

#[test]
fn speed_moves_through_the_file_faster_or_slower() {
    let mut c = clip(1000);
    c.speed = 2.0;
    assert_eq!(
        c.source_position(100.0),
        200.0,
        "double speed is twice as far in"
    );
    c.speed = 0.5;
    assert_eq!(c.source_position(100.0), 50.0);
}

#[test]
fn a_reversed_clip_reads_from_the_far_end_back() {
    // And it must land exactly on the last frame at position zero, not one
    // past it: one past is silence, and a reversed clip that starts with a
    // click is the classic version of this bug.
    let mut c = clip(1000);
    c.source_end = 600;
    c.reverse = true;
    assert_eq!(c.source_position(0.0), 599.0);
    assert_eq!(c.source_position(100.0), 499.0);
    assert_eq!(c.source_position(599.0), 0.0);
}

#[test]
fn a_looping_clip_wraps_back_to_its_own_start() {
    let mut c = clip(1000);
    c.source_start = 100;
    c.source_end = 300;
    c.loop_mode = ClipLoopMode::Loop;
    assert_eq!(c.source_position(0.0), 100.0);
    assert_eq!(c.source_position(199.0), 299.0);
    assert_eq!(c.source_position(200.0), 100.0, "the loop came round");
    assert_eq!(c.source_position(250.0), 150.0);
}

#[test]
fn a_clip_that_does_not_loop_runs_off_the_end_rather_than_wrapping() {
    // Past the end is silence; wrapping a non-looping clip would repeat its
    // front, which is the wrong sound and a very confusing one.
    //
    // And past the end of the **trim**, not of the file: this used to keep
    // counting into the rest of the file, so a clip trimmed at a cut played
    // on past its edge while its picture showed nothing there — *"it removes
    // the content of the audio for the section after the cutoff if i try to
    // expand it again"*. A position below every file is the player's silence.
    let mut c = clip(1000);
    c.source_end = 200;
    assert_eq!(c.source_position(199.0), 199.0);
    assert!(
        c.source_position(200.0) < 0.0,
        "the trim's end is not silence"
    );
    assert!(c.source_position(500.0) < 0.0, "it read on past the trim");
}

// ------------------------------------------------- pitch, speed, and time ---
//
// Reported from using the window: *"changing pitch is stretching the audio
// even when stretch is off and not set to resample."* With the stretch
// switch off, pitch and speed are two different knobs: pitch moves what is
// heard and not how long it takes, and speed moves how long it takes and not
// what is heard. Only `Resample` — varispeed by definition — couples them.

#[test]
fn in_resample_mode_pitch_and_speed_are_the_same_movement_through_the_file() {
    // Varispeed: an octave up is double speed, and saying it twice — once in
    // semitones and once in speed — has to give one answer.
    let mut a = clip(10_000);
    a.stretch = ClipStretch::Resample;
    a.pitch_semitones = 12.0;
    let mut b = clip(10_000);
    b.stretch = ClipStretch::Resample;
    b.speed = 2.0;
    assert!((a.source_position(100.0) - b.source_position(100.0)).abs() < 1e-6);
    assert!((a.time_rate() - 2.0).abs() < 1e-9);
    assert!((a.read_rate() - 2.0).abs() < 1e-9, "the pitch is the speed");
    assert!(!a.shifts_pitch(), "varispeed is a plain read");
}

#[test]
fn with_stretch_off_pitch_does_not_move_through_the_file() {
    let mut c = clip(10_000);
    c.pitch_semitones = 12.0;
    assert_eq!(c.stretch, ClipStretch::Off);
    assert_eq!(
        c.source_position(100.0),
        100.0,
        "an octave up is still frame 100 at frame 100"
    );
    assert!((c.time_rate() - 1.0).abs() < 1e-9);
    assert!(
        (c.read_rate() - 2.0).abs() < 1e-9,
        "but it is heard an octave up"
    );
    assert!(c.shifts_pitch());
}

#[test]
fn with_stretch_off_speed_moves_through_the_file_and_keeps_the_pitch() {
    let mut c = clip(10_000);
    c.speed = 2.0;
    assert_eq!(c.source_position(100.0), 200.0);
    assert!((c.time_rate() - 2.0).abs() < 1e-9);
    assert!(
        (c.read_rate() - 1.0).abs() < 1e-9,
        "twice as fast, not an octave up"
    );
    assert!(c.shifts_pitch());
}

#[test]
fn a_clip_whose_pitch_and_speed_agree_is_a_plain_read_whatever_the_mode() {
    // The identity the stretch switch relies on: a clip in `Off` with its
    // speed doubled *and* its pitch an octave up is exactly the varispeed
    // read a stretched clip makes, so turning stretch off can keep the sound.
    let mut c = clip(10_000);
    c.speed = 2.0;
    c.pitch_semitones = 12.0;
    assert!((c.time_rate() - 2.0).abs() < 1e-9);
    assert!((c.read_rate() - 2.0).abs() < 1e-9);
    assert!(!c.shifts_pitch());
}

#[test]
fn a_grain_reads_at_the_heard_rate_from_where_its_anchor_falls_in_time() {
    // The shifter's whole arithmetic. A grain anchored at clip frame `a`
    // starts where the time map puts `a` and then reads at the heard rate,
    // so with pitch and speed agreeing every grain reads the same frame —
    // which is why that case needs no grains at all.
    let mut c = clip(10_000);
    c.pitch_semitones = 12.0;
    // Anchor 0: frame 100 of the clip is read from frame 200 of the file.
    assert!((c.grain_offset(100.0, 0.0) - 200.0).abs() < 1e-9);
    // Anchor 1000: the grain starts at file frame 1000 (time rate 1) and has
    // read 200 frames by clip frame 1100.
    assert!((c.grain_offset(1100.0, 1000.0) - 1200.0).abs() < 1e-9);

    let mut plain = clip(10_000);
    plain.speed = 2.0;
    plain.pitch_semitones = 12.0;
    for anchor in [0.0, 500.0, 1000.0] {
        assert!(
            (plain.grain_offset(1100.0, anchor) - 2200.0).abs() < 1e-9,
            "anchor {anchor} disagrees with the plain read"
        );
    }
}

#[test]
fn a_grain_offset_is_placed_in_the_file_the_way_a_position_is() {
    // Trim, reverse and the loop are one function's business, whichever way
    // the offset was arrived at.
    let mut c = clip(1000);
    c.source_start = 100;
    c.source_end = 300;
    c.loop_mode = ClipLoopMode::Loop;
    assert_eq!(c.source_at_offset(250.0), 150.0, "the loop came round");
    c.loop_mode = ClipLoopMode::Once;
    c.reverse = true;
    assert_eq!(c.source_at_offset(0.0), 299.0);
    // And `source_position` is the same placement of the time map's offset.
    c.speed = 2.0;
    assert_eq!(c.source_position(50.0), c.source_at_offset(100.0));
}

#[test]
fn the_grain_hop_is_a_fixed_stretch_of_the_files_own_time() {
    // About twenty milliseconds at the file's rate: long enough to hold a
    // cycle of anything a bass plays, short enough not to smear a hit. A
    // clip whose rate is unknown gets a hop anyway rather than a zero that
    // would divide.
    let c = clip(10_000);
    let hop = c.grain_hop();
    assert!((900.0..=1_300.0).contains(&hop), "{hop} frames at 48 kHz");
    let mut unknown = clip(10_000);
    unknown.sample_rate = 0;
    assert!(unknown.grain_hop() >= 64.0);
}

// ---------------------------------------------------------------- fades ---

#[test]
fn a_clip_with_no_fades_is_at_full_level_from_its_first_frame() {
    let c = clip(1000);
    assert_eq!(c.fade_gain(0.0), 1.0);
    assert_eq!(c.fade_gain(999.0), 1.0);
}

#[test]
fn a_fade_in_rises_from_nothing_to_everything_across_its_length() {
    let mut c = clip(1000);
    c.fade_in = Fade {
        frames: 100,
        curve: FadeCurve::Linear,
        tension: 0.0,
    };
    assert_eq!(c.fade_gain(0.0), 0.0);
    assert!((c.fade_gain(50.0) - 0.5).abs() < 1e-6);
    assert_eq!(c.fade_gain(100.0), 1.0);
    assert_eq!(c.fade_gain(500.0), 1.0);
}

#[test]
fn a_fade_out_falls_to_nothing_at_the_clips_last_frame() {
    let mut c = clip(1000);
    c.fade_out = Fade {
        frames: 200,
        curve: FadeCurve::Linear,
        tension: 0.0,
    };
    assert_eq!(c.fade_gain(800.0), 1.0);
    assert!((c.fade_gain(900.0) - 0.5).abs() < 1e-6);
    assert!(c.fade_gain(1000.0).abs() < 1e-6);
}

#[test]
fn two_fades_that_overlap_multiply_rather_than_fighting() {
    // A one-second clip with a two-second fade in and a two-second fade out is
    // a real thing to ask for by dragging, and it has to produce a shape rather
    // than a discontinuity where one wins.
    let mut c = clip(100);
    c.fade_in = Fade {
        frames: 100,
        curve: FadeCurve::Linear,
        tension: 0.0,
    };
    c.fade_out = Fade {
        frames: 100,
        curve: FadeCurve::Linear,
        tension: 0.0,
    };
    assert_eq!(c.fade_gain(0.0), 0.0);
    assert_eq!(c.fade_gain(100.0), 0.0);
    let middle = c.fade_gain(50.0);
    assert!(middle > 0.0 && middle < 1.0, "the middle is {middle}");
}

#[test]
fn every_fade_curve_runs_from_nothing_to_everything_and_never_leaves_the_range() {
    // A curve that overshoots is a fade that gets louder than the clip, which
    // on a full mix is a clip.
    for curve in FadeCurve::ALL {
        let mut c = clip(1000);
        c.fade_in = Fade {
            frames: 100,
            curve,
            tension: 0.0,
        };
        assert_eq!(c.fade_gain(0.0), 0.0, "{curve:?} does not start at nothing");
        assert!(
            (c.fade_gain(100.0) - 1.0).abs() < 1e-6,
            "{curve:?} does not reach one"
        );
        for step in 0..=100 {
            let g = c.fade_gain(step as f64);
            assert!(
                (0.0..=1.0).contains(&g),
                "{curve:?} leaves the range at {step}: {g}"
            );
        }
        // And it never goes backwards.
        let mut previous = -1.0;
        for step in 0..=100 {
            let g = c.fade_gain(step as f64);
            assert!(g >= previous - 1e-6, "{curve:?} dips at {step}");
            previous = g;
        }
    }
}

// ----------------------------------------------------------------- gain ---

#[test]
fn the_boost_is_decibels_and_zero_is_untouched() {
    let mut c = clip(10);
    assert!((c.gain() - 1.0).abs() < 1e-6);
    c.gain_db = 6.0206;
    assert!(
        (c.gain() - 2.0).abs() < 1e-3,
        "+6 dB is twice, got {}",
        c.gain()
    );
    c.gain_db = -6.0206;
    assert!((c.gain() - 0.5).abs() < 1e-3);
}

#[test]
fn the_boost_stops_somewhere_rather_than_going_to_infinity() {
    let mut c = clip(10);
    c.gain_db = 1e9;
    assert!(
        c.gain().is_finite(),
        "a typo in a text field must not be +inf"
    );
}

// ------------------------------------------------------------ the file ---

#[test]
fn a_clip_written_to_json_and_read_back_is_the_same_clip() {
    // The document is JSON on disk (§17.2) and a project must open as what was
    // saved. Round-tripped rather than compared field by field, because a field
    // added later and left out of a hand-written check is exactly the bug this
    // catches.
    let mut c = clip(48_000);
    c.gain_db = -3.0;
    c.fade_in = Fade {
        frames: 512,
        curve: FadeCurve::SCurve,
        tension: 0.0,
    };
    c.filter.cutoff_hz = 800.0;
    c.filter.resonance = 0.4;
    c.reverse = true;
    let text = serde_json::to_string(&c).expect("a clip serialises");
    let back: AudioClipData = serde_json::from_str(&text).expect("and reads back");
    assert_eq!(back, c);
}

#[test]
fn a_clip_saved_before_a_property_existed_opens_with_that_property_at_its_default() {
    // Every field is `serde(default)`, so a project written by an older build
    // opens rather than refusing. The oldest possible clip is its asset and its
    // bounds.
    let asset = serde_json::to_string(&an_asset()).expect("an asset serialises");
    let text = format!(r#"{{"asset":{asset},"source_start":0,"source_end":100}}"#);
    let back: AudioClipData = serde_json::from_str(&text).expect("an old clip still opens");
    assert_eq!(back.gain_db, 0.0);
    assert_eq!(back.speed, 1.0);
    assert_eq!(back.loop_mode, ClipLoopMode::Once);
}

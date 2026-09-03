//! `.fsc` import — FL Studio's piano-roll score files (TDD §14.6's neighbour).
//!
//! # Where the format in these tests comes from
//!
//! Not from a specification: Image-Line publishes none. It was read off **FL
//! Studio's own factory score library** — 609 `.fsc` files shipped with FL
//! Studio 2025, holding 5523 notes, written by every version from 3.0.0 to
//! 20.9.0 — and every rule the importer follows is one that holds across all
//! of them with no exceptions. The two that matter:
//!
//! - A note record is **20 bytes** in a file written before version 8 and
//!   **24 bytes** from version 8 on. The version string is the only thing in
//!   the file that says which, and the block length cannot settle it: 120
//!   bytes is six narrow notes *or* five wide ones, and 240 of the 609 files
//!   are ambiguous that way.
//! - Every field is centred where FL's own knob is, not where Fontelle's is,
//!   so every one of them is converted rather than copied.
//!
//! The corpus itself is not checked in — it is somebody else's copyrighted
//! content — so the fixtures here are byte streams this crate builds, and
//! `a_whole_library_of_real_scores_reads_cleanly` runs against a real copy
//! when one is pointed at.

use fontelle_assets::fixtures::{FscNoteSpec, build_fsc, build_fsc_raw, write_fsc_to_temp_file};
use fontelle_assets::{import_fsc, read_fsc};
use fontelle_types::PPQN;

/// The version FL wrote for most of its factory library, and the one whose
/// record is narrow.
const OLD: &str = "3.5.0";
/// A version whose record is wide.
const NEW: &str = "20.9.0.2696";

/// FL's own resolution, and the one every file in its factory library but a
/// handful uses.
const FL_PPQ: u16 = 96;

/// How many project ticks one FL tick is worth at [`FL_PPQ`].
const PER_FL_TICK: i64 = PPQN / FL_PPQ as i64;

fn note(position: u32, key: u8) -> FscNoteSpec {
    FscNoteSpec {
        position,
        key,
        ..Default::default()
    }
}

#[test]
fn a_score_becomes_notes_on_the_projects_own_grid() {
    let bytes = build_fsc(NEW, FL_PPQ, &[note(96, 60), note(192, 64)]);
    let score = read_fsc(&bytes, "Chords").expect("a well-formed score reads");

    assert_eq!(score.name, "Chords");
    assert_eq!(score.ppq, FL_PPQ as u32);
    assert_eq!(score.notes.len(), 2);
    // A quarter note in at FL's 96 is a quarter note in at Fontelle's 960.
    assert_eq!(score.notes[0].note.start, 96 * PER_FL_TICK);
    assert_eq!(score.notes[0].note.length, 96 * PER_FL_TICK);
    assert_eq!(score.notes[0].note.key, 60);
    assert_eq!(score.notes[1].note.start, 192 * PER_FL_TICK);
    // Where the phrase begins and ends, both measured from zero.
    assert_eq!(score.start, 96 * PER_FL_TICK);
    assert_eq!(score.length, (192 + 96) * PER_FL_TICK);
}

#[test]
fn a_score_from_before_the_record_grew_reads_just_as_well() {
    let bytes = build_fsc(OLD, FL_PPQ, &[note(0, 48), note(48, 55)]);
    let score = read_fsc(&bytes, "Arp").expect("an old score reads");

    assert_eq!(score.notes.len(), 2);
    assert_eq!(score.notes[0].note.key, 48);
    assert_eq!(score.notes[1].note.key, 55);
    assert_eq!(score.notes[1].note.start, 48 * PER_FL_TICK);
}

#[test]
fn the_record_width_comes_from_the_version_and_never_from_the_length() {
    // Six narrow notes are 120 bytes, and so are five wide ones. A reader that
    // guessed from the length would lose a note *and* read every field of the
    // other five out of the wrong place — silently, because every byte of a
    // real note is a plausible value of some other field.
    let six: Vec<FscNoteSpec> = (0..6).map(|i| note(i * 48, 60 + i as u8)).collect();
    let bytes = build_fsc(OLD, FL_PPQ, &six);
    assert_eq!((6 * 20) % 24, 0, "the fixture has to be an ambiguous length");

    let score = read_fsc(&bytes, "Six").expect("an ambiguous-length score reads");
    assert_eq!(score.notes.len(), 6);
    let keys: Vec<u8> = score.notes.iter().map(|n| n.note.key).collect();
    assert_eq!(keys, vec![60, 61, 62, 63, 64, 65]);
}

#[test]
fn every_field_arrives_in_the_units_this_document_stores() {
    let centred = FscNoteSpec {
        // FL's own centres, which are the values it writes for an untouched
        // note. Every one of them has to land on *this* document's neutral,
        // or importing a plain score would pan it, detune it, and lengthen
        // its release.
        pan: 64,
        fine: 120,
        release: 64,
        velocity: 100,
        ..note(0, 60)
    };
    let hard_left = FscNoteSpec {
        pan: 0,
        // One, not zero: zero is a real value in a file this new, and the
        // file too old to have the field at all is its own test below.
        fine: 1,
        ..centred
    };
    let hard_right = FscNoteSpec {
        pan: 128,
        fine: 240,
        ..centred
    };
    let bytes = build_fsc(NEW, FL_PPQ, &[centred, hard_left, hard_right]);
    let score = read_fsc(&bytes, "Field").expect("reads");

    let n = |i: usize| score.notes[i].note;
    assert_eq!(n(0).pan, 0, "FL's centred pan is this document's centre");
    assert_eq!(n(0).fine_pitch, 0, "FL's centred fine pitch is in tune");
    assert_eq!(n(0).velocity, 100);
    assert_eq!(n(0).release, 0, "FL's centred release is the patch's own");

    assert_eq!(n(1).pan, -127, "hard left");
    assert_eq!(n(2).pan, 127, "hard right");
    // FL's fine-pitch knob covers one semitone either way; one step off its
    // bottom is one cent short of that.
    assert_eq!(n(1).fine_pitch, -99);
    assert_eq!(n(2).fine_pitch, 100);
}

#[test]
fn a_release_below_fls_centre_is_the_patchs_own_rather_than_a_shorter_one() {
    // FL's release knob shortens below its centre and lengthens above it;
    // this document's field only lengthens, and `0` means "the patch's own"
    // (see `EventPayload::NoteOn::release`). Reading the bottom half as a
    // fraction of the top would make every note FL shortened *longer* than
    // the instrument asks for, which is the opposite of what was written.
    let bytes = build_fsc(
        NEW,
        FL_PPQ,
        &[
            FscNoteSpec { release: 0, ..note(0, 60) },
            FscNoteSpec { release: 63, ..note(0, 61) },
            FscNoteSpec { release: 64, ..note(0, 62) },
            FscNoteSpec { release: 128, ..note(0, 63) },
        ],
    );
    let score = read_fsc(&bytes, "Rel").expect("reads");
    let rel: Vec<u8> = score.notes.iter().map(|n| n.note.release).collect();
    assert_eq!(rel, vec![0, 0, 0, 127]);
}

#[test]
fn a_score_written_before_fine_pitch_existed_is_read_as_in_tune() {
    // FL 3.0.0 wrote a zero where the fine-pitch byte later went. Read as a
    // value it would be a full semitone flat, which is what every note of the
    // two oldest files in FL's own library would import as.
    let bytes = build_fsc(
        "3.0.0",
        FL_PPQ,
        &[FscNoteSpec { fine: 0, ..note(0, 60) }],
    );
    let score = read_fsc(&bytes, "Ancient").expect("reads");
    assert_eq!(score.notes[0].note.fine_pitch, 0);

    // And in a file new enough to have the field, the same byte is the value
    // it says it is.
    let bytes = build_fsc("3.5.0", FL_PPQ, &[FscNoteSpec { fine: 0, ..note(0, 60) }]);
    let score = read_fsc(&bytes, "Detuned").expect("reads");
    assert_eq!(score.notes[0].note.fine_pitch, -100);
}

#[test]
fn a_note_fl_wrote_at_no_velocity_is_still_a_note() {
    // FL writes velocity 0 on the slide notes in its own library, and this
    // document cannot hold one: a note-on at velocity 0 *is* a note-off
    // everywhere in MIDI, so `NoteProperty::Velocity` starts at 1. Dropping
    // the note instead would silently lose the slide.
    let bytes = build_fsc(
        NEW,
        FL_PPQ,
        &[FscNoteSpec { velocity: 0, slide: true, ..note(0, 60) }],
    );
    let score = read_fsc(&bytes, "Slide").expect("reads");
    assert_eq!(score.notes.len(), 1);
    assert_eq!(score.notes[0].note.velocity, 1);
    assert!(score.notes[0].note.slide, "FL's slide flag is this one");
}

#[test]
fn fls_top_velocity_is_the_top_this_document_has() {
    // FL counts velocity 0..=128 and MIDI counts 0..=127, so exactly one
    // value has nowhere to go. Squeezing the whole scale would move *every*
    // note instead of the one, and 100 — the value on almost every note FL
    // ever wrote — would arrive as 99.
    let bytes = build_fsc(
        NEW,
        FL_PPQ,
        &[
            FscNoteSpec { velocity: 100, ..note(0, 60) },
            FscNoteSpec { velocity: 127, ..note(0, 61) },
            FscNoteSpec { velocity: 128, ..note(0, 62) },
        ],
    );
    let score = read_fsc(&bytes, "Vel").expect("reads");
    let vel: Vec<u8> = score.notes.iter().map(|n| n.note.velocity).collect();
    assert_eq!(vel, vec![100, 127, 127]);
}

#[test]
fn the_files_own_resolution_is_read_and_never_assumed() {
    // Most of FL's library is at 96, and thirty-odd files are at 768. A
    // reader that assumed 96 would put every note of those eight times too
    // far into the song.
    let bytes = build_fsc(OLD, 768, &[note(768, 60)]);
    let score = read_fsc(&bytes, "Fine grid").expect("reads");
    assert_eq!(score.ppq, 768);
    assert_eq!(score.notes[0].note.start, PPQN, "one quarter note in");
    assert_eq!(score.notes[0].note.length, PPQN * 96 / 768);
}

#[test]
fn notes_come_back_in_the_order_they_are_played() {
    let bytes = build_fsc(NEW, FL_PPQ, &[note(192, 60), note(0, 64), note(96, 62)]);
    let score = read_fsc(&bytes, "Order").expect("reads");
    let starts: Vec<i64> = score.notes.iter().map(|n| n.note.start).collect();
    assert_eq!(starts, vec![0, 96 * PER_FL_TICK, 192 * PER_FL_TICK]);
}

#[test]
fn a_score_holding_several_instruments_says_so_and_keeps_them_apart() {
    let bytes = build_fsc(
        NEW,
        FL_PPQ,
        &[
            FscNoteSpec { rack: 0, ..note(0, 36) },
            FscNoteSpec { rack: 2, ..note(0, 60) },
            FscNoteSpec { rack: 0, ..note(96, 38) },
        ],
    );
    let score = read_fsc(&bytes, "Kit").expect("reads");

    assert_eq!(score.rack_channels, vec![0, 2], "in the order FL numbers them");
    assert_eq!(score.notes_on(Some(0)).len(), 2);
    assert_eq!(score.notes_on(Some(2)).len(), 1);
    assert_eq!(score.notes_on(None).len(), 3, "None is the whole score");
}

#[test]
fn a_score_of_one_instrument_says_so_too() {
    let bytes = build_fsc(NEW, FL_PPQ, &[note(0, 60), note(96, 62)]);
    let score = read_fsc(&bytes, "One").expect("reads");
    assert_eq!(score.rack_channels, vec![0]);
}

#[test]
fn the_notes_keep_the_positions_the_file_gives_them() {
    // Read, not helpfully rewritten: a phrase that begins on the second beat
    // begins there on purpose.
    let bytes = build_fsc(NEW, FL_PPQ, &[note(96 * 32, 60), note(96 * 33, 62)]);
    let score = read_fsc(&bytes, "Late").expect("reads");
    assert_eq!(score.notes[0].note.start, 32 * PPQN);
    assert_eq!(score.notes[1].note.start, 33 * PPQN);
    assert_eq!(score.start, 32 * PPQN);
}

#[test]
fn a_phrase_is_the_score_moved_to_where_it_is_being_dropped() {
    // A score saved from bar nine of a pattern is still a phrase, and pasting
    // it should put it where the pointer is rather than eight bars along.
    let bytes = build_fsc(NEW, FL_PPQ, &[note(96 * 32, 60), note(96 * 33, 62)]);
    let score = read_fsc(&bytes, "Late").expect("reads");
    let phrase = score.phrase_on(None);
    assert_eq!(phrase[0].start, 0);
    assert_eq!(phrase[1].start, PPQN);
}

#[test]
fn pulling_one_part_out_of_a_score_does_not_leave_it_the_rest_of_the_leading_rest() {
    // Instrument 0 starts at the top of the phrase and instrument 1 comes in
    // a beat later. Asked for instrument 1 alone, its own first note is where
    // the phrase begins — measuring the offset over the whole score instead
    // would paste it a beat after the pointer.
    let bytes = build_fsc(
        NEW,
        FL_PPQ,
        &[
            FscNoteSpec { rack: 0, ..note(0, 36) },
            FscNoteSpec { rack: 1, ..note(96, 60) },
        ],
    );
    let score = read_fsc(&bytes, "Two parts").expect("reads");
    assert_eq!(score.phrase_on(Some(1))[0].start, 0);
    assert_eq!(score.notes_on(Some(1))[0].start, PPQN, "unmoved, as written");
}

// ------------------------------------------------------------ refusals ---

#[test]
fn a_file_that_is_not_a_score_is_refused_by_name() {
    let error = read_fsc(b"not an FL file at all", "Nope").expect_err("must refuse");
    assert!(
        error.0.contains("FL Studio"),
        "the message should say what it wanted: {error}"
    );
}

#[test]
fn a_truncated_note_block_is_refused_rather_than_half_read() {
    // Half a note is not a note, and reading the block as far as it goes
    // would produce a phrase that is missing its last note with nothing
    // saying so.
    let mut bytes = build_fsc(NEW, FL_PPQ, &[note(0, 60), note(96, 62)]);
    // Ten bytes off the end, and the declared lengths corrected so the file is
    // structurally sound and only the note block is short.
    bytes.truncate(bytes.len() - 10);
    let block_len = bytes.len();
    let events_len = (block_len - 22) as u32;
    bytes[18..22].copy_from_slice(&events_len.to_le_bytes());
    let error = read_fsc(&bytes, "Cut").expect_err("must refuse");
    assert!(
        error.0.contains("note"),
        "the message should say what was wrong: {error}"
    );
}

#[test]
fn a_score_with_no_notes_in_it_is_refused() {
    let bytes = build_fsc_raw(NEW, FL_PPQ, &[]);
    let error = read_fsc(&bytes, "Empty").expect_err("must refuse");
    assert!(
        error.0.contains("no notes"),
        "the message should say what was wrong: {error}"
    );
}

#[test]
fn a_score_with_no_version_in_it_is_refused_rather_than_guessed_at() {
    // Without the version there is no way to know how wide a note is, and a
    // guess is a phrase whose every field is read out of the wrong byte.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"FLhd");
    bytes.extend_from_slice(&6u32.to_le_bytes());
    bytes.extend_from_slice(&16i16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&FL_PPQ.to_le_bytes());
    bytes.extend_from_slice(b"FLdt");
    bytes.extend_from_slice(&0u32.to_le_bytes());
    let error = read_fsc(&bytes, "Anonymous").expect_err("must refuse");
    assert!(
        error.0.contains("version"),
        "the message should say what was missing: {error}"
    );
}

#[test]
fn a_file_whose_resolution_is_zero_is_refused_rather_than_dividing_by_it() {
    let bytes = build_fsc(NEW, 0, &[note(0, 60)]);
    let error = read_fsc(&bytes, "Zero").expect_err("must refuse");
    assert!(error.0.contains("resolution") || error.0.contains("tick"), "{error}");
}

// --------------------------------------------------------- from a path ---

#[test]
fn a_score_read_from_a_path_takes_its_name_from_the_file() {
    let bytes = build_fsc(NEW, FL_PPQ, &[note(0, 60)]);
    let path = write_fsc_to_temp_file("named-score", &bytes);
    let score = import_fsc(&path).expect("reads from disk");
    assert!(
        score.name.starts_with("fontelle-test-named-score"),
        "got {}",
        score.name
    );
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_missing_file_says_which_one() {
    let error = import_fsc(std::path::Path::new("/nowhere/at/all.fsc")).expect_err("must refuse");
    assert!(error.0.contains("all.fsc"), "{error}");
}

// ------------------------------------------------------- the real thing ---

/// Every `.fsc` under `$FONTELLE_FSC_CORPUS`, read for real.
///
/// Ignored by default and driven by an environment variable because the corpus
/// is FL Studio's own factory library: it is not this project's to check in,
/// and a test that needed it would fail on every machine without FL installed.
/// Point it at `.../FL Studio/Data/Patches/Scores` and run
/// `cargo test -p fontelle-assets --test fsc_import -- --ignored`.
///
/// What it asserts is the thing a hand-built fixture cannot: that the rules
/// hold against files this code has never seen, written by twelve versions of
/// somebody else's program over twenty years.
#[test]
#[ignore = "needs FL Studio's factory score library; set FONTELLE_FSC_CORPUS"]
fn a_whole_library_of_real_scores_reads_cleanly() {
    let Ok(root) = std::env::var("FONTELLE_FSC_CORPUS") else {
        panic!("set FONTELLE_FSC_CORPUS to a folder of .fsc files");
    };
    let mut files = Vec::new();
    collect(std::path::Path::new(&root), &mut files);
    assert!(!files.is_empty(), "no .fsc files under {root}");

    let mut notes = 0usize;
    let mut failures = Vec::new();
    for path in &files {
        match import_fsc(path) {
            Ok(score) => {
                assert!(!score.notes.is_empty(), "{} read as empty", path.display());
                for held in &score.notes {
                    // Every field inside what the document may hold. A score
                    // read out of the wrong byte offset fails here rather
                    // than arriving as music nobody wrote.
                    let n = held.note;
                    assert!(n.key <= 127, "{}: key {}", path.display(), n.key);
                    assert!(n.velocity >= 1, "{}: velocity 0", path.display());
                    assert!(n.length >= 1, "{}: zero-length note", path.display());
                    assert!(n.start >= 0, "{}: negative start", path.display());
                    assert!(
                        (-1200..=1200).contains(&(n.fine_pitch as i32)),
                        "{}: fine pitch {}",
                        path.display(),
                        n.fine_pitch
                    );
                    notes += 1;
                }
            }
            Err(e) => failures.push(format!("{}: {e}", path.display())),
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} scores failed:\n{}",
        failures.len(),
        files.len(),
        failures.join("\n")
    );
    println!("read {notes} notes from {} scores", files.len());
}

fn collect(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(listing) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in listing.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("fsc"))
        {
            out.push(path);
        }
    }
}

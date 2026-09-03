//! Item 12 of `docs/first-usable-plan.md`: a whole piece, start to finish.
//!
//! > *"Make an actual multi-part piece in the app, start to finish, on
//! > hardware — the equivalent of M0's 'heard on real hardware' clause,
//! > applied to the whole gate sentence in §3."*
//!
//! The ears are the user's; a test cannot have them, and this file does not
//! pretend to. What it can do is the other half, and it is the half that has
//! found every defect in this project so far: **make the piece through the
//! same trait the window calls**, in the order a person works, and then check
//! what came out. Not `Session`'s internals and not the model's — the seven
//! dead controls this gate kept turning up (tempo, the mixer's gain and pan,
//! the cut tool, glide time, retrigger mode, the projects folder, the bank's
//! folders, and the four note properties) were every one of them a thing the
//! model could do and the window could not reach.
//!
//! So the shape here is deliberately long rather than granular: one test that
//! is a *session*, and a handful after it that pull on the joins a session
//! crosses. A per-feature test proves a feature works. A session proves the
//! features work **in each other's presence**, which is a different claim and
//! the only one that closes this gate.

mod common;

use std::path::PathBuf;

use fontelle_app::{RealiseOptions, SampleLibrary, Session, blank_project};
use fontelle_assets::fixtures::{
    GEN_KEY_RANGE, GEN_OVERRIDING_ROOT_KEY, GEN_SAMPLE_MODES, Sf2Fixture, ZoneSpec, build_sf2,
    gen_range, gen_val,
};
use fontelle_engine::{graph_channel, timeline_channel};
use fontelle_model::Note;
use fontelle_types::{CompiledTimeline, EventPayload, PPQN, Tick};
use fontelle_ui::canvas::{ArrangeEdit, LaneProperty, RollEdit};
use fontelle_ui::document::{DocumentHost, StudioHost};

use common::SR;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("fontelle-shake-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("the scratch folder must be creatable");
    path
}

/// A bank folder holding two real, loadable soundfonts, one of them inside a
/// subfolder — because that is what a bank looks like.
fn a_bank(name: &str) -> PathBuf {
    let dir = scratch(name);
    let fixture = |root: u8| Sf2Fixture {
        samples: (0..64).map(|i| (i * 500 - 16_000) as i16).collect(),
        sample_rate: 44_100,
        header_start: 0,
        header_end: 64,
        header_loop_start: 8,
        header_loop_end: 56,
        origpitch: root,
        pitchadj: 0,
        zone: ZoneSpec {
            generators: vec![
                gen_range(GEN_KEY_RANGE, 0, 127),
                gen_val(GEN_OVERRIDING_ROOT_KEY, i16::from(root)),
                gen_val(GEN_SAMPLE_MODES, 1),
            ],
        },
        extra_zones: Vec::new(),
    };
    std::fs::write(dir.join("Piano.sf2"), build_sf2(&fixture(60))).unwrap();
    std::fs::create_dir_all(dir.join("Bass")).unwrap();
    std::fs::write(dir.join("Bass").join("Sub.sf2"), build_sf2(&fixture(36))).unwrap();
    dir
}

fn options() -> RealiseOptions {
    RealiseOptions {
        sample_rate: SR,
        block_size: fontelle_engine::BLOCK_SIZE,
        quality: fontelle_app::PLAYBACK_QUALITY,
    }
}

/// A session over a blank project, with a bank and a projects folder — the
/// window's state on a first run, minus the window.
fn studio(bank: &std::path::Path, projects: &std::path::Path) -> Session {
    let project = blank_project(8, 120.0, SR);
    let clip = Session::first_clip(&project).expect("a blank project has one clip");
    let channel_nodes = fontelle_app::channel_nodes(&project);
    let (publisher, _timeline) = timeline_channel(CompiledTimeline::empty());
    let library = SampleLibrary::new();
    let realised = fontelle_app::realise(&project, &library, options()).unwrap();
    let (graphs, _source) = graph_channel(realised.graph);

    let mut session = Session::new(
        project,
        library,
        channel_nodes,
        publisher,
        options(),
        clip,
        None,
    )
    .with_graphs(graphs, realised.track_controls)
    .with_param_nodes(realised.param_nodes)
    // Never the developer's own config: `open_bank` writes what it settled on.
    .with_settings_path(bank.join("settings.json"));
    session.add_soundfont_dir(bank);
    session.open_bank();
    session.set_projects_dir(Some(projects.to_path_buf()));
    session
}

/// The loudest sample in a canonical 16-bit WAV — everything past the 44-byte
/// header, read as little-endian pairs.
fn peak_of(bytes: &[u8]) -> u16 {
    bytes
        .get(44..)
        .unwrap_or_default()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| i16::from_le_bytes(*pair).unsigned_abs())
        .max()
        .unwrap_or(0)
}

/// One note-on, flattened: sample, key, and the four properties that took
/// until 2026-08-31 to reach a voice.
type NoteOn = (i64, u8, i16, u8, u8, u8);

/// Every note-on in the published timeline.
fn note_ons(session: &Session) -> Vec<NoteOn> {
    let mut out: Vec<_> = session
        .compiled()
        .events
        .iter()
        .filter_map(|event| match event.payload {
            EventPayload::NoteOn {
                key,
                fine_pitch,
                release,
                mod_x,
                mod_y,
                ..
            } => Some((event.sample, key, fine_pitch, release, mod_x, mod_y)),
            _ => None,
        })
        .collect();
    out.sort_unstable();
    out
}

// ---------------------------------------------------------------- the piece

#[test]
fn a_whole_piece_made_the_way_a_person_makes_one() {
    let bank = a_bank("piece");
    let projects = scratch("piece-projects");
    let mut s = studio(&bank, &projects);

    // --- 1. A new project, in the folder that was just chosen.
    s.new_project().expect("a new project must be makeable");
    assert_eq!(s.projects().len(), 1, "and it shows up in the browser");
    assert!(!s.is_dirty(), "a project just made and saved is not dirty");

    // --- 2. The song's own tempo and metre, off the defaults.
    s.set_tempo(92.0);
    s.set_beats_per_bar(3);
    assert_eq!(s.tempo(), 92.0);
    assert_eq!(s.beats_per_bar(), 3);

    // --- 3. Two instruments, one of them from a folder inside the bank.
    //
    // Through `open_file` and `add_channel_with` rather than by reaching into
    // the project: loading a soundfont and picking a preset out of it is two
    // steps in the window and has to stay two steps here.
    let piano = s
        .library_files()
        .iter()
        .position(|f| f.name == "Piano")
        .expect("the bank's loose soundfont");
    s.open_file(piano).expect("the soundfont must load");
    s.add_channel_with(0).expect("its first preset");

    let folder = s
        .library_files()
        .iter()
        .position(|f| f.name == "Bass")
        .expect("the bank's folder, listed as a folder");
    s.open_file(folder)
        .expect("a folder opens rather than loads");
    let sub = s
        .library_files()
        .iter()
        .position(|f| f.name == "Sub")
        .expect("the soundfont inside it");
    s.open_file(sub).expect("which does load");
    s.add_channel_with(0).expect("and gives a second channel");

    assert_eq!(
        s.channels().len(),
        3,
        "the blank channel plus the two added"
    );

    // --- 4. A mixer built by hand, and the channels routed into it.
    //
    // Strip indices are the host's, which run tracks-then-master: the mixer
    // panel lays master out last and `route_names` lists it last, so the two
    // tracks just made are 0 and 1. (The route *menu* draws Master at the top
    // as its own row — a display order, not this one.)
    s.add_mixer_track();
    s.add_mixer_track();
    s.rename_mixer_track(0, "Keys");
    s.rename_mixer_track(1, "Low");
    s.set_channel_route(1, Some(0));
    s.set_channel_route(2, Some(1));
    s.set_track_gain_db(0, -6.0);
    s.set_track_pan(0, -0.5);
    s.set_track_gain_db(1, -3.0);

    let strips = s.mixer_strips();
    assert_eq!(strips.len(), 3, "the two tracks and master");
    assert!(strips[2].is_master, "master is the last strip");
    assert_eq!(strips[0].name, "Keys");
    assert!((strips[0].gain_db + 6.0).abs() < 0.01);
    assert!((strips[0].pan + 0.5).abs() < 0.01);
    let routes = s.channels();
    assert_eq!(routes[1].route, Some(0), "the piano goes to Keys");
    assert_eq!(routes[2].route, Some(1), "the bass goes to Low");

    // --- 5. Clips on the arrangement, drawn rather than inherited.
    let lanes = s.lanes().len();
    assert!(lanes >= 2, "a lane per channel to draw on");
    let bass_clip = s.arrange(ArrangeEdit::Add {
        lane: 1,
        start: PPQN * 12,
    });
    assert_eq!(bass_clip.clips.len(), 1, "the draw tool makes exactly one clip");

    // --- 6. Notes in each, with the properties that took until today to hear.
    let first = s.clips()[0].id;
    s.open_clip(first);
    let mut written = Vec::new();
    for (index, key) in [60u8, 64, 67, 72].into_iter().enumerate() {
        let ids = s.edit(RollEdit::Add {
            note: Note {
                start: PPQN * index as Tick,
                length: PPQN,
                key,
                velocity: 100,
                pan: 0,
                fine_pitch: 0,
                release: 0,
                mod_x: 0,
                mod_y: 0,
                slide: false,
            },
        });
        s.end_gesture();
        written.extend(ids);
    }
    assert_eq!(written.len(), 4);

    s.edit(RollEdit::SetProperty {
        ids: vec![written[1]],
        property: LaneProperty::FinePitch,
        value: -50,
    });
    s.end_gesture();
    s.edit(RollEdit::SetProperty {
        ids: vec![written[2]],
        property: LaneProperty::Release,
        value: 110,
    });
    s.end_gesture();
    s.edit(RollEdit::SetProperty {
        ids: vec![written[0]],
        property: LaneProperty::ModX,
        value: 64,
    });
    s.end_gesture();
    // On a *different* note from the properties above, because a slide note
    // does not start a voice — it bends the one already sounding — so it
    // compiles to a `NoteSlide` and never appears as a note-on at all. Asking
    // a slide note what its mod X compiled to is asking the wrong question,
    // which this test did on its first run.
    s.edit(RollEdit::SetSlide {
        ids: vec![written[3]],
        slide: true,
    });
    s.end_gesture();

    // --- 7. And the first clip loops, rather than being copied four times.
    s.arrange(ArrangeEdit::SetLoop {
        ids: vec![first],
        loop_length: Some(PPQN * 4),
    });
    s.end_gesture();

    // --- 8. What the audio thread would be given.
    let sounding: Vec<NoteOn> = note_ons(&s);
    assert!(
        sounding.len() > 4,
        "a looped clip plays its notes more than once: {} note-ons",
        sounding.len()
    );
    assert!(
        sounding.iter().any(|n| n.2 == -50),
        "the detuned note reaches the timeline"
    );
    assert!(
        sounding.iter().any(|n| n.3 == 110),
        "so does the one with a long release"
    );
    assert!(
        sounding.iter().any(|n| n.4 == 64),
        "and the one with mod X drawn on it"
    );
    assert!(
        s.compiled()
            .events
            .iter()
            .any(|e| matches!(e.payload, EventPayload::NoteSlide { .. })),
        "and the slide note is a slide rather than a fourth note-on"
    );

    // --- 9. Saved, rendered, and put away.
    assert!(s.is_dirty(), "all of that is unsaved work");
    s.save().expect("the project must save");
    assert!(!s.is_dirty());
    let message = s.export_wav().expect("and render");
    assert!(
        message.contains(".wav") || message.contains("render"),
        "the export says where it went: {message}"
    );

    // --- 10. Opened again from the browser, it is the same piece.
    let mut s = studio(&bank, &projects);
    let index = s
        .projects()
        .iter()
        .position(|p| !p.name.is_empty())
        .expect("the project is listed");
    s.open_project(index).expect("and opens");

    assert_eq!(s.tempo(), 92.0, "the tempo survived the round trip");
    assert_eq!(s.beats_per_bar(), 3, "so did the metre");
    assert_eq!(s.channels().len(), 3, "and every channel");
    let strips = s.mixer_strips();
    assert_eq!(strips.len(), 3, "and the mixer that was built by hand");
    assert_eq!(strips[0].name, "Keys");
    assert!((strips[0].gain_db + 6.0).abs() < 0.01, "and its fader");
    assert!((strips[0].pan + 0.5).abs() < 0.01, "and its pan");
    assert_eq!(s.channels()[1].route, Some(0), "and the routing");

    let reopened = note_ons(&s);
    assert_eq!(
        reopened, sounding,
        "and every note, with every property, in the same place"
    );

    std::fs::remove_dir_all(&bank).ok();
    std::fs::remove_dir_all(&projects).ok();
}

// ------------------------------------------------------- the joins it crosses

#[test]
fn undoing_the_whole_session_gets_back_to_where_it_started() {
    // Every edit is a `Command` (INVARIANT 9), so a person who has just made a
    // mess can get out of it. The failure this guards is the one a long
    // session produces and a short test never does: an edit that quietly does
    // not push history, so undo steps past it and the document ends up in a
    // state no sequence of edits could have produced.
    //
    // Undone to exhaustion rather than once per call, because how many
    // commands an action decomposes into is the session's business, not the
    // caller's — adding a channel with an instrument is legitimately more than
    // one. What has to hold is that undoing everything and redoing everything
    // are each other's inverse, and *that* is a claim a count would only
    // obscure.
    let bank = a_bank("undo");
    let projects = scratch("undo-projects");
    let mut s = studio(&bank, &projects);
    s.new_project().unwrap();

    let clip = s.clips()[0].id;
    s.open_clip(clip);

    /// Everything a person would notice had changed. The tempo as bits so the
    /// whole thing can be compared with `==` — an f64 that came back from a
    /// round of undo is either the same number or a bug.
    type Snapshot = (u64, usize, usize, usize, Vec<NoteOn>);

    fn state(s: &mut Session) -> Snapshot {
        (
            s.tempo().to_bits(),
            s.channels().len(),
            s.mixer_strips().len(),
            s.notes().len(),
            note_ons(s),
        )
    }

    let before = state(&mut s);

    let piano = s
        .library_files()
        .iter()
        .position(|f| f.name == "Piano")
        .unwrap();
    s.open_file(piano).unwrap();

    s.set_tempo(140.0);
    s.add_channel_with(0).unwrap();
    s.add_mixer_track();
    s.set_channel_route(1, Some(0));
    let ids = s.edit(RollEdit::Add {
        note: Note {
            start: 0,
            length: PPQN,
            key: 60,
            velocity: 100,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            slide: false,
        },
    });
    s.end_gesture();
    s.edit(RollEdit::SetProperty {
        ids,
        property: LaneProperty::ModY,
        value: 90,
    });
    s.end_gesture();

    let after = state(&mut s);
    assert_ne!(before, after, "the session did something");

    // Generously past however many commands that was.
    for _ in 0..64 {
        s.undo();
    }
    assert_eq!(
        state(&mut s),
        before,
        "undoing to exhaustion should land exactly where the session began"
    );

    for _ in 0..64 {
        s.redo();
    }
    assert_eq!(
        state(&mut s),
        after,
        "and redoing it should put back what was undone, not something like it"
    );

    std::fs::remove_dir_all(&bank).ok();
    std::fs::remove_dir_all(&projects).ok();
}

#[test]
fn a_piece_with_no_notes_in_it_still_saves_opens_and_renders() {
    // The other end of the session: everything above with nothing written yet,
    // which is the state a project spends its first minute in and the one an
    // empty-collection bug hides in.
    let bank = a_bank("empty");
    let projects = scratch("empty-projects");
    let mut s = studio(&bank, &projects);

    s.new_project().unwrap();
    s.set_tempo(101.0);
    s.save().unwrap();
    s.export_wav()
        .expect("an empty piece renders to silence, not an error");

    let mut s = studio(&bank, &projects);
    s.open_project(0).unwrap();
    assert_eq!(s.tempo(), 101.0);
    assert!(note_ons(&s).is_empty());

    std::fs::remove_dir_all(&bank).ok();
    std::fs::remove_dir_all(&projects).ok();
}

#[test]
fn the_piece_you_made_is_audible_in_the_file_you_rendered() {
    // The end of the whole chain, and the failure that every layer above can
    // pass while producing: notes in the document, events on the timeline, a
    // graph that realises, a WAV of the right length — and silence inside it.
    // Nothing else in the suite reads the rendered samples back.
    let bank = a_bank("audible");
    let projects = scratch("audible-projects");
    let mut s = studio(&bank, &projects);
    s.new_project().unwrap();

    let piano = s
        .library_files()
        .iter()
        .position(|f| f.name == "Piano")
        .unwrap();
    s.open_file(piano).unwrap();
    s.add_channel_with(0).unwrap();

    // Onto the channel that has the instrument, not the blank one that came
    // with the project.
    let clip = s
        .clips()
        .last()
        .map(|c| c.id)
        .expect("the new channel brought a clip");
    s.open_clip(clip);
    for (index, key) in [60u8, 64, 67].into_iter().enumerate() {
        s.edit(RollEdit::Add {
            note: Note {
                start: PPQN * index as Tick,
                length: PPQN,
                key,
                velocity: 110,
                pan: 0,
                fine_pitch: 0,
                release: 0,
                mod_x: 0,
                mod_y: 0,
                slide: false,
            },
        });
        s.end_gesture();
    }
    assert_eq!(note_ons(&s).len(), 3, "three notes on the timeline");

    s.save().unwrap();
    s.export_wav().expect("the render must succeed");

    let renders = s
        .bundle_path()
        .expect("a saved project has a bundle")
        .join("renders");
    let wav = std::fs::read_dir(&renders)
        .expect("the renders folder exists")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "wav"))
        .expect("with a wav in it");
    let bytes = std::fs::read(&wav).expect("which is readable");

    // 16-bit little-endian PCM after a 44-byte canonical header. Read as
    // samples rather than as a length, because a file of the right size full
    // of zeroes is exactly the bug being looked for.
    assert!(bytes.len() > 44, "the render has audio in it at all");
    let peak = peak_of(&bytes);
    assert!(
        peak > 300,
        "the rendered file should carry the notes that were written; peak was {peak}"
    );

    // And the control that makes the number above mean something: the same
    // session with the notes taken out again renders a file that is quiet.
    // Without this, a peak of 300 could be anything — a header misread as
    // audio, a click at the start, a DC offset — and the assertion would pass
    // on a project that produced no music at all.
    let ids: Vec<_> = s.notes().keys().collect();
    s.edit(RollEdit::Remove(ids));
    s.end_gesture();
    s.save().unwrap();
    s.export_wav().unwrap();
    let silent = std::fs::read_dir(&renders)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p != &wav && p.extension().is_some_and(|e| e == "wav"))
        .expect("the second render is a second file, not an overwrite");
    let bytes = std::fs::read(&silent).unwrap();
    let quiet = peak_of(&bytes);
    assert!(
        quiet < 32,
        "a piece with its notes removed should render silence; peak was {quiet}"
    );

    std::fs::remove_dir_all(&bank).ok();
    std::fs::remove_dir_all(&projects).ok();
}

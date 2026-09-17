//! Walking a soundfont collection folder by folder (TDD §17.5).
//!
//! Reported from using the window:
//!
//! > *"Make sure the soundfonts browser is able to handle multiple files and
//! > folders so I can see other folders in there and click into them to see
//! > their contents. Right now I'm only seeing just soundfont names even
//! > though my soundfont directory has folders."*
//!
//! Exactly what it did: [`SoundfontBank::rescan`] walked the whole tree and
//! **flattened** it into one list of file names. A collection organised as
//! `Orchestral/Strings/…` and `Drums/Kits/…` came out as two hundred names in
//! one alphabetical heap, with the organisation — which is information the
//! user put there on purpose — thrown away.
//!
//! # Browsing and searching are different questions
//!
//! The flat index is not a mistake, it is the other half. So the panel does
//! both, and **the search box decides which**:
//!
//! - **Empty query — browse.** Folders first, then the soundfonts in *this*
//!   folder, with a row back up. The structure is the answer.
//! - **Anything typed — search.** The whole collection, flat, each hit
//!   labelled with the folder it is in. §17.5 calls instant fuzzy search the
//!   feature that makes a large collection usable, and a search that only
//!   looked in the folder you happen to be standing in would not be it.

mod common;

use std::path::{Path, PathBuf};

use fontelle_app::bank::{BankRow, SoundfontBank};

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("fontelle-browse-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("the scratch folder must be creatable");
    path
}

/// Writes a **real, spec-valid** soundfont — the same hand-built kind the rest
/// of this workspace's tests use.
///
/// Real rather than sixteen zero bytes, because opening a row parses the
/// file's headers: a browser test that never opened anything would pass over a
/// collection of rubbish, and then the one test that does open something
/// would be the only one that noticed.
fn touch(dir: &Path, name: &str) {
    use fontelle_assets::fixtures::{
        GEN_KEY_RANGE, GEN_OVERRIDING_ROOT_KEY, Sf2Fixture, ZoneSpec, build_sf2, gen_range, gen_val,
    };
    std::fs::create_dir_all(dir).unwrap();
    if !name.ends_with(".sf2") {
        std::fs::write(dir.join(name), b"not a soundfont").unwrap();
        return;
    }
    let fixture = Sf2Fixture {
        samples: (0..32).map(|i| (i * 500 - 8_000) as i16).collect(),
        sample_rate: 44_100,
        header_start: 0,
        header_end: 32,
        header_loop_start: 4,
        header_loop_end: 28,
        origpitch: 60,
        pitchadj: 0,
        zone: ZoneSpec {
            generators: vec![
                gen_range(GEN_KEY_RANGE, 0, 127),
                gen_val(GEN_OVERRIDING_ROOT_KEY, 60),
            ],
        },
        extra_zones: Vec::new(),
    };
    std::fs::write(dir.join(name), build_sf2(&fixture)).unwrap();
}

/// A collection with folders in it, the way people actually organise one.
fn a_collection(name: &str) -> PathBuf {
    let root = scratch(name);
    touch(&root, "Loose Piano.sf2");
    touch(&root, "notes.txt");
    touch(&root.join("Drums"), "Kit A.sf2");
    touch(&root.join("Drums"), "Kit B.sf2");
    touch(&root.join("Orchestral").join("Strings"), "Violins.sf2");
    touch(&root.join("Orchestral").join("Brass"), "Horns.sf2");
    root
}

/// The names of the rows, in order, with folders marked.
fn names(bank: &SoundfontBank) -> Vec<String> {
    bank.rows()
        .iter()
        .map(|row| match row {
            BankRow::Up { .. } => "..".to_string(),
            BankRow::Folder { name, .. } => format!("[{name}]"),
            BankRow::File(entry) => entry.name.clone(),
        })
        .collect()
}

fn open(bank: &mut SoundfontBank, name: &str) {
    let index = bank
        .rows()
        .iter()
        .position(|row| matches!(row, BankRow::Folder { name: n, .. } if n == name))
        .unwrap_or_else(|| panic!("no folder called {name} in {:?}", names(bank)));
    bank.open_row(index);
}

// ------------------------------------------------------------- browsing ---

#[test]
fn the_top_of_a_single_folder_collection_is_its_contents() {
    // Not the folder itself: a bank with one root should not make you click
    // into it before you can see anything.
    let root = a_collection("top");
    let mut bank = SoundfontBank::new(vec![root.clone()]);
    bank.rescan();

    assert_eq!(
        names(&bank),
        vec!["[Drums]", "[Orchestral]", "Loose Piano"],
        "folders first, then files, each alphabetical"
    );
    assert_eq!(bank.at(), Some(root.as_path()));

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_folder_can_be_opened_and_shows_what_is_inside_it() {
    let root = a_collection("open");
    let mut bank = SoundfontBank::new(vec![root.clone()]);
    bank.rescan();

    open(&mut bank, "Drums");
    assert_eq!(names(&bank), vec!["..", "Kit A", "Kit B"]);
    assert_eq!(bank.at(), Some(root.join("Drums").as_path()));

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn folders_nest_and_the_way_back_is_always_the_first_row() {
    let root = a_collection("nest");
    let mut bank = SoundfontBank::new(vec![root.clone()]);
    bank.rescan();

    open(&mut bank, "Orchestral");
    assert_eq!(names(&bank), vec!["..", "[Brass]", "[Strings]"]);
    open(&mut bank, "Strings");
    assert_eq!(names(&bank), vec!["..", "Violins"]);

    // And back out again, one level at a time.
    bank.open_row(0);
    assert_eq!(bank.at(), Some(root.join("Orchestral").as_path()));
    bank.open_row(0);
    assert_eq!(bank.at(), Some(root.as_path()));

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn the_root_has_no_way_up_because_there_is_nowhere_above_it() {
    // Fontelle writes and reads nothing outside the folders the user named
    // (INVARIANT 10), and a browser that could walk to `/` would be offering
    // exactly that.
    let root = a_collection("root");
    let mut bank = SoundfontBank::new(vec![root.clone()]);
    bank.rescan();

    assert!(
        !bank.rows().iter().any(|r| matches!(r, BankRow::Up { .. })),
        "{:?}",
        names(&bank)
    );
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn several_configured_folders_are_listed_as_the_top_level() {
    // With more than one root there *is* a level above them, and it is the
    // list of the folders the user configured.
    let one = a_collection("many-one");
    let two = scratch("many-two");
    touch(&two, "Extra.sf2");

    let mut bank = SoundfontBank::new(vec![one.clone(), two.clone()]);
    bank.rescan();

    assert_eq!(bank.at(), None, "the top is the roots, not a folder");
    let rows = names(&bank);
    assert_eq!(rows.len(), 2, "one row per configured folder: {rows:?}");
    assert!(rows.iter().all(|r| r.starts_with('[')));

    open(&mut bank, two.file_name().unwrap().to_str().unwrap());
    assert_eq!(names(&bank), vec!["..", "Extra"]);
    // And up from a root goes back to the list of roots.
    bank.open_row(0);
    assert_eq!(bank.at(), None);

    std::fs::remove_dir_all(&one).ok();
    std::fs::remove_dir_all(&two).ok();
}

#[test]
fn a_folder_row_says_how_many_soundfonts_are_under_it() {
    // The one number worth showing: a folder with nothing in it looks exactly
    // like a folder with a hundred files until you click it.
    let root = a_collection("counts");
    let mut bank = SoundfontBank::new(vec![root.clone()]);
    bank.rescan();

    let count = |want: &str| {
        bank.rows()
            .iter()
            .find_map(|row| match row {
                BankRow::Folder { name, files, .. } if name == want => Some(*files),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no folder {want}"))
    };
    assert_eq!(count("Drums"), 2);
    assert_eq!(count("Orchestral"), 2, "counted through its subfolders");

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn an_empty_folder_is_still_shown_rather_than_hidden() {
    // Hiding it would make "I put my kits in there" unanswerable: the folder
    // you are looking for would simply not be in the list.
    let root = scratch("empty-folder");
    std::fs::create_dir_all(root.join("Nothing Yet")).unwrap();
    let mut bank = SoundfontBank::new(vec![root.clone()]);
    bank.rescan();

    assert_eq!(names(&bank), vec!["[Nothing Yet]"]);
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn opening_a_file_row_is_not_a_folder_move() {
    let root = a_collection("file-row");
    let mut bank = SoundfontBank::new(vec![root.clone()]);
    bank.rescan();
    let before = bank.at().map(Path::to_path_buf);

    let index = bank
        .rows()
        .iter()
        .position(|r| matches!(r, BankRow::File(_)))
        .expect("there is a file here");
    assert!(
        !bank.open_row(index),
        "a file row does not move the browser"
    );
    assert_eq!(bank.at().map(Path::to_path_buf), before);

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_folder_that_disappears_under_you_leaves_the_browser_somewhere_real() {
    // The folder is on somebody's disk and Fontelle is not the only thing
    // that can move it.
    let root = a_collection("vanish");
    let mut bank = SoundfontBank::new(vec![root.clone()]);
    bank.rescan();
    open(&mut bank, "Drums");

    std::fs::remove_dir_all(root.join("Drums")).unwrap();
    bank.rescan();

    assert_eq!(
        bank.at(),
        Some(root.as_path()),
        "it falls back to somewhere that exists rather than listing nothing"
    );
    std::fs::remove_dir_all(&root).ok();
}

// -------------------------------------------------------------- searching ---

#[test]
fn a_search_looks_through_the_whole_collection_and_not_just_this_folder() {
    // §17.5 calls instant fuzzy search the feature that makes a large
    // collection usable. A search that only looked where you were standing
    // would not be it.
    let root = a_collection("search");
    let mut bank = SoundfontBank::new(vec![root.clone()]);
    bank.rescan();

    let hits = bank.search("vio");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].name, "Violins");
    assert_eq!(
        bank.at(),
        Some(root.as_path()),
        "searching does not move you"
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_search_hit_says_which_folder_it_is_in() {
    // Two files called "Kit" in different folders are the commonest thing in a
    // collection, and a flat list of names cannot tell them apart.
    let root = a_collection("where");
    let mut bank = SoundfontBank::new(vec![root.clone()]);
    bank.rescan();

    let hits = bank.search("horns");
    assert_eq!(hits.len(), 1);
    assert_eq!(
        bank.folder_of(hits[0]),
        "Orchestral/Brass",
        "relative to the configured folder, which is the part that varies"
    );

    // A file sitting in the root has no folder to name.
    let hits = bank.search("loose");
    assert_eq!(bank.folder_of(hits[0]), "");

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn the_flat_index_still_holds_everything_under_every_root() {
    let root = a_collection("index");
    let mut bank = SoundfontBank::new(vec![root.clone()]);
    bank.rescan();

    let mut all: Vec<&str> = bank.entries().iter().map(|e| e.name.as_str()).collect();
    all.sort_unstable();
    assert_eq!(
        all,
        vec!["Horns", "Kit A", "Kit B", "Loose Piano", "Violins"]
    );

    std::fs::remove_dir_all(&root).ok();
}

// ------------------------------------------ through the panel's own seam ---

mod through_the_session {
    //! The same thing again, driven the way the window drives it — one list of
    //! rows and one "activate row N", because that is all a panel has.

    use fontelle_app::{RealiseOptions, SampleLibrary, Session};
    use fontelle_engine::timeline_channel;
    use fontelle_types::CompiledTimeline;
    use fontelle_ui::document::{LibraryKind, StudioHost};

    use super::{a_collection, scratch};

    const SR: u32 = 48_000;

    fn studio(dir: &std::path::Path) -> Session {
        let project = crate::common::a_project_with_a_clip(8, 120.0, SR);
        let clip = Session::first_clip(&project).expect("a blank project has one clip");
        let channel_nodes = fontelle_app::channel_nodes(&project);
        let (publisher, _timeline) = timeline_channel(CompiledTimeline::empty());
        let options = RealiseOptions {
            sample_rate: SR,
            block_size: fontelle_engine::BLOCK_SIZE,
            quality: fontelle_app::PLAYBACK_QUALITY,
        };
        let mut session = Session::new(
            project,
            SampleLibrary::new(),
            channel_nodes,
            publisher,
            options,
            clip,
            None,
        )
        // Never the developer's own `~/.config/fontelle`.
        .with_settings_path(dir.join("settings.json"));
        session.add_soundfont_dir(dir);
        session.open_bank();
        // These are the soundfont browser's tests; the studio opens on the
        // Import tab now, so the tab is switched the way a click would.
        session.set_browser_mode(fontelle_ui::canvas::BrowserMode::Sounds);
        session
    }

    /// The **bank's** rows: the list the panel draws, without the built-in
    /// instrument's row at the top of it.
    ///
    /// Flopsynth is in this list because everything a hundred and twenty-eight
    /// presets need is already here — a search, a virtualised draw, headings,
    /// a click that puts one on a channel. It is not a soundfont, though, and
    /// every test in this module is about soundfonts, so they all ask for the
    /// bank rather than for the list.
    fn rows(session: &Session) -> Vec<(String, LibraryKind)> {
        bank_rows(session)
    }

    fn bank_rows(session: &Session) -> Vec<(String, LibraryKind)> {
        session
            .library_files()
            .into_iter()
            .skip(built_in_rows(session))
            .map(|e| (e.name, e.kind))
            .collect()
    }

    /// How many rows in front of the bank belong to the built-in instruments.
    fn built_in_rows(session: &Session) -> usize {
        usize::from(
            session
                .library_files()
                .first()
                .is_some_and(|first| first.name == "Flopsynth"),
        )
    }

    /// A row index into the bank, as an index into the list the panel draws.
    fn bank_row(session: &Session, index: usize) -> usize {
        index + built_in_rows(session)
    }

    #[test]
    fn the_panel_is_handed_folders_and_files_and_can_tell_them_apart() {
        let dir = a_collection("session-rows");
        let session = studio(&dir);

        assert_eq!(
            rows(&session),
            vec![
                ("Drums".to_string(), LibraryKind::Folder),
                ("Orchestral".to_string(), LibraryKind::Folder),
                ("Loose Piano".to_string(), LibraryKind::File),
            ]
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn activating_a_folder_row_goes_in_and_activating_a_file_row_opens_it() {
        // One method for both, because a panel has one click. The host knows
        // what row it was; the panel only needs a glyph.
        let dir = a_collection("session-open");
        let mut session = studio(&dir);

        session
            .open_file(bank_row(&session, 0))
            .expect("Drums is a folder");
        assert_eq!(
            rows(&session),
            vec![
                ("..".to_string(), LibraryKind::Up),
                ("Kit A".to_string(), LibraryKind::File),
                ("Kit B".to_string(), LibraryKind::File),
            ]
        );
        assert_eq!(
            session.selected_file(),
            None,
            "nothing in this folder is open yet"
        );

        // And back up.
        session.open_file(0).expect("the way up");
        assert_eq!(rows(&session).len(), 3);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_folder_row_says_what_is_in_it() {
        let dir = a_collection("session-detail");
        let session = studio(&dir);
        let entries = session.library_files();
        let first = built_in_rows(&session);
        assert_eq!(entries[first].name, "Drums");
        assert_eq!(entries[first].detail, "2 sf2");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn typing_searches_the_whole_collection_and_clearing_it_puts_you_back() {
        // The search box decides which question is being asked, and asking the
        // other one must not lose where you were standing.
        let dir = a_collection("session-search");
        let mut session = studio(&dir);
        session
            .open_file(bank_row(&session, 0))
            .expect("into Drums");

        session.set_query("vio");
        let found = rows(&session);
        assert_eq!(
            found,
            vec![("Violins".to_string(), LibraryKind::File)],
            "a search reaches outside the folder you are in"
        );
        assert_eq!(
            // A search lists soundfonts wherever they are, and the built-in
            // instrument is not one — so its row is not in a search's results.
            session.library_files()[0].detail,
            "Orchestral/Strings",
            "and says where it found it"
        );

        session.set_query("");
        assert_eq!(
            rows(&session)[0].0,
            "..",
            "clearing the box puts you back where you were browsing"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_open_file_stays_open_across_a_search_and_back() {
        // The highlight is worked out from the file's *path*, not from a row
        // number — the list under it moves constantly.
        let dir = a_collection("session-highlight");
        let mut session = studio(&dir);

        // The loose piano is the third row of the bank.
        let at = bank_row(&session, 2);
        session.open_file(at).ok();
        let before = session.selected_file();
        assert!(before.is_some(), "something is open");

        session.set_query("loose");
        assert_eq!(
            session.selected_file(),
            Some(0),
            "still open, and now the only hit"
        );
        session.set_query("");
        assert_eq!(session.selected_file(), before, "and back where it was");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_status_line_says_where_you_are_and_what_a_search_is_searching() {
        let dir = a_collection("session-status");
        let mut session = studio(&dir);
        assert!(
            session
                .library_status()
                .contains(dir.file_name().unwrap().to_str().unwrap()),
            "{}",
            session.library_status()
        );

        session
            .open_file(bank_row(&session, 0))
            .expect("into Drums");
        assert!(
            session.library_status().contains("Drums"),
            "a browser you can walk into has to say where you are: {}",
            session.library_status()
        );

        session.set_query("kit");
        assert!(
            session.library_status().contains("collection"),
            "and a search has to say it is looking everywhere: {}",
            session.library_status()
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_headings_count_is_the_collections_and_does_not_move_as_you_browse() {
        // A heading that counted the rows in front of you would say "2" inside
        // a folder of two and read as the collection having shrunk — and would
        // count folders as soundfonts besides.
        let dir = a_collection("session-count");
        let mut session = studio(&dir);
        assert_eq!(session.library_count(), 5, "every soundfont under the root");
        assert_eq!(
            bank_rows(&session).len(),
            3,
            "and three rows in front of you: two folders and a file"
        );

        session
            .open_file(bank_row(&session, 0))
            .expect("into Drums");
        assert_eq!(
            session.library_count(),
            5,
            "still five: walking into a folder does not shrink the collection"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_bank_with_no_folder_configured_still_has_something_to_say() {
        let dir = scratch("session-empty");
        let session = studio(&dir);
        assert!(
            bank_rows(&session).is_empty(),
            "there are no soundfonts, because there is no folder to have any in"
        );
        // **And there is still something to play.** The built-in instrument's
        // row needs no folder and no files, so a fresh install with nothing
        // configured is not an empty screen — which is the whole of what this
        // test is named for.
        assert_eq!(
            session
                .library_files()
                .first()
                .map(|row| row.name.clone())
                .unwrap_or_default(),
            "Flopsynth"
        );
        assert!(!session.library_status().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}

// ------------------------------------ searching inside every soundfont ---
//
// Asked for from using the browser: *"we can search through soundfonts, and
// then sounds inside the soundfonts, but we are not able to search for sounds
// from within ALL of our soundfonts! that would be amazing if i could just go
// to soundfonts section, type "tuba" or something in the search bar and of
// course no soundfont would show up since i dont have any sf2 file named that
// but within several of my sf2s are sounds called tuba that should be shown
// within the bottom tab, organized by sf2 so the ones from same sf2s are
// grouped together and obvious and if i have a soundfont selected already it
// should show the results within the one selected at the top first."*
//
// # Why it is scanned in the background
//
// Listing a soundfont's presets means reading and parsing the whole file, and
// a collection is hundreds of megabytes. Doing that on the UI thread the first
// time somebody types a letter would freeze the window for seconds, which is
// worse than not having the feature. So the scan runs on a thread of its own
// and the list fills in as it lands — `pump` is where the results arrive,
// which is the same place the graph's are freed.

mod presets {
    use fontelle_app::{RealiseOptions, SampleLibrary, Session};
    use fontelle_engine::timeline_channel;
    use fontelle_types::CompiledTimeline;
    use fontelle_ui::document::{LibraryKind, StudioHost};

    use super::scratch;

    const SR: u32 = 48_000;

    /// Opens soundfont `index` of the **bank**, whatever row the panel is
    /// drawing it at.
    ///
    /// The Sounds tab carries the built-in instrument's row above the bank —
    /// see `through_the_session::rows` — and these tests are about soundfonts,
    /// so they say which soundfont rather than which row.
    fn open_soundfont(session: &mut Session, index: usize) {
        let offset = usize::from(
            session
                .library_files()
                .first()
                .is_some_and(|first| first.name == "Flopsynth"),
        );
        session
            .open_file(index + offset)
            .expect("that soundfont opens");
    }

    /// Two soundfonts, neither of them *named* after anything inside it.
    fn a_library(name: &str) -> std::path::PathBuf {
        use fontelle_assets::fixtures::build_multi_preset_sf2;
        let dir = scratch(name);
        std::fs::write(
            dir.join("Brass Pack.sf2"),
            build_multi_preset_sf2(&[("Tuba", 0, 0, 60), ("Trumpet", 0, 1, 60)]),
        )
        .unwrap();
        std::fs::write(
            dir.join("Orchestra.sf2"),
            build_multi_preset_sf2(&[("Violin", 0, 0, 60), ("Tuba Solo", 0, 1, 60)]),
        )
        .unwrap();
        dir
    }

    fn studio(dir: &std::path::Path) -> Session {
        let project = crate::common::a_project_with_a_clip(8, 120.0, SR);
        let clip = Session::first_clip(&project).expect("a blank project has one clip");
        let channel_nodes = fontelle_app::channel_nodes(&project);
        let (publisher, _timeline) = timeline_channel(CompiledTimeline::empty());
        let options = RealiseOptions {
            sample_rate: SR,
            block_size: fontelle_engine::BLOCK_SIZE,
            quality: fontelle_app::PLAYBACK_QUALITY,
        };
        let mut session = Session::new(
            project,
            SampleLibrary::new(),
            channel_nodes,
            publisher,
            options,
            clip,
            None,
        )
        .with_settings_path(dir.join("settings.json"));
        session.add_soundfont_dir(dir);
        session.open_bank();
        // These are the soundfont browser's tests; the studio opens on the
        // Import tab now, so the tab is switched the way a click would.
        session.set_browser_mode(fontelle_ui::canvas::BrowserMode::Sounds);
        session
    }

    /// Waits for the background scan, pumping the way the window does.
    fn settle(session: &mut Session) {
        for _ in 0..2_000 {
            session.pump();
            if !session.searching_presets() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("the preset scan never finished");
    }

    fn rows(session: &Session) -> Vec<(String, LibraryKind)> {
        session
            .library_presets()
            .into_iter()
            .map(|e| (e.name, e.kind))
            .collect()
    }

    #[test]
    fn a_query_no_file_matches_still_finds_the_sounds_inside_them() {
        let dir = a_library("preset-search");
        let mut session = studio(&dir);

        session.set_query("tuba");
        assert!(
            session.library_files().is_empty(),
            "no soundfont is called tuba, which is the whole point"
        );
        settle(&mut session);

        let found = rows(&session);
        assert!(
            found
                .iter()
                .any(|(name, kind)| name == "Tuba" && *kind == LibraryKind::File),
            "the preset inside Brass Pack was not found: {found:?}"
        );
        assert!(
            found.iter().any(|(name, _)| name == "Tuba Solo"),
            "the one inside Orchestra was not found: {found:?}"
        );
        assert!(
            !found.iter().any(|(name, _)| name == "Violin"),
            "a preset nobody searched for came back: {found:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_hits_are_grouped_under_the_soundfont_they_came_from() {
        // *"organized by sf2 so the ones from same sf2s are grouped together
        // and obvious"* — a flat list of preset names across a collection is
        // unusable, because the same instrument is in twenty of them.
        let dir = a_library("preset-groups");
        let mut session = studio(&dir);
        session.set_query("tuba");
        settle(&mut session);

        let found = rows(&session);
        let headings: Vec<&String> = found
            .iter()
            .filter(|(_, kind)| *kind == LibraryKind::Group)
            .map(|(name, _)| name)
            .collect();
        assert_eq!(
            headings.len(),
            2,
            "one heading per soundfont with a hit in it: {found:?}"
        );
        // Every preset row comes after a heading, and the last heading before
        // it is the file it is in.
        let first = found
            .iter()
            .position(|(name, _)| name == "Tuba")
            .expect("Tuba is in the list");
        assert!(
            found[..first]
                .iter()
                .any(|(_, kind)| *kind == LibraryKind::Group),
            "a preset row with no heading above it: {found:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_open_soundfonts_own_hits_come_first() {
        // *"if i have a soundfont selected already it should show the results
        // within the one selected at the top first."*
        let dir = a_library("preset-selected-first");
        let mut session = studio(&dir);

        // Open the *second* soundfont, so first-in-the-folder cannot be what
        // puts it at the top.
        let orchestra = session
            .library_files()
            .iter()
            .position(|entry| entry.name == "Orchestra")
            .expect("Orchestra is in the folder");
        session.open_file(orchestra).expect("it opens");

        session.set_query("tuba");
        settle(&mut session);

        let found = rows(&session);
        let heading = found
            .iter()
            .find(|(_, kind)| *kind == LibraryKind::Group)
            .map(|(name, _)| name.clone())
            .expect("there is a heading");
        assert_eq!(
            heading, "Orchestra",
            "the open soundfont's hits have to be the ones you see first: {found:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn choosing_a_hit_from_another_soundfont_loads_that_one() {
        // The row has to *work*: a list of results you cannot click is a list
        // of things you now have to go and find by hand.
        let dir = a_library("preset-choose");
        let mut session = studio(&dir);
        session.set_query("tuba solo");
        settle(&mut session);

        let row = rows(&session)
            .iter()
            .position(|(name, kind)| name == "Tuba Solo" && *kind == LibraryKind::File)
            .expect("the hit is in the list");
        session
            .set_channel_instrument(row)
            .expect("choosing a search result must load it");

        assert_eq!(
            session.channels()[0].name,
            "Tuba Solo",
            "the channel is playing what was chosen"
        );
        assert_eq!(
            session
                .open_file_path()
                .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_string())),
            Some("Orchestra".to_string()),
            "and the browser followed it into the soundfont it came from"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_heading_row_is_not_something_you_can_load() {
        let dir = a_library("preset-heading");
        let mut session = studio(&dir);
        session.set_query("tuba");
        settle(&mut session);
        let heading = rows(&session)
            .iter()
            .position(|(_, kind)| *kind == LibraryKind::Group)
            .expect("there is a heading");
        assert!(
            session.set_channel_instrument(heading).is_err(),
            "a heading is a label, not a preset"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn clearing_the_search_goes_back_to_this_soundfonts_presets() {
        let dir = a_library("preset-clear");
        let mut session = studio(&dir);
        open_soundfont(&mut session, 0);
        assert_eq!(
            rows(&session),
            vec![
                ("Tuba".to_string(), LibraryKind::File),
                ("Trumpet".to_string(), LibraryKind::File),
            ]
        );

        session.set_query("tuba");
        settle(&mut session);
        assert!(rows(&session).iter().any(|(_, k)| *k == LibraryKind::Group));

        session.set_query("");
        assert_eq!(
            rows(&session),
            vec![
                ("Tuba".to_string(), LibraryKind::File),
                ("Trumpet".to_string(), LibraryKind::File),
            ],
            "clearing the box puts the open soundfont's own list back"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_status_line_says_the_collection_is_being_read() {
        // A list that fills in over a few seconds with nothing saying why
        // looks broken. It is the same promise the bank's own scan makes.
        let dir = a_library("preset-status");
        let mut session = studio(&dir);
        session.set_query("tuba");
        settle(&mut session);
        let status = session.library_status();
        assert!(
            status.contains("tuba") || status.contains("match"),
            "the status line said {status:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}

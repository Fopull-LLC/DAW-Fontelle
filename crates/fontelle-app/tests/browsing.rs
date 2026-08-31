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
                BankRow::Folder {
                    name, soundfonts, ..
                } if name == want => Some(*soundfonts),
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

    use fontelle_app::{RealiseOptions, SampleLibrary, Session, blank_project};
    use fontelle_engine::timeline_channel;
    use fontelle_types::CompiledTimeline;
    use fontelle_ui::document::{LibraryKind, StudioHost};

    use super::{a_collection, scratch};

    const SR: u32 = 48_000;

    fn studio(dir: &std::path::Path) -> Session {
        let project = blank_project(8, 120.0, SR);
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
        session
    }

    fn rows(session: &Session) -> Vec<(String, LibraryKind)> {
        session
            .library_files()
            .into_iter()
            .map(|e| (e.name, e.kind))
            .collect()
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

        session.open_file(0).expect("Drums is a folder");
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
        assert_eq!(entries[0].name, "Drums");
        assert_eq!(entries[0].detail, "2 sf2");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn typing_searches_the_whole_collection_and_clearing_it_puts_you_back() {
        // The search box decides which question is being asked, and asking the
        // other one must not lose where you were standing.
        let dir = a_collection("session-search");
        let mut session = studio(&dir);
        session.open_file(0).expect("into Drums");

        session.set_query("vio");
        let found = rows(&session);
        assert_eq!(
            found,
            vec![("Violins".to_string(), LibraryKind::File)],
            "a search reaches outside the folder you are in"
        );
        assert_eq!(
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

        // The loose piano is the third row at the top level.
        session.open_file(2).ok();
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

        session.open_file(0).expect("into Drums");
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
            session.library_files().len(),
            3,
            "and three rows in front of you: two folders and a file"
        );

        session.open_file(0).expect("into Drums");
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
        assert!(session.library_files().is_empty());
        assert!(!session.library_status().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}

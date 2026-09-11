//! The on-disk project bundle (TDD §17.1–17.2).

use std::path::PathBuf;

use fontelle_model::{
    Command, PROJECT_FORMAT_VERSION, Project, SetNumber, StorageError, load_project, save_project,
};

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-storage-{name}-{}.fontelle",
        std::process::id()
    ));
    std::fs::remove_dir_all(&path).ok();
    path
}

fn a_project() -> Project {
    let mut project = Project::new("Test Piece");
    let master = project.mixer.master.unwrap();
    SetNumber::new(fontelle_model::NumberTarget::TrackGainDb(master), -3.5)
        .apply(&mut project)
        .unwrap();
    project.loop_range = Some((0, 3840));
    project
}

#[test]
fn a_saved_project_reads_back_as_the_same_document() {
    let bundle = scratch("round-trip");
    let project = a_project();
    save_project(&project, &bundle).expect("save");

    let back = load_project(&bundle).expect("load");
    // Everything but `created`, which the save fills in for a document that
    // has never been written before — see the metadata test below.
    let mut expected = serde_json::to_value(&project).unwrap();
    expected["meta"]["created"] = serde_json::json!(back.meta.created);
    assert_eq!(expected, serde_json::to_value(&back).unwrap());

    // And from the second save on it is exactly idempotent.
    save_project(&back, &bundle).expect("re-save");
    assert_eq!(
        serde_json::to_value(&back).unwrap(),
        serde_json::to_value(load_project(&bundle).unwrap()).unwrap()
    );
    std::fs::remove_dir_all(&bundle).ok();
}

#[test]
fn a_bundle_is_a_folder_with_the_directories_the_layout_names() {
    // TDD §17.1. They exist from the first save rather than being created on
    // demand, so a render or a recording never has to decide whether it is
    // allowed to make a directory.
    let bundle = scratch("layout");
    save_project(&a_project(), &bundle).expect("save");

    assert!(bundle.join("project.json").is_file());
    for dir in ["assets", "recordings", "renders", "backups", "cache"] {
        assert!(bundle.join(dir).is_dir(), "{dir} is missing");
    }
    std::fs::remove_dir_all(&bundle).ok();
}

#[test]
fn saving_over_an_existing_project_leaves_no_temporary_behind() {
    // §17.1's atomic save: write to a temp file, fsync, rename. A crash
    // mid-save can never corrupt a project, and a completed one leaves the
    // directory clean.
    let bundle = scratch("atomic");
    let mut project = a_project();
    save_project(&project, &bundle).expect("first save");
    project.meta.name = "Renamed".into();
    save_project(&project, &bundle).expect("second save");

    let stray: Vec<_> = std::fs::read_dir(&bundle)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains("tmp") || name.ends_with('~'))
        .collect();
    assert!(stray.is_empty(), "left behind {stray:?}");
    assert_eq!(load_project(&bundle).unwrap().meta.name, "Renamed");
    std::fs::remove_dir_all(&bundle).ok();
}

#[test]
fn the_saved_document_stamps_the_format_version() {
    let bundle = scratch("version");
    save_project(&a_project(), &bundle).expect("save");
    let text = std::fs::read_to_string(bundle.join("project.json")).unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        json["meta"]["format_version"],
        serde_json::json!(PROJECT_FORMAT_VERSION)
    );
    std::fs::remove_dir_all(&bundle).ok();
}

#[test]
fn a_project_from_a_newer_build_is_refused_by_version_naming_both() {
    let bundle = scratch("future");
    save_project(&a_project(), &bundle).expect("save");
    let path = bundle.join("project.json");
    let mut json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    json["meta"]["format_version"] = serde_json::json!(PROJECT_FORMAT_VERSION + 9);
    std::fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();

    match load_project(&bundle) {
        Err(StorageError::FromTheFuture { found, newest }) => {
            assert_eq!(found, PROJECT_FORMAT_VERSION + 9);
            assert_eq!(newest, PROJECT_FORMAT_VERSION);
        }
        other => panic!("expected a version refusal, got {other:?}"),
    }
    std::fs::remove_dir_all(&bundle).ok();
}

#[test]
fn a_truncated_project_fails_with_a_message_rather_than_a_panic() {
    // §20.3's standard, applied to our own format.
    let bundle = scratch("truncated");
    save_project(&a_project(), &bundle).expect("save");
    let path = bundle.join("project.json");
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, &text[..text.len() / 2]).unwrap();

    let err = load_project(&bundle).expect_err("half a document is not a document");
    assert!(matches!(err, StorageError::Format(_)));
    let message = err.to_string();
    assert!(
        message.contains("project.json"),
        "the message must name the file: {message}"
    );
    std::fs::remove_dir_all(&bundle).ok();
}

#[test]
fn nonsense_in_place_of_a_document_fails_the_same_way() {
    let bundle = scratch("nonsense");
    save_project(&a_project(), &bundle).expect("save");
    std::fs::write(bundle.join("project.json"), "{\"meta\": 7}").unwrap();
    assert!(matches!(
        load_project(&bundle),
        Err(StorageError::Format(_))
    ));
    std::fs::remove_dir_all(&bundle).ok();
}

#[test]
fn opening_something_that_is_not_a_project_says_so() {
    let bundle = scratch("missing");
    let err = load_project(&bundle).expect_err("nothing is there");
    assert!(matches!(err, StorageError::Io { .. }));
    assert!(err.to_string().contains("project.json"));
}

#[test]
fn the_document_records_when_it_was_created_and_what_wrote_it() {
    let bundle = scratch("meta");
    let mut project = a_project();
    assert!(
        project.meta.created.is_empty(),
        "a fresh project has no date"
    );
    save_project(&project, &bundle).expect("save");

    let back = load_project(&bundle).unwrap();
    // ISO-8601, UTC, to the second: 2026-08-29T12:34:56Z.
    let created = &back.meta.created;
    assert_eq!(created.len(), 20, "unexpected shape: {created}");
    assert!(created.ends_with('Z'));
    assert!(created.starts_with("20"));
    assert!(!back.meta.app_version.is_empty());

    // And a second save does not restamp it: the date is when the piece was
    // started, not when it was last touched.
    project.meta.created = back.meta.created.clone();
    project.meta.name = "Again".into();
    save_project(&project, &bundle).unwrap();
    assert_eq!(load_project(&bundle).unwrap().meta.created, *created);
    std::fs::remove_dir_all(&bundle).ok();
}

#[test]
fn the_document_is_readable_json_a_person_could_repair() {
    // §17.2 chose JSON to be diffable, inspectable, greppable and recoverable
    // by hand, which is only true if it is actually written that way.
    let bundle = scratch("readable");
    save_project(&a_project(), &bundle).expect("save");
    let text = std::fs::read_to_string(bundle.join("project.json")).unwrap();
    assert!(text.contains('\n'), "it must be pretty-printed");
    assert!(text.contains("\"tempo_map\""));
    assert!(text.contains("Test Piece"));
    std::fs::remove_dir_all(&bundle).ok();
}

// ------------------------------------------- the effects in a saved project

/// **Every** effect the menu offers survives a save and an open, with its
/// settings unchanged.
///
/// Nothing covered this before: no test in this file put an insert on a track
/// at all, so `EffectConfig`'s serde was exercised only incidentally. That was
/// survivable while the enum had two variants written the same day as the
/// storage; it is not once effects arrive one at a time, each bringing its own
/// config struct and its own nested enums — a `DistortionCurve` that failed to
/// round-trip would be a project that opens with the wrong distortion, which
/// is worse than one that refuses to open.
///
/// Driven from `EffectKind::ALL`, so an effect added later is covered the day
/// it is added.
#[test]
fn a_project_with_one_of_every_effect_reads_back_the_way_it_was_saved() {
    use fontelle_model::AddInsert;
    use fontelle_types::{EffectConfig, EffectKind};

    let bundle = scratch("every-effect");
    let mut project = a_project();
    let master = project.mixer.master.unwrap();

    for kind in EffectKind::ALL {
        AddInsert::new(master, kind).apply(&mut project).unwrap();
    }
    // Moved off their defaults, so a load that quietly rebuilt them from
    // `EffectConfig::new` would fail here rather than pass by coincidence.
    for slot in project.mixer.tracks[master].inserts.iter_mut() {
        let ids: Vec<&str> = slot.config.specs().iter().map(|spec| spec.id).collect();
        for id in ids {
            let spec = *slot
                .config
                .specs()
                .iter()
                .find(|spec| spec.id == id)
                .unwrap();
            slot.config
                .set(id, spec.min + (spec.max - spec.min) * 0.375);
        }
    }
    let saved: Vec<EffectConfig> = project.mixer.tracks[master]
        .inserts
        .iter()
        .map(|slot| slot.config)
        .collect();

    save_project(&project, &bundle).expect("save");
    let back = load_project(&bundle).expect("load");

    let master = back.mixer.master.unwrap();
    let loaded: Vec<EffectConfig> = back.mixer.tracks[master]
        .inserts
        .iter()
        .map(|slot| slot.config)
        .collect();
    assert_eq!(loaded.len(), EffectKind::ALL.len(), "an insert per effect");
    for (was, now) in saved.iter().zip(loaded.iter()) {
        assert_eq!(was.kind(), now.kind(), "the slot changed effect on the way");
        for spec in was.specs() {
            let before = was.get(spec.id).unwrap();
            let after = now.get(spec.id).unwrap();
            assert!(
                (before - after).abs() <= (spec.max - spec.min) * 1e-5,
                "{:?}'s {} was saved as {before} and read back as {after}",
                was.kind(),
                spec.id
            );
        }
    }
}

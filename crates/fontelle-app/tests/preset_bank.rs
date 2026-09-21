//! The preset bank: factory files in the binary, user files in a folder
//! (`docs/flopsynth-plan.md` §P.4).
//!
//! Modelled on `bank.rs`'s `FileBank` and holding the same two ideas — a walk
//! that lists what is there, and a list of what could not be read rather than
//! a silence where it was. What is different is that this bank has **two
//! origins**, and every rule in here comes from that: a factory preset is
//! read-only and always present, a user preset is a file somebody owns and can
//! delete, and a user preset named like a factory one is a second row rather
//! than a shadow. Hiding one would be a preset that vanished.

use fontelle_app::preset_bank::PresetBank;
use fontelle_types::{
    DeviceKind, EffectConfig, EffectKind, InstrumentKind, PatchData, Preset, PresetOrigin,
    PresetPayload, PresetRef,
};
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-presets-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("the scratch folder must be creatable");
    path
}

fn a_patch(marker: &str) -> PatchData {
    PatchData {
        format_version: 1,
        body: serde_json::json!({ "marker": marker }),
    }
}

fn a_preset(name: &str, category: &str) -> Preset {
    Preset::new(
        DeviceKind::Instrument(InstrumentKind::Flopsynth),
        name,
        category,
        PresetPayload::Patch(a_patch(name)),
    )
}

// ----------------------------------------------------------------- factory

#[test]
fn the_factory_bank_is_in_the_binary_and_is_not_empty() {
    // A fresh install has presets, which is the drum machine's position kept
    // (§P.3): the constructor presets that used to be code are files now, and
    // a file that ships is embedded rather than installed beside the binary.
    let bank = PresetBank::new(None);
    assert!(
        !bank.entries().is_empty(),
        "the factory bank should ship presets"
    );
    assert!(
        bank.entries()
            .iter()
            .all(|entry| entry.origin == PresetOrigin::Factory)
    );
}

#[test]
fn every_factory_preset_loads_and_is_a_preset_for_the_device_it_claims() {
    // The check that makes the export tool trustworthy: a generated file that
    // does not round-trip is a preset that would fail on somebody's machine
    // and nowhere else.
    let bank = PresetBank::new(None);
    for entry in bank.entries() {
        let preset = bank
            .load(entry)
            .unwrap_or_else(|e| panic!("{} did not load: {e}", entry.path.display()));
        assert!(
            preset.is_consistent(),
            "{} is a {:?} holding somebody else's payload",
            entry.path.display(),
            entry.device
        );
        assert!(!preset.is_from_the_future());
        assert_eq!(preset.name, entry.name);
        assert_eq!(preset.category, entry.category);
        assert_eq!(preset.device, entry.device);
    }
    assert!(
        bank.unreadable().is_empty(),
        "a factory file that cannot be read is a build that should not have shipped: {:?}",
        bank.unreadable()
    );
}

#[test]
fn the_factory_bank_covers_more_than_one_device() {
    // The whole claim of §P: this is the DAW's preset system, not one
    // plugin's. If every file were Flopsynth's it would be the same hardcoded
    // bank in a different place.
    let bank = PresetBank::new(None);
    let mut devices: Vec<_> = bank.entries().iter().map(|e| e.device.clone()).collect();
    devices.dedup();
    devices.sort_by_key(|d| d.slug());
    devices.dedup();
    assert!(
        devices.len() > 1,
        "presets for one device only: {devices:?}"
    );
}

// -------------------------------------------------------------- the user's

#[test]
fn a_preset_saved_to_the_user_folder_reads_back_the_same() {
    let dir = scratch("round-trip");
    let mut bank = PresetBank::new(Some(dir.clone()));
    let preset = a_preset("My Pad", "Pad");
    let reference = bank.save(&preset, false).unwrap();
    assert_eq!(reference.origin, PresetOrigin::User);

    let device = DeviceKind::Instrument(InstrumentKind::Flopsynth);
    let entry = bank.find(&device, &reference).expect("saved and listed");
    assert_eq!(bank.load(entry).unwrap(), preset);
}

#[test]
fn a_saved_preset_is_a_file_in_a_folder_named_after_its_category() {
    // The layout is the point (§P.3): a category *is* a folder, which is what
    // makes a user's categories free-form and "Save as…" able to make one.
    let dir = scratch("layout");
    let mut bank = PresetBank::new(Some(dir.clone()));
    bank.save(&a_preset("My Pad", "Warm"), false).unwrap();
    assert!(
        dir.join("flopsynth")
            .join("Warm")
            .join("My Pad.json")
            .is_file(),
        "expected <dir>/flopsynth/Warm/My Pad.json, found {:?}",
        std::fs::read_dir(&dir).map(|d| d.count())
    );
}

#[test]
fn saving_over_a_preset_is_refused_unless_it_is_asked_for() {
    // "Save as…" must not silently replace somebody's work; "Save" must be
    // able to. One flag, and the two buttons are the two values of it.
    let dir = scratch("overwrite");
    let mut bank = PresetBank::new(Some(dir));
    bank.save(&a_preset("My Pad", "Pad"), false).unwrap();
    assert!(bank.save(&a_preset("My Pad", "Pad"), false).is_err());
    bank.save(&a_preset("My Pad", "Pad"), true).unwrap();
    assert_eq!(
        bank.for_device(&DeviceKind::Instrument(InstrumentKind::Flopsynth))
            .iter()
            .filter(|e| e.origin == PresetOrigin::User)
            .count(),
        1,
        "an overwrite is one preset, not two"
    );
}

#[test]
fn a_name_that_is_a_path_is_refused() {
    // A name reaches the filesystem, so a name that is a path is a preset
    // that writes somewhere nobody asked for. Refused at the bank, which is
    // the only place that knows what a name is about to become.
    let dir = scratch("separator");
    let mut bank = PresetBank::new(Some(dir));
    for name in ["", "   ", "../escape", "a/b", "a\\b", "."] {
        assert!(
            bank.save(&a_preset(name, "Pad"), false).is_err(),
            "{name:?} should not be a preset name"
        );
    }
    for category in ["", "../escape", "a/b"] {
        assert!(
            bank.save(&a_preset("Fine", category), false).is_err(),
            "{category:?} should not be a category"
        );
    }
}

#[test]
fn deleting_a_user_preset_takes_its_file_and_its_row() {
    let dir = scratch("delete");
    let mut bank = PresetBank::new(Some(dir.clone()));
    let reference = bank.save(&a_preset("Doomed", "Pad"), false).unwrap();
    let device = DeviceKind::Instrument(InstrumentKind::Flopsynth);
    let entry = bank.find(&device, &reference).unwrap().clone();
    bank.delete(&entry).unwrap();
    assert!(!entry.path.is_file());
    assert!(bank.find(&device, &reference).is_none());
}

#[test]
fn a_factory_preset_cannot_be_deleted_or_written_over() {
    // Read-only is the whole of the difference between the two origins, and
    // the bank is where it is enforced rather than the button that draws
    // itself disabled — a disabled button is a courtesy, not a rule.
    let dir = scratch("read-only");
    let mut bank = PresetBank::new(Some(dir));
    let factory = bank
        .entries()
        .iter()
        .find(|e| e.origin == PresetOrigin::Factory)
        .expect("the factory bank ships presets")
        .clone();
    assert!(bank.delete(&factory).is_err());
}

#[test]
fn a_file_that_is_not_a_preset_is_listed_by_path_rather_than_skipped() {
    // `FileBank`'s rule, and it matters more here: a preset that silently
    // disappears is a person's work gone with no way to ask why.
    let dir = scratch("unreadable");
    let folder = dir.join("flopsynth").join("Pad");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("Broken.json"), "{ this is not json").unwrap();
    let bank = PresetBank::new(Some(dir));
    assert_eq!(bank.unreadable().len(), 1, "{:?}", bank.unreadable());
    assert_eq!(bank.unreadable()[0].0.file_name().unwrap(), "Broken.json");
}

#[test]
fn a_preset_whose_payload_does_not_match_its_device_is_unreadable_rather_than_loaded() {
    let dir = scratch("inconsistent");
    let folder = dir.join("flopsynth").join("Pad");
    std::fs::create_dir_all(&folder).unwrap();
    let liar = Preset::new(
        DeviceKind::Instrument(InstrumentKind::Flopsynth),
        "Liar",
        "Pad",
        PresetPayload::Effect(EffectConfig::new(EffectKind::Reverb)),
    );
    std::fs::write(
        folder.join("Liar.json"),
        serde_json::to_string_pretty(&liar).unwrap(),
    )
    .unwrap();
    let bank = PresetBank::new(Some(dir));
    assert_eq!(bank.unreadable().len(), 1, "{:?}", bank.unreadable());
    assert!(
        bank.for_device(&DeviceKind::Instrument(InstrumentKind::Flopsynth))
            .iter()
            .all(|e| e.name != "Liar")
    );
}

// ------------------------------------------------------------ both origins

#[test]
fn a_device_lists_factory_first_then_the_users_own() {
    // Factory first because it is what a person is browsing *from*; their own
    // at the end because that is where the one they just made will be.
    let dir = scratch("order");
    let mut bank = PresetBank::new(Some(dir));
    bank.save(&a_preset("Aaa Mine", "Pad"), false).unwrap();
    let rows = bank.for_device(&DeviceKind::Instrument(InstrumentKind::Flopsynth));
    let first_user = rows
        .iter()
        .position(|e| e.origin == PresetOrigin::User)
        .expect("the saved one is there");
    assert!(
        rows[..first_user]
            .iter()
            .all(|e| e.origin == PresetOrigin::Factory),
        "a user preset appeared among the factory ones"
    );
    assert!(
        rows[first_user..]
            .iter()
            .all(|e| e.origin == PresetOrigin::User),
        "a factory preset appeared after a user one"
    );
}

#[test]
fn within_an_origin_the_order_is_category_then_name() {
    let dir = scratch("sorted");
    let mut bank = PresetBank::new(Some(dir));
    for (name, category) in [("Zebra", "Aaa"), ("Apple", "Zzz"), ("Apple", "Aaa")] {
        bank.save(&a_preset(name, category), false).unwrap();
    }
    let rows: Vec<_> = bank
        .for_device(&DeviceKind::Instrument(InstrumentKind::Flopsynth))
        .into_iter()
        .filter(|e| e.origin == PresetOrigin::User)
        .map(|e| (e.category.clone(), e.name.clone()))
        .collect();
    assert_eq!(
        rows,
        vec![
            ("Aaa".to_string(), "Apple".to_string()),
            ("Aaa".to_string(), "Zebra".to_string()),
            ("Zzz".to_string(), "Apple".to_string()),
        ]
    );
}

#[test]
fn a_user_preset_named_like_a_factory_one_is_a_second_row() {
    // §P.3, stated as a test because the alternative is tempting and wrong:
    // shadowing would make the factory preset unreachable, which is a preset
    // that vanished when somebody happened to reuse its name.
    let dir = scratch("shadow");
    let mut bank = PresetBank::new(Some(dir));
    let factory = bank
        .entries()
        .iter()
        .find(|e| e.origin == PresetOrigin::Factory)
        .expect("the factory bank ships presets")
        .clone();
    let mine = Preset::new(
        factory.device.clone(),
        factory.name.clone(),
        factory.category.clone(),
        bank.load(&factory).unwrap().payload,
    );
    bank.save(&mine, false).unwrap();
    let same: Vec<_> = bank
        .for_device(&factory.device)
        .into_iter()
        .filter(|e| e.name == factory.name && e.category == factory.category)
        .collect();
    assert_eq!(same.len(), 2);
    assert_ne!(same[0].origin, same[1].origin);
}

#[test]
fn find_tells_the_two_origins_apart() {
    let dir = scratch("find");
    let mut bank = PresetBank::new(Some(dir));
    let device = DeviceKind::Instrument(InstrumentKind::Flopsynth);
    bank.save(&a_preset("Twin", "Pad"), false).unwrap();
    assert!(
        bank.find(&device, &PresetRef::new("Twin", "Pad", PresetOrigin::User))
            .is_some()
    );
    assert!(
        bank.find(
            &device,
            &PresetRef::new("Twin", "Pad", PresetOrigin::Factory)
        )
        .is_none(),
        "a ref that names the factory bank must not find the user's file"
    );
}

#[test]
fn search_runs_across_every_device() {
    // The browser's Presets tab searches the whole bank, not the open
    // device's corner of it — you go looking for "hall" without first
    // deciding it is a reverb.
    let dir = scratch("search");
    let mut bank = PresetBank::new(Some(dir));
    bank.save(&a_preset("Quokka", "Pad"), false).unwrap();
    let hits = bank.search("quokka");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].name, "Quokka");
    assert!(
        bank.search("").len() >= bank.entries().len(),
        "an empty search shows the whole bank"
    );
}

#[test]
fn the_categories_of_a_device_are_the_folders_it_has() {
    let dir = scratch("categories");
    let mut bank = PresetBank::new(Some(dir));
    let device = DeviceKind::Instrument(InstrumentKind::Flopsynth);
    bank.save(&a_preset("One", "Zebra"), false).unwrap();
    bank.save(&a_preset("Two", "Zebra"), false).unwrap();
    let categories = bank.categories(&device);
    assert_eq!(
        categories.iter().filter(|c| *c == "Zebra").count(),
        1,
        "a category is listed once however many presets are in it"
    );
    assert!(categories.windows(2).all(|pair| pair[0] <= pair[1]));
}

#[test]
fn a_bank_with_no_user_folder_still_lists_the_factory_and_refuses_to_save() {
    // The state a fresh install with no writable data directory is in. It
    // must still *browse*, because everything that ships is still there.
    let mut bank = PresetBank::new(None);
    assert!(!bank.entries().is_empty());
    assert!(bank.save(&a_preset("Nowhere", "Pad"), false).is_err());
}

/// Whether an effect ships presets is a decision taken for every one.
///
/// This used to ask `EffectConfig::presets()`, which is gone (§P.9). It asks
/// the **files** now, which is the same question about the same thing one
/// layer out — and the reason it is worth asking at all is unchanged: an
/// effect that shipped none because nobody decided looks exactly like one that
/// ships none on purpose.
///
/// The decision, as of 2026-09-11, is **every one ships a bank**. The earlier
/// lists — three "none on purpose" (the utility's ten separate jobs, the
/// gate's and the filter's one obvious knob) and five "owed" — were overruled
/// by the person using it: *"please also ensure that every built in effect
/// plugin has a bunch of presets that will be generally useful in a wide
/// variety of situations especially the compressor which im noticing has no
/// presets right now."* A preset for the utility turned out to be exactly a
/// preset for one of its ten jobs (*mono*, *swap sides*), which is a better
/// answer than none. The recipes are `fontelle-types/src/effect_presets.rs`;
/// their floor per effect is `effect_editor.rs`'s.
#[test]
fn whether_an_effect_ships_presets_is_a_decision_taken_for_every_one() {
    use fontelle_types::{DeviceKind, EffectKind};

    let bank = PresetBank::new(None);
    for kind in EffectKind::ALL {
        let ships = !bank.for_device(&DeviceKind::Effect(kind)).is_empty();
        assert!(
            ships,
            "{kind:?} ships no presets, and every effect is meant to"
        );
    }
}

/// Every factory file is somewhere a person can reach it.
///
/// A preset in a folder no device asks for is a preset nobody will ever see —
/// the export tool's slug and the bank's have to be the same string, and they
/// are only the same string because both come from `DeviceKind::slug`.
#[test]
fn every_factory_preset_belongs_to_a_device_the_program_still_has() {
    let bank = PresetBank::new(None);
    for entry in bank.entries() {
        let listed = bank.devices().contains(&entry.device);
        assert!(listed, "{} is for a device nothing lists", entry.name);
        assert!(
            entry
                .path
                .to_string_lossy()
                .starts_with(&entry.device.slug()),
            "{} is filed under {:?} rather than {}",
            entry.name,
            entry.path,
            entry.device.slug()
        );
    }
}

/// The bank reads a preset's words (§5.1) with its index, so the browser
/// can search the tags and show the phrase without opening the file again
/// — and every factory synth row has them, from the rewrite Ty's §9.7
/// asked for.
#[test]
fn every_factory_synth_preset_carries_its_tags_and_its_phrase() {
    use fontelle_types::{DeviceKind, InstrumentKind};
    let bank = PresetBank::new(None);
    let rows = bank.for_device(&DeviceKind::Instrument(InstrumentKind::Flopsynth));
    assert!(rows.len() >= 380);
    for entry in &rows {
        assert!(entry.tags.len() >= 3, "{}: {:?}", entry.name, entry.tags);
        assert!(!entry.notes.is_empty(), "{}", entry.name);
        for tag in &entry.tags {
            assert!(
                fontelle_core::flopsynth::TAGS.contains(&tag.as_str()),
                "{}: {tag}",
                entry.name
            );
        }
    }
    let grand = rows
        .iter()
        .find(|e| e.name == "Grand Piano")
        .expect("the grand");
    assert!(grand.tags.iter().any(|t| t == "sample"));
    assert!(grand.notes.contains("reach for"), "{}", grand.notes);
}

// -------------------------------------------------------------------- packs

/// A **pack** (`docs/flopsynth-next.md` §5): the user's presets as one file
/// to hand to somebody — a JSON document holding the presets whole, so it
/// reads back with the same code a preset file does and needs no archive
/// library to open. Importing one saves each preset into the folder as if
/// it had been saved here, and **skips** a name already taken rather than
/// writing over it: a pack from a friend must not replace your own.
#[test]
fn a_pack_carries_the_users_presets_and_reads_back_into_another_bank() {
    let dir = scratch("pack-out");
    let mut bank = PresetBank::new(Some(dir.clone()));
    bank.save(&a_preset("My Pad", "Pad"), false).unwrap();
    bank.save(&a_preset("My Bass", "Bass"), false).unwrap();
    let device = DeviceKind::Instrument(InstrumentKind::Flopsynth);
    let mine: Vec<_> = bank
        .for_device(&device)
        .into_iter()
        .filter(|e| e.origin == PresetOrigin::User)
        .cloned()
        .collect();
    assert_eq!(mine.len(), 2);

    let pack = dir.join("mine.fontelle-pack.json");
    let written = bank.export_pack(&mine, &pack).unwrap();
    assert_eq!(written, 2);
    assert!(pack.is_file());
    // Legible: a JSON document that says what it is.
    let text = std::fs::read_to_string(&pack).unwrap();
    assert!(text.contains("\"fontelle-pack\""), "{text}");
    assert!(text.contains("My Pad") && text.contains("My Bass"));

    // Into a fresh bank: both land as the user's own, in their categories.
    let other = scratch("pack-in");
    let mut fresh = PresetBank::new(Some(other.clone()));
    let (imported, skipped) = fresh.import_pack(&pack).unwrap();
    assert_eq!((imported, skipped), (2, 0));
    let pad = fresh
        .find(
            &device,
            &PresetRef::new("My Pad", "Pad", PresetOrigin::User),
        )
        .expect("the pad landed");
    assert_eq!(fresh.load(pad).unwrap(), a_preset("My Pad", "Pad"));
    assert!(
        other
            .join("flopsynth")
            .join("Bass")
            .join("My Bass.json")
            .is_file()
    );

    // Again: nothing written over, both skipped and said so.
    let (imported, skipped) = fresh.import_pack(&pack).unwrap();
    assert_eq!((imported, skipped), (0, 2));

    // A file that is not a pack is refused with a reason.
    let not = dir.join("not-a-pack.json");
    std::fs::write(&not, "{\"marker\": 1}").unwrap();
    assert!(fresh.import_pack(&not).is_err());

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&other);
}

#[test]
fn a_pack_of_factory_rows_is_refused_since_every_build_has_them() {
    let dir = scratch("pack-factory");
    let bank = PresetBank::new(Some(dir.clone()));
    let device = DeviceKind::Instrument(InstrumentKind::Flopsynth);
    let factory: Vec<_> = bank
        .for_device(&device)
        .into_iter()
        .take(3)
        .cloned()
        .collect();
    let pack = dir.join("factory.fontelle-pack.json");
    let result = bank.export_pack(&factory, &pack);
    assert!(result.is_err(), "a pack is for what you made");
    assert!(!pack.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

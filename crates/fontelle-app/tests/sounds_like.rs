//! *Sounds like* on the Presets page (`docs/flopsynth-next.md` §5.2): the
//! bank's previews live in a file beside the settings, the first Presets
//! page starts a worker on what the file lacks, and the About column
//! lists the nearest by sound once they are in. Seeded here rather than
//! rendered — the bank is forty seconds of work in a release build and
//! minutes in this one — so what is held is the wiring: the file is read,
//! nothing already indexed is rendered again, and the loaded preset's
//! neighbours reach the view.

mod common;

use fontelle_app::preview_index::PreviewIndex;
use fontelle_core::preview::SoundVector;
use fontelle_types::{DeviceKind, InstrumentKind, PresetPayload};
use fontelle_ui::canvas::{FlopsynthPage, PresetDevice};
use fontelle_ui::document::StudioHost;

use common::SR;

fn vector(t30: f32, centroid: f32) -> SoundVector {
    SoundVector {
        scalar: [t30, centroid, 1.0, 1.0],
        shape: [0.1; 10],
    }
}

#[test]
fn the_about_column_lists_what_the_loaded_preset_sounds_like() {
    let mut session = common::a_session_for(common::a_project_with_a_clip(8, 120.0, SR));
    session.set_channel_kind(0, InstrumentKind::Osc3);
    session.set_channel_kind(0, InstrumentKind::Flopsynth);
    // An index for every factory row, keyed the way the session keys them,
    // with the Felt Piano next to the Grand and the growls far away.
    let bank = fontelle_app::preset_bank::PresetBank::new(None);
    let mut index = PreviewIndex::default();
    let device = DeviceKind::Instrument(InstrumentKind::Flopsynth);
    let rows = bank.for_device(&device);
    for (n, entry) in rows.iter().enumerate() {
        let preset = bank.load(entry).unwrap();
        let PresetPayload::Patch(data) = &preset.payload else {
            panic!("a synth preset holds a patch")
        };
        let hash = PreviewIndex::hash_of(&serde_json::to_string(data).unwrap());
        let vector = match entry.name.as_str() {
            "Grand Piano" => vector(0.0, 5.0),
            "Felt Piano" => vector(0.05, 5.05),
            "Electric Grand" => vector(0.1, 5.2),
            _ => vector(3.0 + n as f32 * 0.01, 8.0),
        };
        index.insert(&entry.name, hash, vector);
    }
    let path = PreviewIndex::path_in(session.settings_path().unwrap().parent().unwrap());
    index.save(&path).unwrap();
    // Load the Grand Piano and look at the Presets page.
    let grand = session
        .preset_choices(PresetDevice::Instrument)
        .iter()
        .position(|p| p.name == "Grand Piano")
        .expect("the grand is in the bank");
    session.apply_preset(PresetDevice::Instrument, grand);
    let view = session.flopsynth(FlopsynthPage::Presets).expect("the page");
    assert_eq!(&view.sounds_like[..2], ["Felt Piano", "Electric Grand"]);
    assert_eq!(view.sounds_like.len(), 5);
    // Everything was indexed, so no worker was started.
    assert!(!session.previews_rendering(), "nothing left to render");
    assert_eq!(session.preview_index().entries.len(), rows.len());
    // The Synth page carries none: the column is the Presets page's.
    let view = session.flopsynth(FlopsynthPage::Synth).expect("the page");
    assert!(view.sounds_like.is_empty());
}

#[test]
fn a_bank_with_no_index_starts_the_worker_and_answers_nothing_yet() {
    let mut session = common::a_session_for(common::a_project_with_a_clip(8, 120.0, SR));
    session.set_channel_kind(0, InstrumentKind::Osc3);
    session.set_channel_kind(0, InstrumentKind::Flopsynth);
    let grand = session
        .preset_choices(PresetDevice::Instrument)
        .iter()
        .position(|p| p.name == "Grand Piano")
        .unwrap();
    session.apply_preset(PresetDevice::Instrument, grand);
    let view = session.flopsynth(FlopsynthPage::Presets).expect("the page");
    assert!(
        view.sounds_like.len() < 5,
        "not rendered yet: {:?}",
        view.sounds_like
    );
    assert!(session.previews_rendering(), "the worker is on it");
}

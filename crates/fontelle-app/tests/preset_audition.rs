//! Hearing a bank row without loading it (`docs/flopsynth-next.md` §5.2).
//!
//! The Presets page used to load on a single click, which is the wrong
//! answer to "what does this one sound like" for the reason the soundfont
//! browser learned first: the only way to hear a sound was to put it on
//! the channel, over whatever was there. A row now **auditions** through
//! the preview voice — the sampler that is in no channel map — and loads
//! only on a double-click or Enter, so a loaded preset's edits are never
//! lost to a stray click.
//!
//! What is held here is the seam: auditioning aims the live path at the
//! preview voice and writes nothing; ending the preview aims it back; a
//! row that is not there says so; and a row has *sounds like* of its own,
//! so the inspector can describe the selected row rather than the loaded.

mod common;

use fontelle_app::preview_index::PreviewIndex;
use fontelle_core::preview::SoundVector;
use fontelle_types::{DeviceKind, InstrumentKind, PresetPayload};
use fontelle_ui::canvas::PresetDevice;
use fontelle_ui::document::{DocumentHost, StudioHost};

use common::SR;

fn a_session() -> fontelle_app::Session {
    let mut session = common::a_session_for(common::a_project_with_a_clip(8, 120.0, SR));
    session.set_channel_kind(0, InstrumentKind::Osc3);
    session.set_channel_kind(0, InstrumentKind::Flopsynth);
    session
}

fn row_of(session: &fontelle_app::Session, name: &str) -> usize {
    session
        .preset_choices(PresetDevice::Instrument)
        .iter()
        .position(|p| p.name == name)
        .unwrap_or_else(|| panic!("{name} is in the bank"))
}

#[test]
fn auditioning_a_row_aims_the_live_path_at_the_preview_voice_and_writes_nothing() {
    let mut session = a_session();
    let channel_target = session.audition_target();
    let before = serde_json::to_string(session.project()).unwrap();
    let patch_before = session.selected_patch().expect("a patch");
    let dirty_before = session.is_dirty();
    let choir = row_of(&session, "Choir Ahh");

    StudioHost::audition_preset(&mut session, PresetDevice::Instrument, choir)
        .expect("the row auditions");

    assert_ne!(
        session.audition_target(),
        channel_target,
        "the next note goes to the preview voice, not the channel"
    );
    // **A listen is not a load.** The channel keeps its patch and its name,
    // the document is untouched, and there is nothing to undo.
    assert_eq!(serde_json::to_string(session.project()).unwrap(), before);
    assert_eq!(session.selected_patch().expect("a patch"), patch_before);
    assert_eq!(
        session.is_dirty(),
        dirty_before,
        "hearing a preset dirties nothing"
    );

    // Ending the preview aims the live path back at the channel.
    session.end_preview();
    assert_eq!(session.audition_target(), channel_target);
}

#[test]
fn a_row_that_is_not_there_is_refused_rather_than_aimed_at() {
    let mut session = a_session();
    let target = session.audition_target();
    let result = StudioHost::audition_preset(&mut session, PresetDevice::Instrument, 100_000);
    assert!(result.is_err(), "no such row");
    assert_eq!(
        session.audition_target(),
        target,
        "a refused audition leaves the live path where it was"
    );
}

#[test]
fn a_row_has_sounds_like_of_its_own() {
    let session = a_session();
    // An index where the Felt Piano and the Electric Grand sit beside the
    // Grand and everything else is far off — `sounds_like.rs`'s seeding.
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
        let (t30, centroid) = match entry.name.as_str() {
            "Grand Piano" => (0.0, 5.0),
            "Felt Piano" => (0.05, 5.05),
            "Electric Grand" => (0.1, 5.2),
            _ => (3.0 + n as f32 * 0.01, 8.0),
        };
        index.insert(
            &entry.name,
            hash,
            SoundVector {
                scalar: [t30, centroid, 1.0, 1.0],
                shape: [0.1; 10],
            },
        );
    }
    let path = PreviewIndex::path_in(session.settings_path().unwrap().parent().unwrap());
    index.save(&path).unwrap();
    // The page has to have been looked at once for the index to be read.
    let _ = session.flopsynth(fontelle_ui::canvas::FlopsynthPage::Presets);

    // The loaded preset is whatever the channel started as; the *row* asked
    // about is the Grand, and its neighbours are the pianos.
    let grand = row_of(&session, "Grand Piano");
    let like = session.preset_sounds_like(PresetDevice::Instrument, grand);
    assert_eq!(&like[..2], ["Felt Piano", "Electric Grand"]);
    assert!(!like.iter().any(|name| name == "Grand Piano"), "not itself");
    // A row that is not there sounds like nothing.
    assert!(
        session
            .preset_sounds_like(PresetDevice::Instrument, 100_000)
            .is_empty()
    );
}

/// The keyboard follows the listen and then comes **back**: while a row is
/// being auditioned a plugged-in keyboard plays it too — try it on the
/// keys before loading — and ending the preview aims the keyboard at the
/// channel again. It used to stay on the preview voice until the next
/// graph rebuild, playing whatever was last listened to.
#[test]
fn the_keyboard_follows_the_audition_and_comes_back_to_the_channel() {
    let session = a_session();
    let target = std::sync::Arc::new(fontelle_midi::LiveTarget::new(
        fontelle_types::NodeId::default(),
    ));
    let mut session = session.with_live_target(std::sync::Arc::clone(&target));
    let channel_node = session.audition_target();
    assert_eq!(
        target.get(),
        channel_node,
        "the keyboard starts on the channel"
    );

    let choir = row_of(&session, "Choir Ahh");
    StudioHost::audition_preset(&mut session, PresetDevice::Instrument, choir)
        .expect("the row auditions");
    assert_eq!(
        target.get(),
        session.audition_target(),
        "while listening, the keyboard plays the listen"
    );
    assert_ne!(target.get(), channel_node);

    session.end_preview();
    assert_eq!(
        target.get(),
        channel_node,
        "and comes back to the channel when the listen ends"
    );
}

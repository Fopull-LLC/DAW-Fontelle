//! The synth window's header actions (`docs/flopsynth-next.md` §3.2, §5):
//! **A/B**, **Init**, **Randomise** and **Mutate**.
//!
//! An A/B pair is two patches the window holds for one channel, of which
//! one is on the channel: switching stashes what is playing in its slot
//! and puts the other slot's patch on — a copy of the same patch the
//! first time, since nothing was in B yet — so an edit can be heard
//! against what it was. It is **window state**, not document state: the
//! file does not carry a B, and a switch goes through the document as the
//! one patch change it is, undoable like a load.
//!
//! Randomise is §5's: ±20 % on continuous values, **never a source
//! change** — the same oscillators, the same tables, the same recording,
//! every chooser and switch where it was — so what comes out is a
//! variation in the preset's family rather than a new preset. Mutate is
//! the same at a twentieth. The Voice card is left alone: the output
//! level and the voice count are the instrument's plumbing, not its
//! sound, and a randomise that could make things loud is a randomise
//! nobody presses twice. So is the tuning: a layer a fifth up is a
//! different instrument, not a variation of this one.

mod common;

use fontelle_types::InstrumentKind;
use fontelle_ui::canvas::{ParamKind, PresetDevice};
use fontelle_ui::document::{DocumentHost, StudioHost};

use common::SR;

fn a_flopsynth() -> fontelle_app::Session {
    let mut session = common::a_session_for(common::a_project_with_a_clip(8, 120.0, SR));
    session.set_channel_kind(0, InstrumentKind::Osc3);
    session.set_channel_kind(0, InstrumentKind::Flopsynth);
    session
}

/// Every control of the window, by address: `(value, kind)`.
fn controls(session: &fontelle_app::Session) -> Vec<(String, f32, ParamKind)> {
    let view = session.instrument().expect("an instrument");
    view.groups
        .iter()
        .flat_map(|g| g.params.iter())
        .map(|p| (p.address.as_str().to_string(), p.value, p.kind.clone()))
        .collect()
}

fn set(session: &mut fontelle_app::Session, address: &str, value: f32) {
    session.set_instrument_param(&fontelle_types::ParamAddress::new(address), value);
    session.end_gesture();
}

fn cutoff(session: &fontelle_app::Session) -> f32 {
    controls(session)
        .iter()
        .find(|(a, _, _)| a == "patch/filter[0]/cutoff")
        .map(|(_, v, _)| *v)
        .expect("a cutoff")
}

#[test]
fn switching_to_b_keeps_a_and_comes_back_to_it() {
    let mut session = a_flopsynth();
    assert_eq!(session.ab_slot(), 0, "a channel starts on A");
    let a_cutoff = cutoff(&session);

    // To B: the same patch, since B held nothing yet.
    session.ab_switch();
    assert_eq!(session.ab_slot(), 1);
    assert_eq!(cutoff(&session), a_cutoff, "B starts as a copy of A");

    // An edit on B.
    set(&mut session, "patch/filter[0]/cutoff", 0.2);
    assert!((cutoff(&session) - 0.2).abs() < 1e-3);

    // Back to A: the edit is B's, not A's.
    session.ab_switch();
    assert_eq!(session.ab_slot(), 0);
    assert_eq!(cutoff(&session), a_cutoff, "A is as it was");

    // And to B again: the edit is still there.
    session.ab_switch();
    assert!((cutoff(&session) - 0.2).abs() < 1e-3, "B kept its edit");

    // A switch is one document change, and undoes as one.
    session.undo();
    assert_eq!(session.ab_slot(), 0, "undoing the switch puts A back");
    assert_eq!(cutoff(&session), a_cutoff);
}

#[test]
fn copying_puts_this_slot_into_the_other() {
    let mut session = a_flopsynth();
    session.ab_switch();
    set(&mut session, "patch/filter[0]/cutoff", 0.3);
    session.ab_switch();
    let a_cutoff = cutoff(&session);
    assert!((a_cutoff - 0.3).abs() > 0.01, "A and B differ first");
    // A copied over B: switching to B now hears A.
    session.ab_copy();
    assert_eq!(session.ab_slot(), 0, "a copy does not switch");
    session.ab_switch();
    assert_eq!(cutoff(&session), a_cutoff, "B is now A's copy");
}

#[test]
fn the_slots_are_per_channel_and_not_in_the_file() {
    let mut session = a_flopsynth();
    session
        .add_channel_of(InstrumentKind::Flopsynth)
        .expect("a second channel");
    session.select_channel(0);
    session.ab_switch();
    assert_eq!(session.ab_slot(), 1);
    // Another channel is on its own A.
    session.select_channel(1);
    assert_eq!(session.ab_slot(), 0);
    session.select_channel(0);
    assert_eq!(session.ab_slot(), 1, "and the first is still on its B");
    // A preset load is a new patch on this channel: the pair starts over,
    // or "B" would be a patch from before the load with nothing to do
    // with what is playing.
    let grand = session
        .preset_choices(PresetDevice::Instrument)
        .iter()
        .position(|p| p.name == "Grand Piano")
        .expect("the grand");
    session.apply_preset(PresetDevice::Instrument, grand);
    assert_eq!(session.ab_slot(), 0);
}

#[test]
fn init_puts_the_init_patch_on_and_is_one_undo() {
    let mut session = a_flopsynth();
    let grand = session
        .preset_choices(PresetDevice::Instrument)
        .iter()
        .position(|p| p.name == "Grand Piano")
        .expect("the grand");
    session.apply_preset(PresetDevice::Instrument, grand);
    let before = session.selected_patch().expect("a patch");
    assert_ne!(before, fontelle_core::flopsynth::flopsynth_init());

    session.init_patch();
    let now = session.selected_patch().expect("a patch");
    assert_eq!(now, fontelle_core::flopsynth::flopsynth_init());
    assert_eq!(
        session.preset_bar(PresetDevice::Instrument).name,
        None,
        "the Init patch came from no preset"
    );
    assert_eq!(
        session.channels()[0].name,
        "Init",
        "the rack row says so too"
    );
    session.undo();
    assert_eq!(session.selected_patch().expect("a patch"), before);
}

#[test]
fn randomise_moves_every_knob_a_little_and_nothing_else() {
    let mut session = a_flopsynth();
    let grand = session
        .preset_choices(PresetDevice::Instrument)
        .iter()
        .position(|p| p.name == "Grand Piano")
        .expect("the grand");
    session.apply_preset(PresetDevice::Instrument, grand);
    let before = controls(&session);
    let patch_before = session.selected_patch().expect("a patch");

    session.randomise_patch(0.2);

    let after = controls(&session);
    assert_eq!(
        before.len(),
        after.len(),
        "the same controls: no source changed"
    );
    let mut moved = 0;
    for ((address, was, kind), (_, now, _)) in before.iter().zip(&after) {
        let delta = (now - was).abs();
        match kind {
            ParamKind::Knob if address.starts_with("patch/voice/") => {
                assert_eq!(was, now, "{address}: the Voice card is left alone");
            }
            ParamKind::Knob
                if address.ends_with("/semitones")
                    || address.ends_with("/tune")
                    || address.ends_with("/octave") =>
            {
                assert_eq!(was, now, "{address}: the tuning is left alone");
            }
            ParamKind::Knob => {
                assert!(
                    delta <= 0.2 + 1e-3,
                    "{address} moved {delta}, more than a fifth"
                );
                if delta > 1e-4 {
                    moved += 1;
                }
            }
            _ => assert_eq!(was, now, "{address}: a chooser or a switch moved"),
        }
    }
    assert!(moved >= 10, "only {moved} knobs moved");
    // The sources are what they were.
    let patch = session.selected_patch().expect("a patch");
    for (a, b) in patch_before.layers.iter().zip(&patch.layers) {
        assert_eq!(
            std::mem::discriminant(&a.source),
            std::mem::discriminant(&b.source)
        );
    }
    // One undo takes it all back.
    session.undo();
    assert_eq!(controls(&session), before);
}

#[test]
fn mutate_is_a_randomise_at_a_twentieth() {
    let mut session = a_flopsynth();
    let before = controls(&session);
    session.randomise_patch(0.05);
    let after = controls(&session);
    let widest = before
        .iter()
        .zip(&after)
        .map(|((_, was, _), (_, now, _))| (now - was).abs())
        .fold(0.0f32, f32::max);
    assert!(widest <= 0.05 + 1e-3, "the widest move was {widest}");
    assert!(widest > 0.0, "something moved");
}

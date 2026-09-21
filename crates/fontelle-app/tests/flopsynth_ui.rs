//! Flopsynth's panel (`docs/flopsynth-plan.md` §8, §9.4).
//!
//! # What this holds, and what it deliberately does not
//!
//! §8 asks for a bespoke canvas: cards laid out as the signal flows, a wave
//! picture per oscillator, a filter response you can drag on, envelope nodes,
//! a modulation ring on every knob. That is the *drawing*, and it is Phase 4's.
//!
//! What is here is the **plumbing that drawing will sit on**, and it is what
//! makes the synth usable today: one group per card the bespoke window will
//! draw, in the order it will draw them, every control reachable, every one of
//! them automatable, and every one of them heard while it turns.
//!
//! The claim that matters most is gate 2 of §1 — *every control on the window
//! is a stable address that automation, MIDI learn and undo reach* — and it is
//! held here the same way `instrument_editor.rs` holds it for the soundfont
//! panel: by walking the panel and asking the patch.

mod common;

use fontelle_types::{InstrumentKind, ParamAddress};
use fontelle_ui::canvas::ParamKind;
use fontelle_ui::document::{DocumentHost, StudioHost};

use common::SR;

/// A Flopsynth channel on the **Init** patch.
///
/// Away from Flopsynth and back, because a new project opens on one already
/// (playing the bank's Grand Piano — see `tests/starting_project.rs`) and
/// setting the kind a channel already is deliberately leaves its patch alone.
/// Every test here counts cards and routes, so what it needs is the patch with
/// nothing in it rather than whichever preset the studio happens to start on.
fn a_flopsynth() -> fontelle_app::Session {
    let mut session = common::a_session_for(common::a_project_with_a_clip(8, 120.0, SR));
    session.set_channel_kind(0, InstrumentKind::Osc3);
    session.set_channel_kind(0, InstrumentKind::Flopsynth);
    session
}

fn group<'a>(
    view: &'a fontelle_ui::canvas::InstrumentView,
    name: &str,
) -> &'a fontelle_ui::canvas::InstrumentGroup {
    view.groups
        .iter()
        .find(|g| g.name == name)
        .unwrap_or_else(|| {
            panic!(
                "no group called {name:?}; the panel has {:?}",
                view.groups
                    .iter()
                    .map(|g| g.name.clone())
                    .collect::<Vec<_>>()
            )
        })
}

/// One card per thing §8.3 draws, in the order it draws them.
#[test]
fn the_panel_has_a_card_for_every_part_of_the_synth() {
    let session = a_flopsynth();
    let view = session.instrument().expect("a Flopsynth has a panel");
    let names: Vec<&str> = view.groups.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "Channel",
            "Voice",
            "OSC A",
            "OSC B",
            "OSC C",
            "SUB",
            "NOISE",
            "Filter 1",
            "Filter 2",
            "ENV 1 \u{b7} amp",
            "ENV 2 \u{b7} filter",
            "ENV 3",
            "ENV 4",
            "ENV 5",
            "ENV 6",
            "LFO 1",
            "LFO 2",
            "LFO 3",
            "LFO 4",
            "LFO 5",
            "LFO 6",
            "LFO 7",
            "LFO 8",
            "SEQ 1",
            "SEQ 2",
            "Chaos",
            "Walk",
            "Macros",
            "Modulation (1)",
        ],
        "the panel is the signal path, in order"
    );
}

/// Gate 2 of §1: **every control on the window is a stable address** that
/// automation reaches. Held by construction — the panel is built from the same
/// list `realise`'s `param_nodes` map reads.
#[test]
fn every_control_on_the_panel_can_be_automated() {
    let session = a_flopsynth();
    let view = session.instrument().expect("a panel");
    let patch = session.selected_patch().expect("a patch");
    let addresses = fontelle_core::flopsynth::addresses(&patch);

    let mut seen = 0;
    for group in &view.groups {
        for control in &group.params {
            let address = control.address.as_str();
            if !address.starts_with("patch/") {
                // The channel's own level and placement, which belong to the
                // channel and not to the patch — see `channel_group`.
                continue;
            }
            seen += 1;
            assert!(
                addresses.contains(&address.to_string()),
                "{address} is on the panel and not in `flopsynth::addresses`, \
                 so a lane made for it would be drawn, saved and silent"
            );
            assert!(
                fontelle_core::patch_params::value(&patch, address).is_some(),
                "{address} is on the panel and cannot be read"
            );
        }
    }
    assert!(seen > 100, "only {seen} controls on the panel");
}

#[test]
fn every_control_reads_back_what_the_panel_writes() {
    let controls: Vec<(fontelle_types::ParamAddress, ParamKind)> = a_flopsynth()
        .instrument()
        .expect("a panel")
        .groups
        .iter()
        .flat_map(|g| g.params.iter())
        .filter(|p| p.address.as_str().starts_with("patch/"))
        .map(|p| (p.address.clone(), p.kind.clone()))
        .collect();

    for (address, kind) in controls {
        // Each on the Init patch afresh: a chooser written at the top of
        // its travel changes what the card draws (a Dual filter has no
        // slope), and a control that a *previous* write took off the
        // panel is not one that failed to read back.
        let mut session = a_flopsynth();
        session.set_instrument_param(&address, 1.0);
        let view = session.instrument().expect("a panel");
        let control = view
            .groups
            .iter()
            .flat_map(|g| g.params.iter())
            .find(|p| p.address == address)
            .unwrap_or_else(|| panic!("{address} left the panel after being written"));
        assert!(
            (control.value - 1.0).abs() < 0.02,
            "{address}: wrote 1.0, the panel reads {}",
            control.value
        );
        assert!(
            !control.display.is_empty(),
            "{address} has no read-out, and a knob whose number is blank is a \
             knob nobody can set on purpose"
        );
        assert!(!control.label.is_empty(), "{address} has no caption");
        // A switch reads on or off and nothing between.
        if matches!(kind, ParamKind::Switch) {
            session.set_instrument_param(&address, 0.4);
            let value = session
                .instrument()
                .expect("a panel")
                .groups
                .iter()
                .flat_map(|g| g.params.iter())
                .find(|p| p.address == address)
                .map(|p| p.value)
                .unwrap();
            assert!(
                value == 0.0 || value == 1.0,
                "{address} is a switch and reads {value}"
            );
        }
    }
}

/// A macro's caption **is** its name, which is the whole of what makes a
/// preset playable from one knob.
#[test]
fn a_macros_caption_is_its_name() {
    let mut session = a_flopsynth();
    // Straight off the Init patch, a macro nobody has named is still a knob.
    let view = session.instrument().expect("a panel");
    let macros = group(&view, "Macros");
    assert_eq!(macros.params.len(), fontelle_core::MACRO_COUNT);
    assert_eq!(macros.params[0].label, "macro 1");

    // And a preset that names them says so on the panel.
    session.open_file(0).expect("Flopsynth's bank opens");
    let at = session
        .library_presets()
        .iter()
        .position(|e| e.name == "Choir Ahh")
        .expect("the bank has a Choir Ahh");
    session.set_channel_instrument(at).expect("it lands");

    let view = session.instrument().expect("a panel");
    let macros = group(&view, "Macros");
    let names: Vec<&str> = macros.params.iter().map(|p| p.label.as_str()).collect();
    assert!(
        names.contains(&"Vowel"),
        "Choir Ahh names its first macro Vowel; the panel says {names:?}"
    );
}

/// The character knob's caption is the **model's**, and a model that has no
/// use for it does not get a knob that does nothing (§8.3).
#[test]
fn the_filter_character_knob_is_named_by_its_model_and_hidden_without_one() {
    let mut session = a_flopsynth();
    // The Init patch's filter 1 is Clean, which has no character.
    let view = session.instrument().expect("a panel");
    assert!(
        !group(&view, "Filter 1")
            .params
            .iter()
            .any(|p| p.address.as_str() == "patch/filter[0]/character"),
        "a Clean filter has no character, so it must not draw the knob"
    );

    // Ladder is the second of the ten models.
    let model = fontelle_types::ParamAddress::new("patch/filter[0]/model");
    session.set_instrument_param(&model, 1.0 / 9.0);
    let view = session.instrument().expect("a panel");
    let character = group(&view, "Filter 1")
        .params
        .iter()
        .find(|p| p.address.as_str() == "patch/filter[0]/character")
        .expect("a ladder has a character knob");
    assert_eq!(character.label, "saturation");
}

/// The matrix's rows are captioned with the route itself, so a panel of eight
/// of them is readable rather than eight knobs called "depth".
#[test]
fn a_modulation_rows_caption_is_the_route_it_is() {
    let session = a_flopsynth();
    let view = session.instrument().expect("a panel");
    let modulation = group(&view, "Modulation (1)");
    assert_eq!(modulation.params.len(), 1);
    assert_eq!(
        modulation.params[0].label, "ENV 2 \u{2192} Filter 1 cutoff",
        "the Init patch's one route, named as what it is"
    );
    assert_eq!(modulation.params[0].address.as_str(), "patch/mod[0]/depth");
}

/// The noise layer has no table, no position and no warp — so the panel does
/// not offer them.
#[test]
fn the_noise_card_draws_only_what_noise_has() {
    let session = a_flopsynth();
    let view = session.instrument().expect("a panel");
    let noise = group(&view, "NOISE");
    let addresses: Vec<&str> = noise.params.iter().map(|p| p.address.as_str()).collect();
    assert!(addresses.contains(&"patch/layer[4]/synth/noise_colour"));
    for missing in [
        "patch/layer[4]/synth/table",
        "patch/layer[4]/synth/position",
        "patch/layer[4]/synth/warp",
        "patch/layer[4]/synth/unison/voices",
    ] {
        assert!(
            !addresses.contains(&missing),
            "{missing} is on the noise card and noise has no such control"
        );
    }
}

/// Oversampling (`docs/flopsynth-next.md` §4.1): one chooser on the Voice
/// card for the patch, one at the end of every oscillator that reads
/// something, none on the noise — and both are Small, three short words.
#[test]
fn the_oversampling_is_a_chooser_on_the_voice_card_and_on_every_oscillator() {
    use fontelle_ui::canvas::{FlopsynthPage, KnobSize, ParamKind};
    let session = a_flopsynth();
    let view = session.instrument().expect("a panel");
    let voice = group(&view, "Voice");
    let over = voice
        .params
        .iter()
        .find(|p| p.address.as_str() == "patch/oversampling")
        .expect("the Voice card carries the patch's oversampling");
    assert_eq!(over.label, "oversample");
    assert_eq!(over.display, "Off");
    assert_eq!(
        over.kind,
        ParamKind::Choice(vec!["Off".into(), "2\u{d7}".into(), "4\u{d7}".into()])
    );
    // Last on the card: the addresses are in the panel's order, and the new
    // one came after everything that was there (ground rule 5).
    assert_eq!(
        voice.params.last().unwrap().address.as_str(),
        "patch/oversampling"
    );

    for (name, layer) in [("OSC A", 0), ("OSC B", 1), ("OSC C", 2), ("SUB", 3)] {
        let card = group(&view, name);
        let last = card.params.last().unwrap();
        assert_eq!(
            last.address.as_str(),
            format!("patch/layer[{layer}]/synth/quality"),
            "{name}'s quality is its last control"
        );
        assert_eq!(last.label, "quality");
        assert_eq!(last.display, "Off");
    }
    let noise = group(&view, "NOISE");
    assert!(
        !noise
            .params
            .iter()
            .any(|p| p.address.as_str().ends_with("synth/quality")),
        "the noise has no read to oversample"
    );

    // Small on the window, and captioned.
    let window = session.flopsynth(FlopsynthPage::Synth).unwrap();
    for name in ["OSC A", "Voice"] {
        let card = window.cards.iter().find(|c| c.group.name == name).unwrap();
        let (index, param) = card
            .group
            .params
            .iter()
            .enumerate()
            .find(|(_, p)| {
                let address = p.address.as_str();
                address == "patch/oversampling" || address.ends_with("synth/quality")
            })
            .unwrap();
        assert_eq!(card.size_of(index), KnobSize::Small, "{name}'s chooser");
        assert_eq!(
            param.label,
            if name == "Voice" {
                "OVERSAMP"
            } else {
                "QUALITY"
            }
        );
    }
    // And the Synth page still fits with the two of them on it.
    assert_page_fits(&session, FlopsynthPage::Synth);
}

/// A preset with effects on it grows the cards for them, built from the
/// effect's own `ParamSpec` list — so a patch effect's knobs are automatable
/// the day they exist.
#[test]
fn a_preset_with_effects_draws_a_card_for_each() {
    let mut session = a_flopsynth();
    session.open_file(0).expect("the bank opens");
    let at = session
        .library_presets()
        .iter()
        // Init Pad ships a chorus and a reverb.
        .position(|e| e.name == "Init Pad")
        .expect("the bank has an Init Pad");
    session.set_channel_instrument(at).expect("it lands");

    let view = session.instrument().expect("a panel");
    let names: Vec<&str> = view.groups.iter().map(|g| g.name.as_str()).collect();
    assert!(
        names.iter().any(|n| n.starts_with("FX 1")),
        "the panel has no effect cards: {names:?}"
    );
    let fx = view
        .groups
        .iter()
        .find(|g| g.name.starts_with("FX 1"))
        .unwrap();
    assert!(
        fx.params.len() > 3,
        "an effect card draws the effect's own parameters"
    );
    assert_eq!(fx.params[0].address.as_str(), "patch/fx[0]/enabled");
    // And every one of them is an address the patch accepts.
    let patch = session.selected_patch().expect("a patch");
    for control in &fx.params {
        assert!(
            fontelle_core::patch_params::value(&patch, control.address.as_str()).is_some(),
            "{} is drawn and cannot be read",
            control.address
        );
    }
}

// ----------------------------------------------------- the bespoke window ---

/// The window §8 asks for: cards laid out as the signal flows, each carrying a
/// picture drawn from the numbers the voice reads.
#[test]
fn a_flopsynth_channel_offers_its_own_window_and_nothing_else_does() {
    use fontelle_ui::canvas::FlopsynthPicture;

    let session = a_flopsynth();
    let view = session
        .flopsynth(fontelle_ui::canvas::FlopsynthPage::Synth)
        .expect("a Flopsynth channel has Flopsynth's window");
    let names: Vec<&str> = view.cards.iter().map(|c| c.group.name.as_str()).collect();
    assert!(names.contains(&"OSC A") && names.contains(&"Filter 1"));

    // The bands: sources first, then what they go through, then what moves
    // those. A window whose rows were worked out by wrapping would put the
    // filter beside the third oscillator.
    let band = |name: &str| {
        view.cards
            .iter()
            .find(|c| c.group.name == name)
            .map(|c| c.row)
            .unwrap_or_else(|| panic!("no card called {name}"))
    };
    assert_eq!(band("OSC A"), band("OSC C"), "the oscillators share a band");
    assert!(band("Filter 1") > band("OSC A"));
    assert_eq!(
        band("Macros"),
        band("Filter 1"),
        "two bands since the envelopes left the page"
    );

    // The LFOs are on no page (`docs/flopsynth-next.md` §3.4): the Matrix
    // page is the table, and an LFO is edited in the strip's inspector.
    let modulation = session
        .flopsynth(fontelle_ui::canvas::FlopsynthPage::Modulation)
        .expect("a Flopsynth channel has every page");
    let on_matrix: Vec<&str> = modulation
        .cards
        .iter()
        .map(|c| c.group.name.as_str())
        .collect();
    assert!(
        on_matrix.is_empty(),
        "the Matrix page is the table: {on_matrix:?}"
    );
    assert!(!names.contains(&"LFO 1"), "the Synth page has no LFOs");
    assert!(
        !modulation.sources.is_empty(),
        "the Matrix page lists the sources a badge can be dragged from"
    );
    let lfo = modulation
        .sources
        .iter()
        .position(|s| s == "LFO 1")
        .unwrap();
    let inspecting = session
        .flopsynth_inspecting(fontelle_ui::canvas::FlopsynthPage::Modulation, Some(lfo))
        .unwrap();
    assert!(
        inspecting.cards.iter().any(|c| c.group.name == "LFO 1"),
        "an LFO's card comes with the inspector"
    );

    // And every other instrument gets the knob grid, not this.
    let mut other = common::a_session_for(common::a_project_with_a_clip(8, 120.0, SR));
    other.set_channel_kind(0, InstrumentKind::Osc3);
    assert!(
        other
            .flopsynth(fontelle_ui::canvas::FlopsynthPage::Synth)
            .is_none(),
        "only a Flopsynth draws Flopsynth's window"
    );
    assert!(other.instrument().is_some(), "and the rest get the grid");

    // The pictures are the right kind for the card they are on.
    let picture = |name: &str| {
        view.cards
            .iter()
            .find(|c| c.group.name == name)
            .map(|c| c.picture.clone())
            .unwrap()
    };
    assert!(matches!(picture("OSC A"), FlopsynthPicture::Wave { .. }));
    assert!(matches!(
        picture("Filter 1"),
        FlopsynthPicture::Response { .. }
    ));
    // The envelopes and the LFOs are in the inspector, and the picture is
    // the same picture.
    let env = session
        .flopsynth_inspecting(fontelle_ui::canvas::FlopsynthPage::Modulation, Some(0))
        .unwrap()
        .cards
        .iter()
        .find(|c| c.group.name == "ENV 1 \u{b7} amp")
        .map(|c| c.picture.clone())
        .expect("the inspector draws the envelope");
    assert!(matches!(env, FlopsynthPicture::Envelope { .. }));
    let lfo = inspecting
        .cards
        .iter()
        .find(|c| c.group.name == "LFO 1")
        .map(|c| c.picture.clone())
        .expect("the inspector draws the LFO");
    assert!(matches!(lfo, FlopsynthPicture::Lfo { .. }));
    // Noise has no cycle to draw, and a picture of one realisation of it would
    // be a different picture every frame.
    assert_eq!(picture("NOISE"), FlopsynthPicture::None);
    assert_eq!(picture("Macros"), FlopsynthPicture::None);
}

/// §8.1 rule 5: the picture is computed from the numbers the voice reads, so
/// **moving a knob moves the picture**. One that did not would be a decoration.
#[test]
fn the_pictures_follow_the_controls_that_make_them() {
    use fontelle_ui::canvas::FlopsynthPicture;
    let mut session = a_flopsynth();

    let wave_of = |session: &fontelle_app::Session| match session
        .flopsynth(fontelle_ui::canvas::FlopsynthPage::Synth)
        .unwrap()
        .cards
        .iter()
        .find(|c| c.group.name == "OSC A")
        .map(|c| c.picture.clone())
        .unwrap()
    {
        FlopsynthPicture::Wave { points, position } => (points, position),
        other => panic!("expected a wave, got {other:?}"),
    };

    // A different table is a different wave.
    let (saw, _) = wave_of(&session);
    session.set_instrument_param(
        &fontelle_types::ParamAddress::new("patch/layer[0]/synth/table"),
        0.0,
    );
    let (sine, _) = wave_of(&session);
    let difference: f32 = saw
        .iter()
        .zip(&sine)
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>()
        / saw.len() as f32;
    assert!(
        difference > 0.05,
        "choosing a table has to change the picture: {difference}"
    );

    // And on a morphing table, so is a different position.
    session.set_instrument_param(
        &fontelle_types::ParamAddress::new("patch/layer[0]/synth/table"),
        // Analog Morph is the sixth of forty.
        5.0 / 39.0,
    );
    let (low, at_low) = wave_of(&session);
    session.set_instrument_param(
        &fontelle_types::ParamAddress::new("patch/layer[0]/synth/position"),
        1.0,
    );
    let (high, at_high) = wave_of(&session);
    assert!(at_low < at_high, "the position knob is on the picture");
    let moved: f32 = low
        .iter()
        .zip(&high)
        .map(|(a, b)| (a - b).abs())
        .sum::<f32>()
        / low.len() as f32;
    assert!(
        moved > 0.05,
        "the position knob has to move the frame under it: {moved}"
    );

    // The filter's curve follows its cutoff: closing it takes the top off.
    let high_at = |session: &fontelle_app::Session, index: usize| match session
        .flopsynth(fontelle_ui::canvas::FlopsynthPage::Synth)
        .unwrap()
        .cards
        .iter()
        .find(|c| c.group.name == "Filter 1")
        .map(|c| c.picture.clone())
        .unwrap()
    {
        FlopsynthPicture::Response { points, .. } => points[index],
        other => panic!("expected a response, got {other:?}"),
    };
    let open = high_at(&session, 80);
    session.set_instrument_param(
        &fontelle_types::ParamAddress::new("patch/filter[0]/cutoff"),
        0.3,
    );
    let closed = high_at(&session, 80);
    assert!(
        closed < open - 6.0,
        "closing the cutoff has to take the top off the curve: {open:.1} dB \
         against {closed:.1} dB"
    );
}

/// A switched-off filter is a wire, and its picture says so rather than
/// drawing the curve it *would* have.
#[test]
fn a_switched_off_filter_draws_a_flat_line() {
    use fontelle_ui::canvas::FlopsynthPicture;
    let mut session = a_flopsynth();
    // Filter 2 is off in the Init patch.
    let curve = match session
        .flopsynth(fontelle_ui::canvas::FlopsynthPage::Synth)
        .unwrap()
        .cards
        .iter()
        .find(|c| c.group.name == "Filter 2")
        .map(|c| c.picture.clone())
        .unwrap()
    {
        FlopsynthPicture::Response { points, .. } => points,
        other => panic!("expected a response, got {other:?}"),
    };
    assert!(
        curve.iter().all(|db| db.abs() < 0.01),
        "an off filter draws unity, not the curve it would have"
    );

    // Switching it on gives it one.
    session.set_instrument_param(
        &fontelle_types::ParamAddress::new("patch/filter[1]/enabled"),
        1.0,
    );
    let on = match session
        .flopsynth(fontelle_ui::canvas::FlopsynthPage::Synth)
        .unwrap()
        .cards
        .iter()
        .find(|c| c.group.name == "Filter 2")
        .map(|c| c.picture.clone())
        .unwrap()
    {
        FlopsynthPicture::Response { points, .. } => points,
        other => panic!("expected a response, got {other:?}"),
    };
    assert!(
        on.iter().any(|db| db.abs() > 1.0),
        "and then it has a curve"
    );
}

/// Every control on the window is still an address automation reaches — the
/// same claim the grid makes, made again for the window that replaced it.
#[test]
fn every_control_on_the_bespoke_window_can_be_automated() {
    let session = a_flopsynth();
    let view = session
        .flopsynth(fontelle_ui::canvas::FlopsynthPage::Synth)
        .expect("a window");
    let patch = session.selected_patch().expect("a patch");
    let addresses = fontelle_core::flopsynth::addresses(&patch);
    let mut seen = 0;
    for card in &view.cards {
        for control in &card.group.params {
            let address = control.address.as_str();
            if !address.starts_with("patch/") {
                continue;
            }
            seen += 1;
            assert!(
                addresses.contains(&address.to_string()),
                "{address} is on the window and not automatable"
            );
        }
    }
    assert!(seen > 100, "only {seen} controls");
}

/// The shape of each card is declared by the layer that knows what the card
/// *is* — how many cells across, and whether it stands aside — so the window
/// is laid out as a page rather than as a wrapping list. The first build let
/// the layout guess, and the guess put the voice above the oscillators and
/// the noise off the bottom.
#[test]
fn the_synth_page_declares_each_cards_shape() {
    let session = a_flopsynth();
    let view = session
        .flopsynth(fontelle_ui::canvas::FlopsynthPage::Synth)
        .expect("Flopsynth's window");
    let card = |name: &str| {
        view.cards
            .iter()
            .find(|c| c.group.name == name)
            .unwrap_or_else(|| panic!("no card called {name}"))
    };
    for osc in ["OSC A", "OSC B", "OSC C"] {
        assert_eq!(card(osc).columns, 5, "{osc} is five cells across");
        assert!(!card(osc).aside);
    }
    for slim in ["SUB", "NOISE"] {
        assert!(
            card(slim).aside,
            "{slim} stands aside, in a column of its own"
        );
        assert_eq!(card(slim).columns, 4);
        assert_eq!(
            card(slim).row,
            card("OSC A").row,
            "and is a source, so it is in their band"
        );
    }
    // Both filters four across, in the second band.
    let filters: Vec<_> = view
        .cards
        .iter()
        .filter(|c| c.group.name.starts_with("Filter"))
        .collect();
    assert!(!filters.is_empty());
    for filter in filters {
        assert_eq!(filter.columns, 4);
        assert!(!filter.aside);
    }
    assert_eq!(card("Voice").columns, 3);
    assert_eq!(card("Macros").columns, 4);
    // The voice (with the channel's two knobs) and the macros share the
    // filters' band: two bands, the envelopes off the page.
    assert_eq!(card("Voice").row, card("Filter 1").row);
    assert_eq!(card("Macros").row, card("Filter 1").row);
    assert!(card("Filter 1").row > card("OSC A").row);
    // And the whole page fits the window it opens at — the layout's own test
    // holds this with a fixture; this holds it with the real view.
    let theme = fontelle_ui::theme::Theme::dark_default();
    let (w, h) = fontelle_ui::layout::FLOPSYNTH_SIZE;
    let body = fontelle_ui::layout::editor_window_layout(w as f32, h as f32, &theme.metrics).body;
    let layout = fontelle_ui::canvas::flopsynth_layout(body, &theme.metrics, &view);
    for (index, placed) in layout.cards.iter().enumerate() {
        assert!(
            !placed.frame.is_empty() && placed.frame.bottom() <= body.bottom() + 0.01,
            "{} runs off the window: {:?}",
            view.cards[index].group.name,
            placed.frame
        );
    }
}

/// §8.6: the Presets page is a view over the bank, and the host hands the
/// bank over with the view — on that page and on no other, because a hundred
/// and twenty-eight rows built for a page that is not showing is work nobody
/// sees.
#[test]
fn the_presets_page_carries_the_bank_and_the_others_do_not() {
    use fontelle_ui::canvas::FlopsynthPage;
    let session = a_flopsynth();
    let presets = session
        .flopsynth(FlopsynthPage::Presets)
        .expect("Flopsynth's window");
    assert!(
        presets.bank.len() >= 128,
        "the page lists the whole factory bank, not {}",
        presets.bank.len()
    );
    assert!(
        presets
            .bank
            .iter()
            .any(|p| p.name == "Choir Ahh" && p.category == "Choir & Vocal"),
        "a preset is listed under its category"
    );
    assert!(
        presets.cards.is_empty(),
        "the Presets page is the bank, not cards"
    );
    let synth = session
        .flopsynth(FlopsynthPage::Synth)
        .expect("the Synth page");
    assert!(synth.bank.is_empty());
}

/// §8.5: the chain is edited from the window — an effect is added from the
/// `+ effect` list and taken off its card — and the list offers only the
/// kinds that cost no latency, because an instrument's latency is the one
/// case the graph does not compensate.
#[test]
fn an_effect_is_added_to_the_patch_from_the_window_and_taken_off_again() {
    use fontelle_types::EffectKind;
    use fontelle_ui::canvas::FlopsynthPage;
    let mut session = a_flopsynth();
    let kinds = session.patch_effect_kinds();
    assert_eq!(
        kinds.first(),
        Some(&EffectKind::Chorus),
        "the list is §3.9's, in its order"
    );
    assert!(
        !kinds.contains(&EffectKind::Gate),
        "the gate looks ahead, so it is not offered"
    );
    assert!(kinds.contains(&EffectKind::Reverb) && kinds.contains(&EffectKind::Compressor));

    let bare = session
        .flopsynth(FlopsynthPage::Effects)
        .expect("the Effects page");
    assert!(
        bare.cards.is_empty() && bare.fx_room,
        "the Init patch has no effects and room for one"
    );

    session.add_patch_effect(EffectKind::Chorus);
    let view = session
        .flopsynth(FlopsynthPage::Effects)
        .expect("the Effects page");
    let chorus = view
        .cards
        .iter()
        .find(|c| c.group.name == "FX 1 \u{b7} Chorus")
        .expect("a card for the chorus");
    assert!(chorus.removable, "an effect card can be taken off");
    assert!(
        session
            .flopsynth(FlopsynthPage::Synth)
            .unwrap()
            .cards
            .iter()
            .all(|c| !c.removable),
        "nothing on the Synth page can be"
    );

    session.remove_patch_effect(0);
    assert!(
        session
            .flopsynth(FlopsynthPage::Effects)
            .unwrap()
            .cards
            .is_empty()
    );

    // A full chain has no room, and a fifth effect is refused rather than
    // squeezed in.
    for _ in 0..fontelle_core::MAX_PATCH_FX {
        session.add_patch_effect(EffectKind::Reverb);
    }
    let full = session.flopsynth(FlopsynthPage::Effects).unwrap();
    assert_eq!(full.rack.len(), fontelle_core::MAX_PATCH_FX);
    assert!(!full.fx_room);
    session.add_patch_effect(EffectKind::Delay);
    assert_eq!(
        session
            .flopsynth(FlopsynthPage::Effects)
            .unwrap()
            .rack
            .len(),
        fontelle_core::MAX_PATCH_FX
    );
}

/// §3.6: the Effects page is a rack — a row per slot with its name, its
/// on/off, its wet/dry and its level — beside the **selected** slot's card,
/// and only that one, with the effect's own picture on it: the delay's
/// taps, the reverb's tail, the distortion's transfer curve, the EQ's
/// response, the compressor's gain curve.
#[test]
fn the_effects_page_is_a_rack_beside_the_selected_slots_card_with_its_picture() {
    use fontelle_types::EffectKind;
    use fontelle_ui::canvas::{FlopsynthPage, FlopsynthPicture, FlopsynthShowing};
    let mut session = a_flopsynth();
    for kind in [
        EffectKind::Chorus,
        EffectKind::Delay,
        EffectKind::Reverb,
        EffectKind::Distortion,
        EffectKind::Eq,
        EffectKind::Compressor,
    ] {
        session.add_patch_effect(kind);
    }
    let view = session.flopsynth(FlopsynthPage::Effects).unwrap();
    assert_eq!(view.rack.len(), 6);
    assert_eq!(view.rack[1].name, "FX 2 \u{b7} Delay");
    assert!(view.rack.iter().all(|row| row.enabled));
    let patch = session.selected_patch().unwrap();
    for (row, slot) in view.rack.iter().zip(&patch.fx) {
        assert!((row.mix - slot.config.mix()).abs() < 1e-6);
        assert_eq!(row.level, 0.0, "nothing is sounding");
    }
    // Nothing chosen: the first slot's card, and only that.
    assert_eq!(view.fx_slot, Some(0));
    assert_eq!(view.cards.len(), 1);
    assert_eq!(view.cards[0].group.name, "FX 1 \u{b7} Chorus");
    // Each slot's card, with its picture.
    let card_of = |session: &fontelle_app::Session, slot: usize| {
        let view = session
            .flopsynth_showing(
                FlopsynthPage::Effects,
                FlopsynthShowing {
                    inspector: None,
                    fx_slot: Some(slot),
                    wave_tool: Default::default(),
                },
            )
            .unwrap();
        assert_eq!(view.fx_slot, Some(slot));
        assert_eq!(view.cards.len(), 1);
        view.cards[0].clone()
    };
    let delay = card_of(&session, 1);
    let FlopsynthPicture::Curve { marks, points, .. } = &delay.picture else {
        panic!("the delay's taps: {:?}", delay.picture);
    };
    assert!(marks.len() >= 2 && points.is_empty(), "taps, not a curve");
    assert!(
        marks.windows(2).all(|w| w[0].0 < w[1].0 && w[0].1 > w[1].1),
        "later and quieter: {marks:?}"
    );
    let reverb = card_of(&session, 2);
    let FlopsynthPicture::Curve { points, .. } = &reverb.picture else {
        panic!("the reverb's tail");
    };
    assert!(points.len() >= 32 && points[0] > 0.9 && points[points.len() - 1] < 0.1);
    assert!(points.windows(2).all(|w| w[0] >= w[1]), "a tail falls");
    let distortion = card_of(&session, 3);
    let FlopsynthPicture::Curve { points, .. } = &distortion.picture else {
        panic!("the distortion's transfer curve");
    };
    assert!(points.len() >= 32 && points.iter().all(|p| (0.0..=1.0).contains(p)));
    assert!(
        points[0] < 0.5 && points[points.len() - 1] > 0.5,
        "in below, out above"
    );
    let eq = card_of(&session, 4);
    let FlopsynthPicture::Curve { points, marks, .. } = &eq.picture else {
        panic!("the EQ's response");
    };
    let bands_on = patch.fx[4].config.specs().len();
    let _ = bands_on;
    assert!(points.len() >= 32, "a response");
    assert!(
        points.iter().all(|p| (p - 0.5).abs() < 1e-3),
        "flat, with every band off"
    );
    assert!(marks.is_empty(), "a dot per band that is on — none yet");
    // A compressor at its default ratio of 1 is a wire; at 4:1 the top of
    // its curve is held down.
    let compressor = card_of(&session, 5);
    let FlopsynthPicture::Curve { points, .. } = &compressor.picture else {
        panic!("the compressor's gain curve");
    };
    assert!(points.len() >= 32);
    assert!(
        (points[points.len() - 1] - 1.0).abs() < 1e-3,
        "a wire at 1:1"
    );
    let ratio = compressor
        .group
        .params
        .iter()
        .find(|p| p.address.as_str().ends_with("/ratio"))
        .expect("the ratio")
        .address
        .clone();
    session.set_instrument_param(&ratio, 0.5);
    let compressor = card_of(&session, 5);
    let FlopsynthPicture::Curve { points, .. } = &compressor.picture else {
        panic!("the compressor's gain curve");
    };
    assert!(
        points[points.len() - 1] < 0.95,
        "the top is held down: {}",
        points[points.len() - 1]
    );
    assert!(
        points.windows(2).all(|w| w[0] <= w[1] + 1e-6),
        "and it never falls"
    );
    // A slot past the end shows the last.
    let last = session
        .flopsynth_showing(
            FlopsynthPage::Effects,
            FlopsynthShowing {
                inspector: None,
                fx_slot: Some(99),
                wave_tool: Default::default(),
            },
        )
        .unwrap();
    assert_eq!(last.fx_slot, Some(5));
    // And the slot's card still carries the chain's controls, addressed
    // as ever, so a lane on `patch/fx[1]/mix` is unchanged.
    assert!(
        delay
            .group
            .params
            .iter()
            .any(|p| p.address.as_str() == "patch/fx[1]/mix")
    );
}

/// A slot is moved along the chain from the window — the drag by the
/// card's header that `FlopsynthHit::Header` was always for
/// (`docs/flopsynth-next.md` §1.4(5)) — and it is one undo.
#[test]
fn an_effect_slot_is_moved_along_the_chain_and_it_is_one_undo() {
    use fontelle_types::EffectKind;
    use fontelle_ui::canvas::FlopsynthPage;
    let mut session = a_flopsynth();
    for kind in [EffectKind::Chorus, EffectKind::Delay, EffectKind::Reverb] {
        session.add_patch_effect(kind);
    }
    // The rack's rows are the chain, in order (§3.6).
    let names = |session: &fontelle_app::Session| -> Vec<String> {
        session
            .flopsynth(FlopsynthPage::Effects)
            .unwrap()
            .rack
            .iter()
            .map(|row| row.name.clone())
            .collect()
    };
    assert_eq!(
        names(&session),
        [
            "FX 1 \u{b7} Chorus",
            "FX 2 \u{b7} Delay",
            "FX 3 \u{b7} Reverb"
        ]
    );

    // The reverb dragged onto the chorus's card goes first; the others slide.
    session.move_patch_effect(2, 0);
    assert_eq!(
        names(&session),
        [
            "FX 1 \u{b7} Reverb",
            "FX 2 \u{b7} Chorus",
            "FX 3 \u{b7} Delay"
        ]
    );
    // The chain is what sounds: the patch says so too, not only the cards.
    let patch = session.selected_patch().expect("a patch");
    assert_eq!(patch.fx[0].config.kind(), EffectKind::Reverb);
    assert_eq!(patch.fx[2].config.kind(), EffectKind::Delay);

    // Forwards too: the first dragged onto the last goes last.
    session.move_patch_effect(0, 2);
    assert_eq!(
        names(&session),
        [
            "FX 1 \u{b7} Chorus",
            "FX 2 \u{b7} Delay",
            "FX 3 \u{b7} Reverb"
        ]
    );

    // A slot dropped on itself, or an index that is not there, is nothing —
    // not an undo entry that does nothing.
    session.move_patch_effect(1, 1);
    session.move_patch_effect(7, 0);
    session.undo();
    assert_eq!(
        names(&session),
        [
            "FX 1 \u{b7} Reverb",
            "FX 2 \u{b7} Chorus",
            "FX 3 \u{b7} Delay"
        ],
        "one undo takes back the last move, whole"
    );
    // And only the last: a change to the chain's shape is one thing
    // somebody did, not a step of a drag to merge with the one before.
    session.undo();
    assert_eq!(
        names(&session),
        [
            "FX 1 \u{b7} Chorus",
            "FX 2 \u{b7} Delay",
            "FX 3 \u{b7} Reverb"
        ],
        "the second undo takes back the first move and nothing else"
    );
    session.undo();
    assert_eq!(
        names(&session),
        ["FX 1 \u{b7} Chorus", "FX 2 \u{b7} Delay"],
        "and the third takes off the last effect added, not all three"
    );
}

/// An effect card is built from the effect's own parameter table the way the
/// effect window is — a stepped parameter with named positions is a chooser
/// that says its names, and a read-out carries its unit — rather than from a
/// second formatter that printed a chorus's mode as "0.00".
#[test]
fn an_effect_card_is_the_effect_windows_own_controls() {
    use fontelle_types::EffectKind;
    use fontelle_ui::canvas::FlopsynthPage;
    let mut session = a_flopsynth();
    session.add_patch_effect(EffectKind::Chorus);
    let view = session
        .flopsynth(FlopsynthPage::Effects)
        .expect("the Effects page");
    let card = &view.cards[0];
    let param = |label: &str| {
        card.group
            .params
            .iter()
            .find(|p| p.label.eq_ignore_ascii_case(label))
            .unwrap_or_else(|| panic!("no control called {label} on {:?}", card.group.name))
    };
    assert!(
        matches!(param("mode").kind, ParamKind::Choice(_)),
        "a stepped parameter is a chooser, not a knob reading 0.00"
    );
    assert!(
        param("rate").display.ends_with("Hz"),
        "a read-out carries its unit: {:?}",
        param("rate").display
    );
    assert!(matches!(param("on").kind, ParamKind::Switch));
    assert!(
        card.group
            .params
            .iter()
            .all(|p| p.address.as_str().starts_with("patch/fx[0]/")),
        "every control is addressed on the patch's own chain"
    );
}

/// Everything on `page` at the size the window opens at: every card drawn
/// and inside the body, no two cards over one another, the matrix panel and
/// every route row inside the body and under the last card, and the
/// `+ effect` button (when there is one) in the body too.
///
/// `docs/flopsynth-next.md` §1.4(2): the fit was tested for the Synth page
/// and nothing else, and the Modulation page drew its matrix *under* the
/// ENV 3/4 cards — the first two of the Grand Piano's eleven routes hidden,
/// the last cut at the window's edge. The canopy took what the cards left
/// and forgot the badges and the matrix were on the page too.
fn assert_page_fits(session: &fontelle_app::Session, page: fontelle_ui::canvas::FlopsynthPage) {
    assert_page_fits_inspecting(session, page, None);
}

/// [`assert_page_fits`] with a source open in the inspector: the drawer and
/// its card are on the window, the page's own cards are still drawn under
/// it, and the window still has a body — a drawer that ate the page was a
/// blank window (2026-09-18, the first click on a badge on the Matrix page).
fn assert_page_fits_inspecting(
    session: &fontelle_app::Session,
    page: fontelle_ui::canvas::FlopsynthPage,
    inspector: Option<usize>,
) {
    assert_page_fits_showing(
        session,
        page,
        fontelle_ui::canvas::FlopsynthShowing {
            inspector,
            fx_slot: None,
            wave_tool: Default::default(),
        },
    );
}

fn assert_page_fits_showing(
    session: &fontelle_app::Session,
    page: fontelle_ui::canvas::FlopsynthPage,
    showing: fontelle_ui::canvas::FlopsynthShowing,
) {
    let inspector = showing.inspector;
    let view = session
        .flopsynth_showing(page, showing)
        .expect("Flopsynth's window");
    let theme = fontelle_ui::theme::Theme::dark_default();
    let (w, h) = fontelle_ui::layout::FLOPSYNTH_SIZE;
    let body = fontelle_ui::layout::editor_window_layout(w as f32, h as f32, &theme.metrics).body;
    let layout = fontelle_ui::canvas::flopsynth_layout(body, &theme.metrics, &view);
    let inside = |r: &fontelle_ui::layout::Rect| {
        r.x >= body.x - 0.01
            && r.right() <= body.right() + 0.01
            && r.y >= body.y - 0.01
            && r.bottom() <= body.bottom() + 0.01
    };
    assert!(
        !layout.body.is_empty() && !layout.canopy.is_empty() && !layout.strip.is_empty(),
        "{page:?} inspecting {inspector:?}: the window lost its body"
    );
    if view.inspector.is_some() {
        assert!(
            !layout.inspector.is_empty() && inside(&layout.inspector),
            "{page:?} inspecting {inspector:?}: the drawer is off the window: {:?}",
            layout.inspector
        );
        assert!(
            layout.inspector.bottom() <= layout.strip.y + 0.01,
            "{page:?} inspecting {inspector:?}: the drawer covers the strip"
        );
        assert!(
            view.cards
                .iter()
                .any(|c| c.row == fontelle_ui::canvas::INSPECTOR_ROW),
            "{page:?} inspecting {inspector:?}: no card in the drawer"
        );
    }
    let mut cards_bottom = body.y;
    for (index, placed) in layout.cards.iter().enumerate() {
        let name = &view.cards[index].group.name;
        assert!(!placed.frame.is_empty(), "{page:?}: {name} was not drawn");
        if view.cards[index].row == fontelle_ui::canvas::INSPECTOR_ROW {
            assert!(
                placed.frame.y >= layout.inspector.y - 0.01
                    && placed.frame.bottom() <= layout.inspector.bottom() + 0.01,
                "{page:?}: {name} is outside the drawer {:?}: {:?}",
                layout.inspector,
                placed.frame
            );
            assert_eq!(placed.cells.len(), view.cards[index].group.params.len());
            continue;
        }
        assert!(
            inside(&placed.frame),
            "{page:?}: {name} runs off the window: {:?} in {body:?}",
            placed.frame
        );
        assert_eq!(
            placed.cells.len(),
            view.cards[index].group.params.len(),
            "{page:?}: {name} lost a control"
        );
        cards_bottom = cards_bottom.max(placed.frame.bottom());
        for (other_index, other) in layout.cards.iter().enumerate().skip(index + 1) {
            // The drawer lies over the page: its card may cover this one.
            if view.cards[other_index].row == fontelle_ui::canvas::INSPECTOR_ROW {
                continue;
            }
            let (a, b) = (placed.frame, other.frame);
            let overlaps = a.x < b.right() - 0.01
                && b.x < a.right() - 0.01
                && a.y < b.bottom() - 0.01
                && b.y < a.bottom() - 0.01;
            assert!(
                !overlaps,
                "{page:?}: {name} and {} overlap: {a:?} and {b:?}",
                view.cards[other_index].group.name
            );
        }
    }
    if page == fontelle_ui::canvas::FlopsynthPage::Modulation {
        assert!(
            !layout.matrix.is_empty() && inside(&layout.matrix),
            "{page:?}: the matrix panel is off the window: {:?} in {body:?}",
            layout.matrix
        );
        assert!(
            layout.matrix.y >= cards_bottom - 0.01,
            "{page:?}: the matrix at {} starts under the cards, which end at {cards_bottom}",
            layout.matrix.y
        );
        if !layout.inspector.is_empty() {
            assert!(
                layout.matrix.y >= layout.inspector.bottom() - 0.01,
                "{page:?}: the table at {} runs under the drawer, which ends at {}",
                layout.matrix.y,
                layout.inspector.bottom()
            );
        }
        assert_eq!(layout.routes.len(), view.routes.len());
        // A row that does not fit is **not drawn** — the matrix scrolls
        // (`fontelle-ui/tests/flopsynth.rs`, the scrolling test) — and a
        // row that is drawn is inside the panel.
        let mut drawn = 0;
        for (index, row) in layout.routes.iter().enumerate() {
            if row.frame.is_empty() {
                continue;
            }
            drawn += 1;
            assert!(
                row.frame.y >= layout.matrix.y - 0.01
                    && row.frame.bottom() <= layout.matrix.bottom() + 0.01,
                "{page:?}: route {index} at {:?} is outside the matrix {:?}",
                row.frame,
                layout.matrix
            );
        }
        assert!(
            drawn >= fontelle_ui::canvas::MATRIX_ROWS_LEAST,
            "{page:?}: only {drawn} of {} routes are drawn at the design size",
            view.routes.len()
        );
        let hidden = view.routes.len() - drawn;
        assert!(
            layout.matrix_max_scroll >= hidden as f32 * fontelle_ui::canvas::MATRIX_ROW - 0.01,
            "{page:?}: {hidden} rows are hidden and the matrix scrolls only {}",
            layout.matrix_max_scroll
        );
        for (index, badge) in layout.badges.iter().enumerate() {
            assert!(
                !badge.is_empty() && inside(badge),
                "{page:?}: badge {index} is off the window"
            );
        }
    }
    if view.fx_room {
        assert!(
            !layout.add_effect.is_empty() && inside(&layout.add_effect),
            "{page:?}: the + effect button is off the window"
        );
    }
}

#[test]
fn the_modulation_page_fits_the_window_with_the_grand_pianos_routes() {
    // The project a studio opens on: the Grand Piano, eleven routes.
    let session = common::a_session_for(fontelle_app::blank_project(8, 120.0, SR));
    let view = session
        .flopsynth(fontelle_ui::canvas::FlopsynthPage::Modulation)
        .expect("Flopsynth's window");
    assert!(
        view.routes.len() >= 8,
        "the piano's matrix is what this test is for; it has {} routes",
        view.routes.len()
    );
    assert_page_fits(&session, fontelle_ui::canvas::FlopsynthPage::Modulation);
}

#[test]
fn the_effects_page_fits_the_window_with_four_slots() {
    use fontelle_types::EffectKind;
    let mut session = a_flopsynth();
    for kind in [
        EffectKind::Chorus,
        EffectKind::Delay,
        EffectKind::Reverb,
        EffectKind::Eq,
    ] {
        session.add_patch_effect(kind);
    }
    let view = session
        .flopsynth(fontelle_ui::canvas::FlopsynthPage::Effects)
        .expect("Flopsynth's window");
    assert_eq!(view.rack.len(), 4, "four slots, four rows");
    assert_eq!(view.cards.len(), 1, "and the selected slot's card");
    assert_page_fits(&session, fontelle_ui::canvas::FlopsynthPage::Effects);
    // Each slot's card fits, the EQ's sixteen-odd controls included.
    for slot in 0..4 {
        assert_page_fits_showing(
            &session,
            fontelle_ui::canvas::FlopsynthPage::Effects,
            fontelle_ui::canvas::FlopsynthShowing {
                inspector: None,
                fx_slot: Some(slot),
                wave_tool: Default::default(),
            },
        );
    }
}

/// At the smallest size the window may be dragged to, the Synth page of the
/// preset a studio opens on keeps every cell at its design size — the
/// layout's own test holds this with a fixture; this holds it with the
/// Grand Piano's real page, measured by the real shaper the way the window
/// measures it. `docs/flopsynth-next.md` §1.4(3): the floor is where no
/// caption or value is cut.
#[test]
fn at_the_minimum_size_the_grand_pianos_synth_page_keeps_every_cell_whole() {
    use fontelle_ui::canvas::{
        FLOP_CELL_H, FLOP_CELL_W, FlopsynthPage, ParamKind, is_nameplate_control,
    };
    let session = common::a_session_for(fontelle_app::blank_project(8, 120.0, SR));
    let view = session
        .flopsynth(FlopsynthPage::Synth)
        .expect("Flopsynth's window");
    let theme = fontelle_ui::theme::Theme::dark_default();
    let mut text = fontelle_ui::text::TextContext::new();
    let mut labels = fontelle_ui::text::Labels::new();
    for card in &view.cards {
        for param in &card.group.params {
            labels.ensure_small(&param.label, &theme.font, &mut text);
            if let ParamKind::Choice(options) = &param.kind {
                for option in options {
                    labels.ensure_small(option, &theme.font, &mut text);
                }
            }
        }
    }
    let measure = |s: &str| {
        labels
            .get_small(s)
            .map(|l| l.width)
            .unwrap_or_else(|| fontelle_ui::canvas::estimated_width(s))
    };
    let (w, h) = fontelle_ui::layout::flopsynth_window_size(1.0);
    let body = fontelle_ui::layout::editor_window_layout(w as f32, h as f32, &theme.metrics).body;
    let layout = fontelle_ui::canvas::flopsynth_layout_with(body, &theme.metrics, &view, &measure);
    for (index, placed) in layout.cards.iter().enumerate() {
        // Above the strip, not merely inside the window: a card that ran
        // into the strip was drawn under the badges (the Voice card, once
        // it had a picture and eleven controls at three cells wide).
        assert!(
            placed.frame.bottom() <= layout.strip.y + 0.01,
            "{} runs into the strip at the minimum size ({} past {})",
            view.cards[index].group.name,
            placed.frame.bottom(),
            layout.strip.y
        );
        for (param, cell) in &placed.cells {
            let control = &view.cards[index].group.params[*param];
            if is_nameplate_control(control) {
                continue;
            }
            let expected = if view.cards[index].is_half(*param) {
                fontelle_ui::canvas::FLOP_CELL_HALF
            } else {
                FLOP_CELL_H
            };
            assert!(
                cell.width >= FLOP_CELL_W - 0.01 && (cell.height - expected).abs() < 0.01,
                "{}'s {} is {}x{} at {w}x{h}",
                view.cards[index].group.name,
                control.label,
                cell.width,
                cell.height
            );
        }
    }
}

/// §3.1 and §3.5: every card declares a knob size per control — exactly one
/// Large per oscillator, filter and envelope (the knob a player reaches for
/// first: position or start or bright, cutoff, decay), the fine adjustments
/// Small, the rest Medium — and the two pages are composed as Ty decided on
/// 2026-09-18: the envelopes are off the Synth page, which is the sources
/// over the filters, the channel, the voice and the macros.
#[test]
fn every_card_declares_its_knob_sizes_and_the_synth_page_is_two_bands() {
    use fontelle_ui::canvas::{FlopsynthPage, KnobSize};
    let session = common::a_session_for(fontelle_app::blank_project(8, 120.0, SR));
    let view = session
        .flopsynth(FlopsynthPage::Synth)
        .expect("Flopsynth's window");
    let card = |name: &str| {
        view.cards
            .iter()
            .find(|c| c.group.name == name)
            .unwrap_or_else(|| panic!("no card called {name}"))
    };
    let large_of = |name: &str| -> Vec<String> {
        let c = card(name);
        c.group
            .params
            .iter()
            .enumerate()
            .filter(|(i, _)| c.size_of(*i) == KnobSize::Large)
            .map(|(_, p)| p.label.clone())
            .collect()
    };
    for c in &view.cards {
        assert_eq!(
            c.sizes.len(),
            c.group.params.len(),
            "{} declares a size for every control",
            c.group.name
        );
    }
    // The Grand Piano: two sampled oscillators, one string.
    assert_eq!(large_of("OSC A"), ["START"]);
    assert_eq!(large_of("OSC C"), ["BRIGHT"]);
    assert_eq!(large_of("Filter 1"), ["CUTOFF"]);
    assert!(
        large_of("SUB").is_empty(),
        "the sub is set aside, nothing on it is Large"
    );
    let small = |name: &str, label: &str| {
        let c = card(name);
        let i = c
            .group
            .params
            .iter()
            .position(|p| p.label == label)
            .unwrap();
        c.size_of(i) == KnobSize::Small
    };
    for label in ["PAN", "SEMI", "FINE", "WIDTH", "BLEND"] {
        assert!(small("OSC A", label), "{label} is a fine adjustment");
    }
    assert!(small("Filter 1", "KEY TRK"));
    assert!(!small("OSC A", "LEVEL") && !small("OSC A", "UNISON"));

    // Two bands: sources in 0, everything else in 1, nothing further.
    for c in &view.cards {
        let expected = match c.group.name.as_str() {
            "OSC A" | "OSC B" | "OSC C" | "SUB" | "NOISE" => 0,
            _ => 1,
        };
        assert_eq!(c.row, expected, "{} is in band {}", c.group.name, c.row);
        assert!(
            !c.group.name.starts_with("ENV"),
            "{} is on the Synth page",
            c.group.name
        );
    }
    assert_eq!(card("OSC A").columns, 5);
    assert_eq!(card("SUB").columns, 4);
    assert!(card("SUB").aside && card("NOISE").aside);
    assert_eq!(card("Filter 1").columns, 4);
    // The channel's two knobs ride on the Voice card: five cards did not
    // fit the second band beside the aside column.
    assert!(view.cards.iter().all(|c| c.group.name != "Channel"));
    let voice = card("Voice");
    assert!(voice.group.params.iter().any(|p| p.label == "VOLUME"));
    assert!(voice.group.params.iter().any(|p| p.label == "PAN"));
    // And the envelopes are in the inspector, with the LFOs.
    let modulation = session
        .flopsynth_inspecting(FlopsynthPage::Modulation, Some(0))
        .unwrap();
    assert!(
        modulation
            .cards
            .iter()
            .any(|c| c.group.name == "ENV 1 \u{b7} amp")
    );
    assert_eq!(large_of("Macros").len(), 0);
    let env = modulation
        .cards
        .iter()
        .find(|c| c.group.name == "ENV 1 \u{b7} amp")
        .unwrap();
    let decay = env
        .group
        .params
        .iter()
        .position(|p| p.label == "DECAY")
        .unwrap();
    assert_eq!(env.size_of(decay), KnobSize::Large);
}

/// §3.2: the scale is a setting — chosen once, kept — and the view carries
/// it so the layout multiplies by it. A scale the window does not offer is
/// refused rather than written.
#[test]
fn the_scale_is_a_setting_the_view_carries() {
    use fontelle_ui::canvas::FlopsynthPage;
    let dir = std::env::temp_dir().join(format!("fontelle-scale-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut session = common::a_session_for(fontelle_app::blank_project(8, 120.0, SR))
        .with_settings_path(dir.join("settings.json"));
    assert_eq!(session.flopsynth_scale(), 1.0);
    assert_eq!(session.flopsynth(FlopsynthPage::Synth).unwrap().scale, 1.0);
    session.set_flopsynth_scale(1.25);
    assert_eq!(session.flopsynth_scale(), 1.25);
    assert_eq!(session.flopsynth(FlopsynthPage::Synth).unwrap().scale, 1.25);
    let (saved, error) = fontelle_app::settings::Settings::load_from(&dir.join("settings.json"));
    assert!(error.is_none());
    assert_eq!(saved.flopsynth_scale_percent, 125, "kept for next time");
    session.set_flopsynth_scale(3.0);
    assert_eq!(
        session.flopsynth_scale(),
        1.25,
        "not a scale the window offers"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// §3.1 principle 12: words are design. Every caption on the window is a
/// word a player uses, from the caption file (`fontelle_app::captions`),
/// drawn in capitals — and the file has no word the window never uses.
/// The generic panel and the automation lanes keep the lowercase words:
/// the caption is what the bridge *draws*, the label is the parameter's
/// name.
///
/// The macros are their own names (a person's words, as typed) and an
/// effect card's captions are its effect's (the rack of §3.6 is where they
/// are redrawn), so those two are outside the file.
#[test]
fn every_caption_on_the_window_is_a_word_from_the_caption_file() {
    use fontelle_app::captions::{CAPTIONS, caption};
    use fontelle_types::EffectKind;
    use fontelle_ui::canvas::FlopsynthPage;
    let mut session = common::a_session_for(fontelle_app::blank_project(8, 120.0, SR));
    session.add_patch_effect(EffectKind::Chorus);
    let mut used: std::collections::BTreeSet<&'static str> = Default::default();
    for page in [
        FlopsynthPage::Synth,
        FlopsynthPage::Modulation,
        FlopsynthPage::Effects,
    ] {
        let view = session.flopsynth(page).expect("Flopsynth's window");
        for card in &view.cards {
            if card.group.name == "Macros" || card.group.name.starts_with("FX ") {
                continue;
            }
            for param in &card.group.params {
                if fontelle_ui::canvas::is_nameplate_control(param) {
                    continue;
                }
                let word = CAPTIONS
                    .iter()
                    .find(|(_, drawn)| *drawn == param.label)
                    .unwrap_or_else(|| {
                        panic!(
                            "{}'s {:?} ({}) is not a caption in the file",
                            card.group.name, param.label, param.address
                        )
                    });
                used.insert(word.1);
                assert_eq!(
                    param.label,
                    param.label.to_uppercase(),
                    "{} is drawn in capitals",
                    param.label
                );
                assert!(
                    param.label.chars().count() <= 9,
                    "{} is longer than a cell holds",
                    param.label
                );
            }
        }
    }
    // The Init patch and the Grand Piano between them show every source
    // kind but the user table; the words for a table oscillator's controls
    // are used on the sub. Whatever the two do not show, the file may still
    // hold — but a word no control anywhere maps to is a word to delete.
    for (word, drawn) in CAPTIONS {
        assert_eq!(caption(word), Some(*drawn));
        assert!(
            !drawn.is_empty() && *drawn == drawn.to_uppercase(),
            "{drawn} is not capitals"
        );
    }
    assert!(used.len() > 40, "{} captions used", used.len());
    // A word that is not in the file is drawn in capitals rather than
    // dropped, and says so.
    assert_eq!(caption("not a caption"), None);
    // The generic panel keeps the parameter's own name.
    let panel = session.instrument().expect("the panel");
    assert!(
        panel
            .groups
            .iter()
            .any(|g| g.params.iter().any(|p| p.label == "cutoff"))
    );
}

/// §3.3: Alt-click resets a knob to the **preset's** value, the menu offers
/// the **default** (the Init patch's) too, and a value can be **typed** in
/// the knob's own unit — read back through the same read-out the knob
/// shows, so "9 kHz" lands where the read-out says "9.00 kHz". A chooser
/// takes an option's name.
#[test]
fn a_knob_has_a_preset_value_a_default_and_takes_a_typed_value() {
    use fontelle_ui::canvas::{FlopsynthPage, Typed};
    let mut session = common::a_session_for(fontelle_app::blank_project(8, 120.0, SR));
    let cutoff = ParamAddress::new("patch/filter[0]/cutoff");
    let display = |session: &fontelle_app::Session, address: &ParamAddress| -> String {
        session
            .flopsynth(FlopsynthPage::Synth)
            .unwrap()
            .cards
            .iter()
            .flat_map(|c| c.group.params.iter())
            .find(|p| p.address == *address)
            .map(|p| p.display.clone())
            .expect("the cutoff is on the page")
    };
    let value = |session: &fontelle_app::Session, address: &ParamAddress| -> f32 {
        session
            .flopsynth(FlopsynthPage::Synth)
            .unwrap()
            .cards
            .iter()
            .flat_map(|c| c.group.params.iter())
            .find(|p| p.address == *address)
            .map(|p| p.value)
            .unwrap()
    };
    let loaded = value(&session, &cutoff);
    assert_eq!(
        display(&session, &cutoff),
        "9.00 kHz",
        "the Grand Piano's filter"
    );
    let preset = session
        .instrument_param_preset_value(&cutoff)
        .expect("the channel came from a preset");
    assert!((preset - loaded).abs() < 1e-4);
    let default = session
        .instrument_param_default_value(&cutoff)
        .expect("every address has a default");
    assert!(
        (default - loaded).abs() > 0.01,
        "the Init patch's cutoff is not the piano's"
    );

    // Turned, the preset's value is still the preset's; the default too.
    session.set_instrument_param(&cutoff, 0.2);
    assert!((session.instrument_param_preset_value(&cutoff).unwrap() - preset).abs() < 1e-4);
    assert!((session.instrument_param_default_value(&cutoff).unwrap() - default).abs() < 1e-4);

    // Typed in the knob's unit, with and without the unit, with a prefix.
    for text in ["9 kHz", "9000", "9k", "9.0 khz"] {
        let typed = fontelle_ui::canvas::parse_typed(text).unwrap();
        let normalised = session
            .instrument_param_from_typed(&cutoff, &typed)
            .unwrap_or_else(|| panic!("{text} did not read"));
        session.set_instrument_param(&cutoff, normalised);
        assert_eq!(display(&session, &cutoff), "9.00 kHz", "typed {text}");
    }
    // A percentage is a share of the travel whatever the unit.
    let typed = fontelle_ui::canvas::parse_typed("37%").unwrap();
    let normalised = session
        .instrument_param_from_typed(&cutoff, &typed)
        .unwrap();
    assert!((normalised - 0.37).abs() < 1e-4);
    // Out of range is the nearest end.
    let typed = fontelle_ui::canvas::parse_typed("900 kHz").unwrap();
    assert!(
        (session
            .instrument_param_from_typed(&cutoff, &typed)
            .unwrap()
            - 1.0)
            .abs()
            < 1e-3
    );
    // A unit the read-out does not speak reads as nothing.
    let typed = fontelle_ui::canvas::parse_typed("9 ms").unwrap();
    assert_eq!(session.instrument_param_from_typed(&cutoff, &typed), None);
    // A chooser takes an option's name, and its number as a fraction.
    let division = ParamAddress::new("patch/lfo[0]/division");
    let typed = Typed::fraction(0.125);
    let normalised = session
        .instrument_param_from_typed(&division, &typed)
        .expect("1/8 is a division");
    session.set_instrument_param(&division, normalised);
    let lfo = session
        .mod_sources()
        .iter()
        .position(|s| s == "LFO 1")
        .unwrap();
    let view = session
        .flopsynth_inspecting(FlopsynthPage::Modulation, Some(lfo))
        .unwrap();
    let param = view
        .cards
        .iter()
        .flat_map(|c| c.group.params.iter())
        .find(|p| p.address == division)
        .unwrap();
    assert_eq!(param.display, "1/8");
    let typed = fontelle_ui::canvas::parse_typed("1/16").unwrap();
    let normalised = session
        .instrument_param_from_typed(&division, &typed)
        .unwrap();
    session.set_instrument_param(&division, normalised);
    let lfo = session
        .mod_sources()
        .iter()
        .position(|s| s == "LFO 1")
        .unwrap();
    let view = session
        .flopsynth_inspecting(FlopsynthPage::Modulation, Some(lfo))
        .unwrap();
    let param = view
        .cards
        .iter()
        .flat_map(|c| c.group.params.iter())
        .find(|p| p.address == division)
        .unwrap();
    assert_eq!(param.display, "1/16");
    // A word that is an option's name, for a chooser of words.
    let mode = ParamAddress::new("patch/voice/mode");
    let normalised = session
        .instrument_param_from_text(&mode, "mono")
        .expect("Mono is a mode");
    session.set_instrument_param(&mode, normalised);
    let view = session.flopsynth(FlopsynthPage::Synth).unwrap();
    let param = view
        .cards
        .iter()
        .flat_map(|c| c.group.params.iter())
        .find(|p| p.address == mode)
        .unwrap();
    assert_eq!(param.display, "Mono");
}

/// §3.3: the choosers whose options are shapes carry a thumbnail per option
/// — a wavetable's first frame, an LFO wave's cycle — so the menu can draw
/// them. A chooser of words (the voice mode) carries none.
#[test]
fn the_table_and_the_lfo_wave_choosers_carry_a_thumbnail_per_option() {
    use fontelle_ui::canvas::FlopsynthPage;
    let session = a_flopsynth();
    let synth = session.flopsynth(FlopsynthPage::Synth).unwrap();
    let lfo = session
        .mod_sources()
        .iter()
        .position(|s| s == "LFO 1")
        .unwrap();
    let modulation = session
        .flopsynth_inspecting(FlopsynthPage::Modulation, Some(lfo))
        .unwrap();
    let options = |view: &fontelle_ui::canvas::FlopsynthView, address: &str| -> usize {
        view.cards
            .iter()
            .flat_map(|c| c.group.params.iter())
            .find(|p| p.address.as_str() == address)
            .map(|p| match &p.kind {
                fontelle_ui::canvas::ParamKind::Choice(o) => o.len(),
                _ => 0,
            })
            .unwrap_or_else(|| panic!("no {address}"))
    };
    let table = ParamAddress::new("patch/layer[0]/synth/table");
    let shapes = synth
        .thumbnails_for(&table)
        .expect("the table chooser has thumbnails");
    assert_eq!(
        shapes.len(),
        options(&synth, table.as_str()),
        "one per table"
    );
    for (index, shape) in shapes.iter().enumerate() {
        assert!(
            shape.len() >= 32,
            "table {index} is drawn from {} points",
            shape.len()
        );
        assert!(shape.iter().all(|s| (-1.0..=1.0).contains(s)));
        assert!(
            shape.iter().any(|s| s.abs() > 0.2),
            "table {index} is not flat"
        );
    }
    let wave = ParamAddress::new("patch/lfo[0]/wave");
    let shapes = modulation
        .thumbnails_for(&wave)
        .expect("the wave chooser has thumbnails");
    assert_eq!(shapes.len(), options(&modulation, wave.as_str()));
    let mode = ParamAddress::new("patch/voice/mode");
    assert!(
        synth.thumbnails_for(&mode).is_none(),
        "a chooser of words has no pictures"
    );
}

/// §3.4: the strip's sources are on every page, each with a thumbnail — an
/// envelope's curve, an LFO's cycle, a macro's value — and asking for a
/// source to be inspected puts its card in the view marked for the
/// drawer, on whatever page is showing.
#[test]
fn every_page_carries_the_sources_and_the_inspected_sources_card() {
    use fontelle_ui::canvas::{FlopsynthPage, INSPECTOR_ROW};
    let session = common::a_session_for(fontelle_app::blank_project(8, 120.0, SR));
    for page in FlopsynthPage::ALL {
        let view = session.flopsynth(page).expect("Flopsynth's window");
        assert!(
            view.sources.len() >= 20,
            "{page:?}: {} sources",
            view.sources.len()
        );
        assert_eq!(view.source_shapes.len(), view.sources.len());
        let env = view.sources.iter().position(|s| s == "ENV 1").unwrap();
        let lfo = view.sources.iter().position(|s| s == "LFO 1").unwrap();
        let velocity = view.sources.iter().position(|s| s == "Velocity").unwrap();
        assert!(
            view.source_shapes[env].len() >= 16,
            "{page:?}: the envelope has a curve"
        );
        assert!(view.source_shapes[env].iter().any(|s| *s > 0.5), "it rises");
        assert!(
            view.source_shapes[lfo].len() >= 16,
            "{page:?}: the LFO has a cycle"
        );
        assert!(
            view.source_shapes[velocity].is_empty(),
            "{page:?}: the velocity has no picture"
        );
        // And a family per source, index for index — the ink its badge
        // wears, which the host names because the window may not see the
        // source itself.
        use fontelle_ui::document::SourceFamily;
        assert_eq!(view.source_families.len(), view.sources.len());
        assert_eq!(view.source_families[env], SourceFamily::Envelope);
        assert_eq!(view.source_families[lfo], SourceFamily::Lfo);
        assert_eq!(view.source_families[velocity], SourceFamily::Note);
        let wheel = view.sources.iter().position(|s| s == "Wheel").unwrap();
        assert_eq!(view.source_families[wheel], SourceFamily::Performance);
        assert!(view.inspector.is_none());
        assert!(view.cards.iter().all(|c| c.row != INSPECTOR_ROW));
    }
    // Inspecting LFO 2 on the Synth page: its card comes along, marked.
    let lfo_2 = session
        .flopsynth(FlopsynthPage::Synth)
        .expect("Flopsynth's window")
        .sources
        .iter()
        .position(|s| s == "LFO 2")
        .expect("LFO 2 is a source");
    let synth = session
        .flopsynth_inspecting(FlopsynthPage::Synth, Some(lfo_2))
        .expect("Flopsynth's window");
    assert_eq!(synth.inspector, Some(lfo_2));
    let inspected: Vec<&str> = synth
        .cards
        .iter()
        .filter(|c| c.row == INSPECTOR_ROW)
        .map(|c| c.group.name.as_str())
        .collect();
    assert_eq!(inspected, ["LFO 2"]);
    assert!(
        synth.cards.iter().any(|c| c.group.name == "OSC A"),
        "the page's own cards stay"
    );
    // An envelope's card too, and a macro's is the macros' card; the
    // velocity has nothing to edit and no card comes.
    let envelope = session
        .flopsynth_inspecting(FlopsynthPage::Effects, Some(0))
        .unwrap();
    assert!(
        envelope
            .cards
            .iter()
            .any(|c| c.row == INSPECTOR_ROW && c.group.name.starts_with("ENV 1"))
    );
    // The first macro is the first source after the follower, whatever
    // the Grand Piano calls it.
    let sources = session.mod_sources();
    let m1 = sources.iter().position(|s| s == "Follow").unwrap() + 1;
    let macros = session
        .flopsynth_inspecting(FlopsynthPage::Synth, Some(m1))
        .unwrap();
    assert!(
        macros
            .cards
            .iter()
            .any(|c| c.row == INSPECTOR_ROW && c.group.name == "Macros")
    );
    let velocity = session
        .mod_sources()
        .iter()
        .position(|s| s == "Velocity")
        .unwrap();
    let none = session
        .flopsynth_inspecting(FlopsynthPage::Synth, Some(velocity))
        .unwrap();
    assert!(none.cards.iter().all(|c| c.row != INSPECTOR_ROW));
    assert_eq!(none.inspector, None, "nothing to inspect is not inspecting");
}

/// Every page holds every source's drawer (§3.4): the pages under the
/// Grand Piano, with each envelope, LFO and macro open in turn.
#[test]
fn every_page_fits_with_every_source_open_in_the_inspector() {
    use fontelle_ui::canvas::FlopsynthPage;
    // The project a studio opens on: the Grand Piano, nineteen routes.
    let session = common::a_session_for(fontelle_app::blank_project(8, 120.0, SR));
    let sources = session.mod_sources();
    for page in FlopsynthPage::ALL {
        for (index, name) in sources.iter().enumerate() {
            if !(name.starts_with("ENV") || name.starts_with("LFO") || name.starts_with('M')) {
                continue;
            }
            assert_page_fits_inspecting(&session, page, Some(index));
        }
    }
}

/// §3.4's editors, on the cards the inspector shows: an envelope's card
/// carries its LOOP chooser, and an LFO's its DRAW switch, GRID chooser
/// and READ chooser — the parameters `patch_params` gained for them —
/// captioned from the file like every other control.
#[test]
fn the_envelope_card_has_a_loop_and_the_lfo_card_a_draw_switch_a_grid_and_a_read() {
    use fontelle_ui::canvas::{FlopsynthPage, ParamKind};
    let session = a_flopsynth();
    let sources = session.mod_sources();
    let env = sources.iter().position(|s| s == "ENV 1").unwrap();
    let view = session
        .flopsynth_inspecting(FlopsynthPage::Synth, Some(env))
        .unwrap();
    let card = view
        .cards
        .iter()
        .find(|c| c.group.name.starts_with("ENV 1"))
        .expect("the envelope's card");
    let find = |card: &fontelle_ui::canvas::FlopsynthCard, tail: &str| {
        card.group
            .params
            .iter()
            .find(|p| p.address.as_str().ends_with(tail))
            .cloned()
            .unwrap_or_else(|| panic!("no {tail} on {}", card.group.name))
    };
    let loop_ = find(card, "/loop");
    assert_eq!(loop_.label, "LOOP");
    assert!(
        matches!(&loop_.kind, ParamKind::Choice(options) if options.len() >= 4 && options[0] == "off")
    );
    assert_eq!(loop_.display, "off");

    let lfo = sources.iter().position(|s| s == "LFO 1").unwrap();
    let view = session
        .flopsynth_inspecting(FlopsynthPage::Synth, Some(lfo))
        .unwrap();
    let card = view
        .cards
        .iter()
        .find(|c| c.group.name == "LFO 1")
        .expect("the LFO's card");
    let draw = find(card, "/draw");
    assert_eq!(draw.label, "DRAW");
    assert!(matches!(draw.kind, ParamKind::Switch));
    assert_eq!(draw.display, "off");
    let grid = find(card, "/grid");
    assert_eq!(grid.label, "GRID");
    assert!(
        matches!(&grid.kind, ParamKind::Choice(options) if options.contains(&"16".to_string()) && options[0] == "off")
    );
    let read = find(card, "/shape_mode");
    assert_eq!(read.label, "READ");
    assert!(matches!(&read.kind, ParamKind::Choice(options) if options == &["smooth", "step"]));
    // And every card still declares a size per control.
    assert_eq!(card.sizes.len(), card.group.params.len());
}

/// §7 step 7: the picture is held to the DSP by a test that renders both.
/// An envelope's bent attack on the card's picture follows
/// `fontelle_dsp::shape_progress`, the bend the generator plays; a drawn
/// LFO shape's picture follows `LfoShape::value`, which the voice will
/// read. Both through the host, from a real patch.
#[test]
fn the_editors_pictures_are_the_dsps_own_curves() {
    use fontelle_ui::canvas::{
        FlopsynthPage, FlopsynthPicture, env_corners, env_curve_points, lfo_shape_curve_points,
    };
    let mut session = a_flopsynth();
    let sources = session.mod_sources();
    let env = sources.iter().position(|s| s == "ENV 1").unwrap();
    // A bent attack, through the knob it has always had.
    let shape = ParamAddress::new("patch/env[0]/attack_shape");
    session.set_instrument_param(&shape, 0.9);
    session.set_instrument_param(&ParamAddress::new("patch/env[0]/attack"), 0.5);
    let bend = session.selected_patch().unwrap().envelopes[0].attack_shape;
    assert!(bend > 0.5, "{bend}");
    let view = session
        .flopsynth_inspecting(FlopsynthPage::Synth, Some(env))
        .unwrap();
    let card = view
        .cards
        .iter()
        .find(|c| c.group.name.starts_with("ENV 1"))
        .unwrap();
    let FlopsynthPicture::Envelope(pic) = &card.picture else {
        panic!("an envelope's picture");
    };
    assert!((pic.attack_shape - bend).abs() < 1e-6);
    let rect = fontelle_ui::layout::Rect::new(0.0, 0.0, 600.0, 120.0);
    let corners = env_corners(rect, pic);
    let points = env_curve_points(rect, pic);
    // Every drawn point of the attack is on the generator's curve.
    let (x0, x1) = (corners.delay_end.0, corners.attack_end.0);
    for (x, y) in points
        .iter()
        .filter(|(x, _)| *x > x0 + 0.5 && *x < x1 - 0.5)
    {
        let t = (x - x0) / (x1 - x0);
        let level = fontelle_dsp::shape_progress(t, bend);
        let expected = rect.bottom() - rect.height * level;
        assert!((y - expected).abs() < 0.5, "at {t}: {y} vs {expected}");
    }

    // A drawn shape: DRAW on, then a factory shape written through the
    // host, and the picture is that shape's own value.
    let lfo = sources.iter().position(|s| s == "LFO 1").unwrap();
    session.set_instrument_param(&ParamAddress::new("patch/lfo[0]/draw"), 1.0);
    let (_, bounce) = fontelle_types::LfoShape::presets()
        .into_iter()
        .find(|(name, _)| *name == "Bounce")
        .unwrap();
    session.set_lfo_shape(0, bounce.clone());
    session.end_gesture();
    let view = session
        .flopsynth_inspecting(FlopsynthPage::Synth, Some(lfo))
        .unwrap();
    let card = view.cards.iter().find(|c| c.group.name == "LFO 1").unwrap();
    let FlopsynthPicture::LfoShape { shape, .. } = &card.picture else {
        panic!("a drawn shape's picture, not {:?}", card.picture);
    };
    assert_eq!(*shape, bounce);
    for (x, y) in lfo_shape_curve_points(rect, shape) {
        let phase = (x / rect.width).min(0.999_99);
        let expected = rect.y + rect.height * (0.5 - bounce.value(phase) * 0.5);
        assert!((y - expected).abs() < 0.6, "at {phase}: {y} vs {expected}");
    }
    // And the badge's thumbnail is the drawn shape too.
    let thumb = &view.source_shapes[lfo];
    assert!((thumb[0] - bounce.value(0.0)).abs() < 1e-5);
    // The shape's wave, sampled, is what the shapes menu starts over from;
    // a drag's writes coalesce into one undo, broken by the release.
    let from_wave = session.lfo_wave_shape(0).unwrap();
    assert_eq!(
        from_wave.points.len(),
        fontelle_core::patch_params::DRAWN_POINTS
    );
    let depth = session.undo_depth();
    let mut moved = bounce.clone();
    moved.points[1].y = 0.5;
    session.set_lfo_shape(0, moved.clone());
    moved.points[1].y = 0.2;
    session.set_lfo_shape(0, moved.clone());
    session.end_gesture();
    assert_eq!(session.undo_depth(), depth + 1, "one drag, one undo");
    session.undo();
    assert_eq!(
        session.selected_patch().unwrap().lfos[0].shape,
        Some(bounce)
    );
}

/// The generators of `docs/flopsynth-next.md` §4.2 are on the strip with
/// the LFOs — two sequencers, the chaos, the walk, the follower — each
/// with the ink of its family and a thumbnail of what it does, and each
/// but the follower opens an editor in the inspector: a sequencer's is its
/// sixteen steps as bars, dragged; the chaos's and the walk's a trace and
/// their knobs.
#[test]
fn the_generators_are_on_the_strip_and_edited_in_the_inspector() {
    use fontelle_ui::canvas::{FlopsynthPage, FlopsynthPicture, INSPECTOR_ROW};
    use fontelle_ui::document::SourceFamily;
    let mut session = a_flopsynth();
    session.set_instrument_param(&ParamAddress::new("patch/seq[0]/step[2]"), 1.0);
    session.set_instrument_param(&ParamAddress::new("patch/seq[0]/length"), 0.4);
    let view = session.flopsynth(FlopsynthPage::Synth).expect("a window");
    let at = |name: &str| {
        view.sources
            .iter()
            .position(|s| s == name)
            .unwrap_or_else(|| panic!("no {name} on the strip: {:?}", view.sources))
    };
    // After the last LFO, before the note sources.
    let last_lfo = view
        .sources
        .iter()
        .rposition(|s| s.starts_with("LFO "))
        .unwrap();
    assert_eq!(at("SEQ 1"), last_lfo + 1);
    assert_eq!(at("SEQ 2"), last_lfo + 2);
    assert_eq!(at("Chaos"), last_lfo + 3);
    assert_eq!(at("Walk"), last_lfo + 4);
    assert_eq!(at("Follow"), last_lfo + 5);
    for name in ["SEQ 1", "SEQ 2", "Chaos", "Walk"] {
        assert_eq!(view.source_families[at(name)], SourceFamily::Lfo, "{name}");
        assert!(
            view.source_shapes[at(name)].len() >= 16,
            "{name} has a picture on its badge"
        );
    }
    assert_eq!(view.source_families[at("Follow")], SourceFamily::Envelope);
    // The sequencer's thumbnail is its steps: step 2 up, the rest at rest,
    // over the seven that play.
    let seq = &view.source_shapes[at("SEQ 1")];
    let point_of = |step: usize| step * seq.len() / 7 + 1;
    assert!(seq[point_of(2)] > 0.9, "step 2 is up: {seq:?}");
    assert!(seq[point_of(1)].abs() < 0.1, "step 1 is at rest: {seq:?}");

    // The inspector: a sequencer's card is its steps and its clock.
    let inspecting = |source: &str| {
        session
            .flopsynth_inspecting(FlopsynthPage::Synth, Some(at(source)))
            .expect("a window")
            .cards
            .into_iter()
            .find(|c| c.row == INSPECTOR_ROW)
    };
    let seq = inspecting("SEQ 1").expect("a card for the sequencer");
    assert_eq!(seq.group.name, "SEQ 1");
    let FlopsynthPicture::Steps {
        sequencer,
        steps,
        length,
        ..
    } = &seq.picture
    else {
        panic!("a sequencer's picture is its steps: {:?}", seq.picture);
    };
    assert_eq!(*sequencer, 0);
    assert_eq!(steps.len(), 16);
    assert_eq!(*length, 7);
    assert!((steps[2] - 1.0).abs() < 1e-6 && steps[1].abs() < 1e-6);
    let addresses: Vec<&str> = seq
        .group
        .params
        .iter()
        .map(|p| p.address.as_str())
        .collect();
    for tail in ["length", "sync", "rate", "division", "smooth"] {
        assert!(
            addresses.contains(&format!("patch/seq[0]/{tail}").as_str()),
            "no {tail}: {addresses:?}"
        );
    }
    assert!(
        !addresses.iter().any(|a| a.contains("step[")),
        "the steps are the picture, not sixteen knobs"
    );
    let chaos = inspecting("Chaos").expect("a card for the chaos");
    assert!(matches!(chaos.picture, FlopsynthPicture::Curve { .. }));
    assert_eq!(chaos.group.params.len(), 1);
    let walk = inspecting("Walk").expect("a card for the walk");
    assert!(matches!(walk.picture, FlopsynthPicture::Curve { .. }));
    assert_eq!(walk.group.params.len(), 2);
    assert!(
        inspecting("Follow").is_none(),
        "the follower has nothing to edit"
    );
    // And none of them is on a page: the strip is where they live.
    for page in FlopsynthPage::ALL {
        let view = session.flopsynth(page).unwrap();
        assert!(
            !view
                .cards
                .iter()
                .any(|c| ["SEQ 1", "SEQ 2", "Chaos", "Walk"].contains(&c.group.name.as_str())),
            "{page:?} carries a generator's card"
        );
    }
}

/// The Voice card's picture is the velocity curve (§3.5, §4.2): the gain
/// over velocity, with the four custom points as marks — the curve the
/// voice plays, read off `velocity_gain` so the picture cannot lie.
///
/// Ignored until the window's size is decided (§9.5): at 1180×840 the
/// card has no room for a picture over its four rows — see `picture_for`.
#[test]
#[ignore = "waits on the window's design size (docs/flopsynth-next.md §9.5)"]
fn the_voice_card_draws_the_velocity_curve() {
    use fontelle_core::{VelocityCurve, velocity_gain};
    use fontelle_ui::canvas::{FlopsynthPage, FlopsynthPicture};
    let mut session = a_flopsynth();
    let picture = |session: &fontelle_app::Session| {
        session
            .flopsynth(FlopsynthPage::Synth)
            .unwrap()
            .cards
            .into_iter()
            .find(|c| c.group.name == "Voice")
            .expect("a Voice card")
            .picture
    };
    let FlopsynthPicture::Curve { points, marks, .. } = picture(&session) else {
        panic!("the Voice card's picture is a curve");
    };
    assert!(points.len() >= 32);
    let at = |v: u8| points[(usize::from(v) * (points.len() - 1)) / 127];
    assert!((at(127) - 1.0).abs() < 0.02 && at(0) < 0.02);
    assert!(
        (at(64) - velocity_gain(64, VelocityCurve::Square)).abs() < 0.03,
        "the square: {}",
        at(64)
    );
    assert_eq!(marks.len(), 4, "the four points, wherever the curve is");
    assert!((marks[3].0 - 1.0).abs() < 1e-6 && (marks[1].0 - 64.0 / 127.0).abs() < 0.01);
    // Choose linear: the middle rises to a half.
    session.set_instrument_param(&ParamAddress::new("patch/voice/velocity_curve"), 0.0);
    let FlopsynthPicture::Curve { points, .. } = picture(&session) else {
        unreachable!()
    };
    let at = |v: u8| points[(usize::from(v) * (points.len() - 1)) / 127];
    assert!((at(64) - 0.504).abs() < 0.03, "linear: {}", at(64));
    // A point dragged: the curve is custom and passes through it.
    session.set_instrument_param(&ParamAddress::new("patch/voice/velocity_point[1]"), 0.9);
    let FlopsynthPicture::Curve { points, marks, .. } = picture(&session) else {
        unreachable!()
    };
    let at = |v: u8| points[(usize::from(v) * (points.len() - 1)) / 127];
    assert!(
        (at(64) - 0.9).abs() < 0.03,
        "custom through 0.9: {}",
        at(64)
    );
    assert!((marks[1].1 - 0.9).abs() < 1e-6);
}

/// Every source on the strip has a short name for a narrow badge (phase
/// 3), index for index with `sources`: the envelopes and the LFOs by
/// letter and number, a macro by its first letters, the rest by a
/// three-letter word.
#[test]
fn every_source_has_a_short_name_for_a_narrow_badge() {
    use fontelle_ui::canvas::FlopsynthPage;
    let session = common::a_session_for(fontelle_app::blank_project(8, 120.0, SR));
    let view = session.flopsynth(FlopsynthPage::Synth).unwrap();
    assert_eq!(view.source_short.len(), view.sources.len());
    let short = |name: &str| {
        let at = view.sources.iter().position(|s| s == name).unwrap();
        view.source_short[at].as_str()
    };
    assert_eq!(short("ENV 1"), "E1");
    assert_eq!(short("LFO 8"), "L8");
    assert_eq!(short("SEQ 2"), "S2");
    assert_eq!(short("Chaos"), "CHS");
    assert_eq!(short("Walk"), "WLK");
    assert_eq!(short("Follow"), "FLW");
    assert_eq!(short("Velocity"), "VEL");
    assert_eq!(short("Aftertouch"), "AT");
    assert_eq!(short("Note X"), "X");
    // A named macro: its first three letters; an unnamed one its number.
    let brightness = view.sources.iter().position(|s| s == "Brightness").unwrap();
    assert_eq!(view.source_short[brightness], "BRI");
    let m5 = view.sources.iter().position(|s| s == "M5").unwrap();
    assert_eq!(view.source_short[m5], "M5");
    for short in &view.source_short {
        assert!(!short.is_empty() && short.chars().count() <= 3, "{short}");
    }
}

/// The seven of §4.5 in a patch's chain: each takes a slot and draws a
/// card, and the five with a shape to draw have a picture — the folder's
/// transfer curve, the phaser's and the flanger's combs at the sweep's
/// middle, the multiband's three drives across the band, hyper's copies
/// as marks at their detunes. The shifter and width have nothing a
/// picture could say.
#[test]
fn the_seven_new_effects_take_a_slot_and_draw_their_pictures() {
    use fontelle_types::EffectKind;
    use fontelle_ui::canvas::{FlopsynthPage, FlopsynthPicture, FlopsynthShowing};
    let kinds = [
        EffectKind::Fold,
        EffectKind::Phaser,
        EffectKind::Flanger,
        EffectKind::Multiband,
        EffectKind::Hyper,
        EffectKind::Shifter,
        EffectKind::Width,
    ];
    let mut session = a_flopsynth();
    for kind in kinds {
        assert!(
            fontelle_core::flopsynth::PATCH_FX_KINDS.contains(&kind),
            "{kind:?} is not offered to a patch"
        );
        session.add_patch_effect(kind);
    }
    let patch = session.selected_patch().unwrap();
    assert_eq!(patch.fx.len(), kinds.len());
    let card_of = |slot: usize| {
        let view = session
            .flopsynth_showing(
                FlopsynthPage::Effects,
                FlopsynthShowing {
                    inspector: None,
                    fx_slot: Some(slot),
                    wave_tool: Default::default(),
                },
            )
            .unwrap();
        assert_eq!(view.cards.len(), 1);
        view.cards[0].clone()
    };
    for (slot, kind) in kinds.iter().enumerate() {
        let card = card_of(slot);
        assert_eq!(
            card.group.name,
            format!("FX {} \u{b7} {}", slot + 1, kind.label())
        );
        assert!(!card.group.params.is_empty(), "{kind:?} draws no controls");
    }
    // The folder at rest is a wire: a diagonal. Driven, it folds — the
    // curve comes back down past full scale.
    let fold = card_of(0);
    let FlopsynthPicture::Curve {
        points, midline, ..
    } = &fold.picture
    else {
        panic!("the folder's transfer curve: {:?}", fold.picture);
    };
    assert!(*midline && points.len() >= 32);
    assert!(
        points[0] < 0.1 && points[points.len() - 1] > 0.9,
        "a wire: {points:?}"
    );
    let phaser = card_of(1);
    let FlopsynthPicture::Curve { points, .. } = &phaser.picture else {
        panic!("the phaser's comb: {:?}", phaser.picture);
    };
    let (low, high) = points
        .iter()
        .fold((1.0f32, 0.0f32), |(l, h), p| (l.min(*p), h.max(*p)));
    assert!(
        low < 0.2 && high > 0.8,
        "notches and peaks: {low} .. {high}"
    );
    let flanger = card_of(2);
    let FlopsynthPicture::Curve { points, .. } = &flanger.picture else {
        panic!("the flanger's comb: {:?}", flanger.picture);
    };
    let (low, high) = points
        .iter()
        .fold((1.0f32, 0.0f32), |(l, h), p| (l.min(*p), h.max(*p)));
    assert!(
        low < 0.2 && high > 0.8,
        "notches and peaks: {low} .. {high}"
    );
    let multiband = card_of(3);
    let FlopsynthPicture::Curve { points, marks, .. } = &multiband.picture else {
        panic!("the multiband's bands: {:?}", multiband.picture);
    };
    assert!(
        points.len() >= 32 && marks.len() == 2,
        "three bands, two crossovers"
    );
    let hyper = card_of(4);
    let FlopsynthPicture::Curve { marks, .. } = &hyper.picture else {
        panic!("hyper's copies: {:?}", hyper.picture);
    };
    assert_eq!(marks.len(), 2, "a mark per copy");
    assert!(matches!(card_of(5).picture, FlopsynthPicture::None));
    assert!(matches!(card_of(6).picture, FlopsynthPicture::None));
}

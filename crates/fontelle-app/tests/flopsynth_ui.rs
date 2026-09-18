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

use fontelle_types::InstrumentKind;
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
            "LFO 1",
            "LFO 2",
            "LFO 3",
            "LFO 4",
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
    let mut session = a_flopsynth();
    let controls: Vec<(fontelle_types::ParamAddress, ParamKind)> = session
        .instrument()
        .expect("a panel")
        .groups
        .iter()
        .flat_map(|g| g.params.iter())
        .filter(|p| p.address.as_str().starts_with("patch/"))
        .map(|p| (p.address.clone(), p.kind.clone()))
        .collect();

    for (address, kind) in controls {
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
    assert_eq!(macros.params.len(), 4);
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

    // Ladder is the second of the four models.
    let model = fontelle_types::ParamAddress::new("patch/filter[0]/model");
    session.set_instrument_param(&model, 1.0 / 3.0);
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

    // The LFOs are on the page that is about modulation (§8.4), not on the one
    // about making the sound — which is what the page split is for.
    let modulation = session
        .flopsynth(fontelle_ui::canvas::FlopsynthPage::Modulation)
        .expect("a Flopsynth channel has every page");
    let moved: Vec<&str> = modulation
        .cards
        .iter()
        .map(|c| c.group.name.as_str())
        .collect();
    assert!(moved.contains(&"LFO 1"), "{moved:?}");
    assert!(!names.contains(&"LFO 1"), "the Synth page has no LFOs");
    assert!(
        !modulation.sources.is_empty(),
        "the Modulation page lists the sources a badge can be dragged from"
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
    // The envelopes are on the Modulation page since the Synth page became
    // two bands, and the picture is the same picture.
    let env = modulation
        .cards
        .iter()
        .find(|c| c.group.name == "ENV 1 \u{b7} amp")
        .map(|c| c.picture.clone())
        .expect("the Modulation page draws the envelopes");
    assert!(matches!(env, FlopsynthPicture::Envelope { .. }));
    // The LFO's picture is on its own page, and it is the same picture.
    let lfo = modulation
        .cards
        .iter()
        .find(|c| c.group.name == "LFO 1")
        .map(|c| c.picture.clone())
        .expect("the Modulation page draws the LFOs");
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
    assert_eq!(full.cards.len(), fontelle_core::MAX_PATCH_FX);
    assert!(!full.fx_room);
    session.add_patch_effect(EffectKind::Delay);
    assert_eq!(
        session
            .flopsynth(FlopsynthPage::Effects)
            .unwrap()
            .cards
            .len(),
        fontelle_core::MAX_PATCH_FX
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
    let names = |session: &fontelle_app::Session| -> Vec<String> {
        session
            .flopsynth(FlopsynthPage::Effects)
            .unwrap()
            .cards
            .iter()
            .map(|c| c.group.name.clone())
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
    let view = session.flopsynth(page).expect("Flopsynth's window");
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
    let mut cards_bottom = body.y;
    for (index, placed) in layout.cards.iter().enumerate() {
        let name = &view.cards[index].group.name;
        assert!(!placed.frame.is_empty(), "{page:?}: {name} was not drawn");
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
    assert_eq!(view.cards.len(), 4, "four slots, four cards");
    assert_page_fits(&session, fontelle_ui::canvas::FlopsynthPage::Effects);
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
        assert!(
            placed.frame.bottom() <= body.bottom() + 0.01,
            "{} runs off the window at the minimum size",
            view.cards[index].group.name
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
    // And the envelopes are on the Modulation page, with the LFOs, until
    // the inspector takes them.
    let modulation = session.flopsynth(FlopsynthPage::Modulation).unwrap();
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

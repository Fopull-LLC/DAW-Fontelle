//! The three kinds of oscillator on Flopsynth's window: what each card shows.
//!
//! A card's controls are what its source *has* — a table its chooser and
//! frame, a recording its loop and start, a string its stiffness, damping,
//! strike and decay — and its picture is what that source *is*: one cycle,
//! the recording's shape, the partials where the string puts them. The
//! `kind` chooser is what turns one into another, and it is on every card
//! but the noise's.
//!
//! One of these is a report as well as a design: an oscillator reading a
//! dropped **wavetable** drew the noise layer's colour knob in place of its
//! own position, because the card decided what to draw by whether the source
//! was a *bank* table.

mod common;

use fontelle_dsp::SynthSource;
use fontelle_types::{InstrumentKind, ParamAddress};
use fontelle_ui::canvas::{FlopsynthPage, FlopsynthPicture, ParamKind};
use fontelle_ui::document::StudioHost;

use common::SR;

/// The kind chooser's positions, normalised: four kinds since phase 4.
const SAMPLE_KIND: f32 = 1.0 / 3.0;
const STRING_KIND: f32 = 2.0 / 3.0;

fn a_flopsynth() -> fontelle_app::Session {
    let mut session = common::a_session_for(common::a_project_with_a_clip(8, 120.0, SR));
    session.set_channel_kind(0, InstrumentKind::Osc3);
    session.set_channel_kind(0, InstrumentKind::Flopsynth);
    session
}

fn set(session: &mut fontelle_app::Session, address: &str, value: f32) {
    session.set_instrument_param(&ParamAddress::new(address), value);
}

fn card(session: &fontelle_app::Session, name: &str) -> fontelle_ui::canvas::FlopsynthCard {
    session
        .flopsynth(FlopsynthPage::Synth)
        .expect("a window")
        .cards
        .into_iter()
        .find(|c| c.group.name == name)
        .unwrap_or_else(|| panic!("no {name} card"))
}

fn captions(card: &fontelle_ui::canvas::FlopsynthCard) -> Vec<(String, String)> {
    card.group
        .params
        .iter()
        .map(|p| (p.address.as_str().to_string(), p.label.clone()))
        .collect()
}

fn has(captions: &[(String, String)], address: &str) -> bool {
    captions.iter().any(|(a, _)| a == address)
}

fn caption_of<'a>(captions: &'a [(String, String)], address: &str) -> &'a str {
    captions
        .iter()
        .find(|(a, _)| a == address)
        .map(|(_, c)| c.as_str())
        .unwrap_or_else(|| panic!("{address} is not on the card"))
}

#[test]
fn a_table_card_has_a_kind_chooser_a_table_and_a_frame() {
    let session = a_flopsynth();
    let osc = card(&session, "OSC A");
    let list = captions(&osc);
    assert!(has(&list, "patch/layer[0]/synth/kind"));
    assert!(has(&list, "patch/layer[0]/synth/table"));
    assert_eq!(caption_of(&list, "patch/layer[0]/synth/position"), "WT POS");
    assert!(!has(&list, "patch/layer[0]/synth/sample/loop"));
    assert!(!has(&list, "patch/layer[0]/synth/string/stiffness"));
    let kind = osc
        .group
        .params
        .iter()
        .find(|p| p.address.as_str() == "patch/layer[0]/synth/kind")
        .unwrap();
    assert_eq!(kind.display, "Table");
    assert!(
        matches!(&kind.kind, ParamKind::Choice(options) if options == &["Table", "Sample", "String", "Spectral"])
    );
    assert!(matches!(osc.picture, FlopsynthPicture::Wave { .. }));
    // The kind chooser comes first: it decides what the rest of the card is.
    assert_eq!(list[0].0, "patch/layer[0]/synth/kind");
}

#[test]
fn a_sample_card_has_its_loop_and_start_and_draws_the_recording() {
    let mut session = a_flopsynth();
    let dir = std::env::temp_dir().join(format!("fontelle-srcui-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let samples: Vec<f32> = (0..24_000)
        .map(|i| (std::f32::consts::TAU * 440.0 * i as f32 / 48_000.0).sin() * 0.8)
        .collect();
    let path = dir.join("Rhodes A4.wav");
    std::fs::write(
        &path,
        fontelle_assets::fixtures::build_wav(48_000, 1, &samples),
    )
    .unwrap();
    session.load_sample(1, &path).expect("loads");

    let osc = card(&session, "OSC B");
    let list = captions(&osc);
    assert!(
        !has(&list, "patch/layer[1]/synth/table"),
        "a recording has no table chooser"
    );
    assert_eq!(caption_of(&list, "patch/layer[1]/synth/position"), "START");
    assert_eq!(
        caption_of(&list, "patch/layer[1]/synth/sample/loop"),
        "LOOP"
    );
    assert_eq!(
        caption_of(&list, "patch/layer[1]/synth/sample/loop_start"),
        "LOOP IN"
    );
    assert_eq!(
        caption_of(&list, "patch/layer[1]/synth/sample/loop_end"),
        "LOOP OUT"
    );
    let kind = osc
        .group
        .params
        .iter()
        .find(|p| p.address.as_str() == "patch/layer[1]/synth/kind")
        .unwrap();
    assert_eq!(kind.display, "Sample");
    match &osc.picture {
        FlopsynthPicture::Sound {
            peaks,
            start,
            loop_region,
            name,
        } => {
            assert_eq!(name, "Rhodes A4");
            assert!(
                peaks.len() >= 64,
                "an overview of the recording: {} columns",
                peaks.len()
            );
            assert!(peaks.iter().all(|(low, high)| low <= high));
            assert!(
                peaks.iter().any(|(_, high)| *high > 0.5),
                "the tone is in it"
            );
            assert_eq!(*start, 0.0);
            assert!(
                loop_region.is_none(),
                "the loop is off until it is turned on"
            );
        }
        other => panic!("expected the recording's picture, got {other:?}"),
    }
    // Turn the loop on (the second of the five ways of reading) and the
    // picture shows the region.
    set(&mut session, "patch/layer[1]/synth/sample/loop", 0.25);
    set(&mut session, "patch/layer[1]/synth/position", 0.25);
    match card(&session, "OSC B").picture {
        FlopsynthPicture::Sound {
            start, loop_region, ..
        } => {
            assert!((start - 0.25).abs() < 1e-3);
            let (a, b) = loop_region.expect("the loop shows once it is on");
            assert!(a < b);
        }
        other => panic!("{other:?}"),
    }
    // Another oscillator switched to Sample plays the patch's first
    // recording until one is dropped on it: layering the same sound twice
    // with different settings is the point of having three oscillators.
    set(&mut session, "patch/layer[2]/synth/kind", SAMPLE_KIND);
    match card(&session, "OSC C").picture {
        FlopsynthPicture::Sound { name, .. } => assert_eq!(name, "Rhodes A4"),
        other => panic!("{other:?}"),
    }
    // And a drop on it then is a recording of its own, not a replacement
    // of the one OSC B is still playing.
    session.load_sample(2, &path).expect("loads");
    let patch = session.selected_patch().unwrap();
    assert_eq!(patch.samples.len(), 2);
    std::fs::remove_dir_all(&dir).ok();

    // A sample oscillator on a patch with nothing dropped yet says so rather
    // than drawing nothing.
    let mut fresh = a_flopsynth();
    set(&mut fresh, "patch/layer[2]/synth/kind", SAMPLE_KIND);
    match card(&fresh, "OSC C").picture {
        FlopsynthPicture::Sound { peaks, name, .. } => {
            assert!(peaks.is_empty());
            assert!(
                name.contains("drop"),
                "an empty sample card asks for a sound: {name}"
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_string_card_has_its_string_and_draws_the_partials() {
    let mut session = a_flopsynth();
    set(&mut session, "patch/layer[0]/synth/kind", STRING_KIND);
    let patch = session.selected_patch().unwrap();
    assert!(matches!(
        &patch.layers[0].source,
        fontelle_core::Source::Synth(osc) if osc.source == SynthSource::String
    ));
    let osc = card(&session, "OSC A");
    let list = captions(&osc);
    assert!(!has(&list, "patch/layer[0]/synth/table"));
    assert!(!has(&list, "patch/layer[0]/synth/sample/loop"));
    assert_eq!(caption_of(&list, "patch/layer[0]/synth/position"), "BRIGHT");
    assert_eq!(
        caption_of(&list, "patch/layer[0]/synth/string/stiffness"),
        "STIFF"
    );
    assert_eq!(
        caption_of(&list, "patch/layer[0]/synth/string/damping"),
        "DAMP"
    );
    assert_eq!(
        caption_of(&list, "patch/layer[0]/synth/string/strike"),
        "STRIKE"
    );
    assert_eq!(
        caption_of(&list, "patch/layer[0]/synth/string/decay"),
        "RING"
    );
    match &osc.picture {
        FlopsynthPicture::Partials { bars, harmonics } => {
            assert!(bars.len() >= 16, "{} partials drawn", bars.len());
            assert!(*harmonics >= 16);
            // The bars sit at the string's own frequencies: sharp of the
            // harmonic grid, more so going up — which is the picture saying
            // what the source is.
            let (x1, _) = bars[0];
            let (x8, h8) = bars[7];
            assert!(
                x8 > 8.0 * x1 * 1.005,
                "the eighth partial is stretched: {x8} against {}",
                8.0 * x1
            );
            assert!(h8 > 0.0 && h8 <= 1.0);
            assert!(bars[0].1 > bars[7].1, "the fundamental is the tallest");
        }
        other => panic!("expected the partials, got {other:?}"),
    }
    // Stiffer is more stretched; brighter is taller up top.
    let eighth = |session: &fontelle_app::Session| match card(session, "OSC A").picture {
        FlopsynthPicture::Partials { bars, .. } => bars[7],
        other => panic!("{other:?}"),
    };
    let (x_before, h_before) = eighth(&session);
    set(&mut session, "patch/layer[0]/synth/string/stiffness", 0.9);
    let (x_after, _) = eighth(&session);
    assert!(x_after > x_before);
    set(&mut session, "patch/layer[0]/synth/position", 1.0);
    let (_, h_after) = eighth(&session);
    assert!(h_after > h_before);
}

/// The report: a dropped wavetable's card drew "colour", the noise layer's
/// knob, where its position belonged.
#[test]
fn a_dropped_wavetables_card_has_a_position_not_a_colour() {
    let mut session = a_flopsynth();
    let dir = std::env::temp_dir().join(format!("fontelle-srcui-wt-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let table: Vec<f32> = (0..8_192)
        .map(|i| (i % 2_048) as f32 / 1_024.0 - 1.0)
        .collect();
    let path = dir.join("Pad.wav");
    std::fs::write(
        &path,
        fontelle_assets::fixtures::build_wav(48_000, 1, &table),
    )
    .unwrap();
    session.load_wavetable(0, &path).expect("loads");
    let osc = card(&session, "OSC A");
    let list = captions(&osc);
    assert!(has(&list, "patch/layer[0]/synth/position"));
    assert!(!has(&list, "patch/layer[0]/synth/noise_colour"));
    assert!(
        !has(&list, "patch/layer[0]/synth/table"),
        "a dropped table is not in the bank's list"
    );
    assert!(has(&list, "patch/layer[0]/synth/kind"));
    std::fs::remove_dir_all(&dir).ok();
}

/// Switching kinds from the window: every card's controls are addresses the
/// patch answers, whichever kind it is on.
#[test]
fn every_control_on_every_kind_of_card_is_readable() {
    let mut session = a_flopsynth();
    for kind in [0.0f32, SAMPLE_KIND, STRING_KIND, 1.0] {
        set(&mut session, "patch/layer[0]/synth/kind", kind);
        let patch = session.selected_patch().unwrap();
        let addresses = fontelle_core::flopsynth::addresses(&patch);
        let osc = card(&session, "OSC A");
        for control in &osc.group.params {
            let address = control.address.as_str();
            if !address.starts_with("patch/") {
                continue;
            }
            assert!(
                addresses.contains(&address.to_string()),
                "{address} is on the card at kind {kind} and not in `addresses`"
            );
            assert!(
                fontelle_core::patch_params::value(&patch, address).is_some(),
                "{address} cannot be read at kind {kind}"
            );
        }
    }
    // And the noise card has no kind chooser.
    let noise = card(&session, "NOISE");
    assert!(
        !captions(&noise)
            .iter()
            .any(|(a, _)| a == "patch/layer[4]/synth/kind")
    );
}

/// The grain cloud's own knobs stand where the loop points stood — a cloud
/// has no loop — and a recording with more than one zone gets a chooser
/// that can lock the oscillator to one of them.
#[test]
fn a_grain_card_swaps_the_loop_points_for_grain_and_spray_and_a_kit_lists_its_hits() {
    use fontelle_core::factory_samples::FactorySampleSet;
    let mut session = a_flopsynth();
    session
        .load_factory_sample(1, FactorySampleSet::KitStudio)
        .expect("the kit loads");
    let osc = card(&session, "OSC B");
    let list = captions(&osc);
    assert_eq!(
        caption_of(&list, "patch/layer[1]/synth/sample/loop_start"),
        "LOOP IN"
    );
    assert!(!has(&list, "patch/layer[1]/synth/sample/grain"));
    assert!(!has(&list, "patch/layer[1]/synth/sample/spray"));
    // The zone chooser: *any*, then every hit by the name the roll gives it.
    let zone = osc
        .group
        .params
        .iter()
        .find(|p| p.address.as_str() == "patch/layer[1]/synth/sample/zone")
        .expect("a kit's card has a zone chooser");
    assert_eq!(zone.label, "ZONE");
    assert_eq!(zone.display, "any");
    let ParamKind::Choice(options) = &zone.kind else {
        panic!("the zone is a chooser");
    };
    assert_eq!(options.len(), 37);
    assert_eq!(options[0], "any");
    assert_eq!(options[1], "Kick 2");
    assert_eq!(options[4], "Snare");
    set(&mut session, "patch/layer[1]/synth/sample/zone", 4.0 / 36.0);
    let osc = card(&session, "OSC B");
    let zone = osc
        .group
        .params
        .iter()
        .find(|p| p.address.as_str() == "patch/layer[1]/synth/sample/zone")
        .unwrap();
    assert_eq!(zone.display, "Snare");
    // A recording of one zone has nothing to choose between.
    let dir = std::env::temp_dir().join(format!("fontelle-grainui-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let samples: Vec<f32> = (0..24_000)
        .map(|i| (std::f32::consts::TAU * 440.0 * i as f32 / 48_000.0).sin() * 0.8)
        .collect();
    let path = dir.join("Tone A4.wav");
    std::fs::write(
        &path,
        fontelle_assets::fixtures::build_wav(48_000, 1, &samples),
    )
    .unwrap();
    session.load_sample(2, &path).expect("loads");
    let list = captions(&card(&session, "OSC C"));
    assert!(
        !has(&list, "patch/layer[2]/synth/sample/zone"),
        "one zone is no choice"
    );

    // Grains: the last way of reading. The loop points go, the grain and
    // the spray come, and the picture shows where the grains may land.
    set(&mut session, "patch/layer[1]/synth/sample/loop", 1.0);
    set(&mut session, "patch/layer[1]/synth/position", 0.5);
    set(&mut session, "patch/layer[1]/synth/sample/spray", 0.2);
    let osc = card(&session, "OSC B");
    let list = captions(&osc);
    assert!(!has(&list, "patch/layer[1]/synth/sample/loop_start"));
    assert!(!has(&list, "patch/layer[1]/synth/sample/loop_end"));
    assert_eq!(
        caption_of(&list, "patch/layer[1]/synth/sample/grain"),
        "GRAIN"
    );
    assert_eq!(
        caption_of(&list, "patch/layer[1]/synth/sample/spray"),
        "SPRAY"
    );
    let grain = osc
        .group
        .params
        .iter()
        .find(|p| p.address.as_str() == "patch/layer[1]/synth/sample/grain")
        .unwrap();
    assert_eq!(grain.display, "80 ms");
    assert!(matches!(grain.kind, ParamKind::Knob));
    match osc.picture {
        FlopsynthPicture::Sound {
            start, loop_region, ..
        } => {
            assert!((start - 0.5).abs() < 1e-3);
            let (a, b) = loop_region.expect("the spray shows as a region");
            assert!((a - 0.3).abs() < 1e-3 && (b - 0.7).abs() < 1e-3, "{a}..{b}");
        }
        other => panic!("{other:?}"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The SUB card's sugar (`docs/flopsynth-next.md` §4.3): a shape chooser
/// over the four sub tables, an octave chooser over the semitones, and a
/// direct-out switch over the route — three new addresses writing the
/// fields the raw ones write, drawn in their place on the sub's card.
#[test]
fn the_sub_card_has_a_shape_an_octave_and_a_direct_out() {
    use fontelle_dsp::{FilterRoute, SynthSource, WavetableId};
    let mut session = a_flopsynth();
    let sub = card(&session, "SUB");
    let drawn = captions(&sub);
    for tail in ["sub_shape", "octave", "direct"] {
        assert!(
            has(&drawn, &format!("patch/layer[3]/synth/{tail}")),
            "no {tail}: {drawn:?}"
        );
    }
    for tail in ["table", "semitones", "route"] {
        assert!(
            !has(&drawn, &format!("patch/layer[3]/synth/{tail}")),
            "{tail} is drawn twice"
        );
    }
    assert_eq!(
        caption_of(&drawn, "patch/layer[3]/synth/sub_shape"),
        "SHAPE"
    );
    assert_eq!(caption_of(&drawn, "patch/layer[3]/synth/octave"), "OCTAVE");
    assert_eq!(caption_of(&drawn, "patch/layer[3]/synth/direct"), "DIRECT");
    // Square, an octave down, direct out.
    set(&mut session, "patch/layer[3]/synth/sub_shape", 2.0 / 3.0);
    set(&mut session, "patch/layer[3]/synth/octave", 0.5);
    set(&mut session, "patch/layer[3]/synth/direct", 1.0);
    let patch = session.selected_patch().unwrap();
    let fontelle_core::Source::Synth(osc) = &patch.layers[3].source else {
        panic!("a synth layer");
    };
    assert_eq!(osc.source, SynthSource::Table(WavetableId::SubSquare));
    assert_eq!(osc.semitones, -12);
    assert_eq!(osc.filter_route, FilterRoute::Bypass);
    // Direct off puts the sub back through the first filter.
    set(&mut session, "patch/layer[3]/synth/direct", 0.0);
    let patch = session.selected_patch().unwrap();
    let fontelle_core::Source::Synth(osc) = &patch.layers[3].source else {
        panic!("a synth layer");
    };
    assert_eq!(osc.filter_route, FilterRoute::F1);
    // The raw addresses still answer, for the lanes that name them.
    use fontelle_core::patch_params::value;
    assert!(value(&patch, "patch/layer[3]/synth/semitones").is_some());
    assert!(value(&patch, "patch/layer[3]/synth/route").is_some());
    // And the full oscillators are as they were.
    let a = captions(&card(&session, "OSC A"));
    assert!(has(&a, "patch/layer[0]/synth/table") && !has(&a, "patch/layer[0]/synth/sub_shape"));
}

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
    assert_eq!(caption_of(&list, "patch/layer[0]/synth/position"), "pos");
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
        matches!(&kind.kind, ParamKind::Choice(options) if options == &["Table", "Sample", "String"])
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
    assert_eq!(caption_of(&list, "patch/layer[1]/synth/position"), "start");
    assert_eq!(
        caption_of(&list, "patch/layer[1]/synth/sample/loop"),
        "loop"
    );
    assert_eq!(
        caption_of(&list, "patch/layer[1]/synth/sample/loop_start"),
        "loop in"
    );
    assert_eq!(
        caption_of(&list, "patch/layer[1]/synth/sample/loop_end"),
        "loop out"
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
    // Turn the loop on and the picture shows the region.
    set(&mut session, "patch/layer[1]/synth/sample/loop", 1.0);
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
    set(&mut session, "patch/layer[2]/synth/kind", 0.5);
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
    set(&mut fresh, "patch/layer[2]/synth/kind", 0.5);
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
    set(&mut session, "patch/layer[0]/synth/kind", 1.0);
    let patch = session.selected_patch().unwrap();
    assert!(matches!(
        &patch.layers[0].source,
        fontelle_core::Source::Synth(osc) if osc.source == SynthSource::String
    ));
    let osc = card(&session, "OSC A");
    let list = captions(&osc);
    assert!(!has(&list, "patch/layer[0]/synth/table"));
    assert!(!has(&list, "patch/layer[0]/synth/sample/loop"));
    assert_eq!(caption_of(&list, "patch/layer[0]/synth/position"), "bright");
    assert_eq!(
        caption_of(&list, "patch/layer[0]/synth/string/stiffness"),
        "stiff"
    );
    assert_eq!(
        caption_of(&list, "patch/layer[0]/synth/string/damping"),
        "damp"
    );
    assert_eq!(
        caption_of(&list, "patch/layer[0]/synth/string/strike"),
        "strike"
    );
    assert_eq!(
        caption_of(&list, "patch/layer[0]/synth/string/decay"),
        "ring"
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
    for kind in [0.0f32, 0.5, 1.0] {
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

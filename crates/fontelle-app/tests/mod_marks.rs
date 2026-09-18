//! What the Flopsynth window's knobs wear — the ring on a modulated one, the
//! glow on one a badge could land on — asked of the document **once per
//! revision**, not once per knob.
//!
//! # The report this comes from
//!
//! > *"im seeming to just have stuttering when dragging audio clips in
//! > general"* — and, when it was pinned down: *"the drag stutter occurred
//! > for me while i had my cursor snap mode set to none."*
//!
//! With snap off every pointer motion is a `MoveClip`, and every accepted
//! command is a revision the window re-reads its lists on. One of those reads
//! was the knob marks, and it asked `routes_to` and `is_mod_destination` for
//! every control on the window — each of which cloned the selected patch and
//! rebuilt its whole destination list. On the bank's Grand Piano that was
//! about nine milliseconds a revision, so a mouse reporting at a few hundred
//! hertz starved the frames of the thread they were drawn on. Nothing about
//! it was audio: a note clip dragged with snap off stuttered the same way, and
//! an audio clip is simply the kind nobody drags on the grid.

mod common;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use fontelle_types::InstrumentKind;
use fontelle_ui::canvas::FlopsynthPage;
use fontelle_ui::document::StudioHost;

use common::SR;

/// Every control the window draws, over every page, by address.
fn every_control(session: &fontelle_app::Session) -> Vec<fontelle_types::ParamAddress> {
    FlopsynthPage::ALL
        .iter()
        .filter_map(|page| session.flopsynth(*page))
        .flat_map(|view| {
            view.cards
                .into_iter()
                .flat_map(|card| card.group.params.into_iter().map(|p| p.address))
        })
        .collect()
}

/// The marks say, for every control, exactly what the two per-knob questions
/// said — so the window can stop asking them.
#[test]
fn the_marks_agree_with_the_per_knob_answers() {
    let mut session = common::a_session_for(common::a_project_with_a_clip(8, 120.0, SR));
    session.set_channel_kind(0, InstrumentKind::Osc3);
    session.set_channel_kind(0, InstrumentKind::Flopsynth);

    let marks: HashMap<String, Option<f32>> = session
        .modulation_marks()
        .into_iter()
        .map(|mark| (mark.address.as_str().to_string(), mark.depth))
        .collect();
    assert!(
        !marks.is_empty(),
        "the Init patch has knobs a route can reach"
    );

    let controls = every_control(&session);
    assert!(!controls.is_empty());
    let mut modulated = 0;
    for address in &controls {
        let expected_destination = session.is_mod_destination(address);
        let expected_depth = session.routes_to(address).last().map(|route| route.depth);
        let mark = marks.get(address.as_str());
        assert_eq!(
            mark.is_some(),
            expected_destination,
            "{}: a mark exactly when a route could reach it",
            address.as_str()
        );
        if let Some(depth) = mark {
            assert_eq!(
                *depth,
                expected_depth,
                "{}: the newest route's depth",
                address.as_str()
            );
        }
        if expected_depth.is_some() {
            modulated += 1;
        }
    }
    assert_eq!(
        modulated, 1,
        "the Init patch's one route (ENV 2 → Filter 1 cutoff) wears a ring"
    );
}

/// A control nothing modulates is a mark with no depth — the glow without
/// the ring — and one the badge cannot reach is no mark at all.
#[test]
fn the_output_trim_is_no_destination_and_the_cutoff_is_a_modulated_one() {
    let mut session = common::a_session_for(common::a_project_with_a_clip(8, 120.0, SR));
    session.set_channel_kind(0, InstrumentKind::Osc3);
    session.set_channel_kind(0, InstrumentKind::Flopsynth);
    let marks = session.modulation_marks();
    let cutoff = marks
        .iter()
        .find(|mark| mark.address.as_str() == "patch/filter[0]/cutoff")
        .expect("the filter's cutoff can be modulated");
    assert!(
        cutoff.depth.is_some(),
        "and the Init patch's route reaches it"
    );
    let resonance = marks
        .iter()
        .find(|mark| mark.address.as_str() == "patch/filter[0]/resonance")
        .expect("so can its resonance");
    assert_eq!(resonance.depth, None, "nothing routes there yet");
    assert!(
        marks
            .iter()
            .all(|mark| mark.address.as_str() != "patch/gain"),
        "the output trim has no routes and can have none"
    );
}

/// The whole point: a revision's worth of marks, on the patch every new
/// project opens on, costs a fraction of a frame.
///
/// Two hundred revisions, because that is what a second of dragging with
/// snap off produces; they used to take the better part of two seconds in a
/// release build. Debug builds are slower, so the bound is loose — the claim
/// is the order of magnitude.
#[test]
fn a_revisions_marks_for_the_grand_piano_are_a_fraction_of_a_frame() {
    let session = common::a_session_for(fontelle_app::blank_project(8, 120.0, SR));
    assert!(
        session.flopsynth(FlopsynthPage::Synth).is_some(),
        "a new project opens on the built-in synth"
    );
    let started = Instant::now();
    for _ in 0..200 {
        let marks = session.modulation_marks();
        assert!(marks.len() > 20, "the Grand Piano has knobs to mark");
    }
    let took = started.elapsed();
    assert!(
        took < Duration::from_millis(500),
        "200 revisions of marks took {took:?}; one has to fit inside a frame"
    );
}

/// §3.3: one ring per route, in the **source's** colour — so the marks
/// carry every route to a control, oldest first, each with its source's
/// family, and the newest's depth is still the depth the old ring showed.
/// The Grand Piano's cutoff has three: an envelope, the velocity and a
/// macro.
#[test]
fn a_mark_carries_every_route_with_its_sources_family() {
    use fontelle_ui::document::SourceFamily;
    let session = common::a_session_for(fontelle_app::blank_project(8, 120.0, SR));
    let marks = session.modulation_marks();
    let cutoff = marks
        .iter()
        .find(|mark| mark.address.as_str() == "patch/filter[0]/cutoff")
        .expect("the cutoff can be modulated");
    let families: Vec<SourceFamily> = cutoff.rings.iter().map(|ring| ring.family).collect();
    assert_eq!(
        families,
        [
            SourceFamily::Envelope,
            SourceFamily::Note,
            SourceFamily::Macro
        ],
        "ENV 2, Velocity, Brightness"
    );
    assert_eq!(
        cutoff.depth,
        cutoff.rings.last().map(|ring| ring.depth),
        "the newest route's depth is the one the ring used to show"
    );
    let routes = session.routes_to(&fontelle_types::ParamAddress::new("patch/filter[0]/cutoff"));
    for (ring, route) in cutoff.rings.iter().zip(routes.iter()) {
        assert!((ring.depth - route.depth).abs() < 1e-6);
        assert_eq!(
            session.mod_sources()[ring.source],
            route.source,
            "the ring names its source by index"
        );
    }
    // The live values, one per source, for the dot on each band: the
    // macros read the patch, and the list is as long as the sources.
    let values = session.mod_source_values();
    assert_eq!(values.len(), session.mod_sources().len());
    let brightness = session
        .mod_sources()
        .iter()
        .position(|s| s == "Brightness")
        .unwrap();
    assert!((0.0..=1.0).contains(&values[brightness]));
}

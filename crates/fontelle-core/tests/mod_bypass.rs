//! A route can be **bypassed** (`docs/flopsynth-next.md` §3.4): kept in the
//! matrix with its source, destination and depth, and heard by nothing —
//! the way an effect slot is switched off rather than pulled out. A route
//! taken out to try the sound without it has to be put back by hand, and
//! its depth with it.

use fontelle_core::{Curve, ModDest, ModMatrix, ModRoute, ModSource};

fn route(bypass: bool) -> ModRoute {
    ModRoute {
        source: ModSource::Lfo(0),
        destination: ModDest::FilterCutoff(0),
        depth: 0.8,
        curve: Curve::Linear,
        via: None,
        invert: false,
        bypass,
    }
}

#[test]
fn a_bypassed_route_contributes_nothing_and_a_live_one_its_depth() {
    let sources = |_: ModSource| 1.0;
    let live = ModMatrix {
        routes: vec![route(false)],
    };
    assert!((live.evaluate(ModDest::FilterCutoff(0), &sources) - 0.8).abs() < 1e-6);
    let off = ModMatrix {
        routes: vec![route(true)],
    };
    assert_eq!(off.evaluate(ModDest::FilterCutoff(0), &sources), 0.0);
    // Both routes in one matrix: only the live one is heard.
    let both = ModMatrix {
        routes: vec![route(true), route(false)],
    };
    assert!((both.evaluate(ModDest::FilterCutoff(0), &sources) - 0.8).abs() < 1e-6);
}

/// A patch written before the field existed reads with every route live,
/// and a live route writes no `bypass` — the file is the file it was.
#[test]
fn bypass_is_absent_from_the_file_unless_set() {
    let live = serde_json::to_string(&route(false)).unwrap();
    assert!(!live.contains("bypass"), "{live}");
    let off = serde_json::to_string(&route(true)).unwrap();
    assert!(off.contains("\"bypass\":true"), "{off}");
    let old = r#"{"source":{"Lfo":0},"destination":{"FilterCutoff":0},"depth":0.8,"curve":"Linear","via":null,"invert":false}"#;
    let read: ModRoute = serde_json::from_str(old).unwrap();
    assert!(!read.bypass);
    let read: ModRoute = serde_json::from_str(&off).unwrap();
    assert!(read.bypass);
}

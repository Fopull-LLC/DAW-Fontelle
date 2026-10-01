//! Where the mixer's patch cables plug in: a patch bay under the strips, a
//! pair of jacks per strip, and which cables a set of routes makes.
//!
//! The simulation that hangs them is `tests/cables.rs`. This is the pure
//! geometry around it, per §2.5 of the plan: no window, no document.

use fontelle_ui::cables::{CableKey, CableRole};
use fontelle_ui::canvas::{
    MAX_SEND_DB, MIN_SEND_DB, PATCH_HEIGHT, SEND_KNOB_TRAVEL, StripRoute, mixer_cables,
    mixer_layout, mixer_layout_for, send_knob_db,
};
use fontelle_ui::document::{MixerStrip, SendInfo};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::Theme;

fn strip(name: &str) -> MixerStrip {
    MixerStrip {
        name: name.to_string(),
        gain_db: 0.0,
        pan: 0.0,
        mute: false,
        solo: false,
        is_master: false,
        color: [0x4f, 0x8f, 0xd0, 0xff],
        inserts: Vec::new(),
        sends: Vec::new(),
    }
}

/// `count` ordinary tracks and a master, last, as `mixer_strips` gives them.
fn strips(count: usize) -> Vec<MixerStrip> {
    let mut out: Vec<MixerStrip> = (0..count)
        .map(|i| strip(&format!("Track {}", i + 1)))
        .collect();
    out.push(MixerStrip {
        is_master: true,
        ..strip("Master")
    });
    out
}

/// Every track to the master, output on — a new project.
fn routes(count: usize) -> Vec<StripRoute<usize>> {
    (0..count)
        .map(|id| StripRoute {
            id: 100 + id,
            output: None,
            output_on: true,
        })
        .collect()
}

fn body() -> Rect {
    Rect::new(10.0, 40.0, 900.0, 400.0)
}

fn metrics() -> fontelle_ui::theme::Metrics {
    Theme::dark_default().metrics
}

#[test]
fn the_patch_bay_runs_under_every_strip_and_takes_nothing_from_the_options() {
    let strips = strips(4);
    let l = mixer_layout(body(), &metrics(), &strips, 0);
    let master = l.master.as_ref().unwrap();
    assert!(!l.patch.is_empty(), "a panel this tall has a patch bay");
    assert!((l.patch.height - PATCH_HEIGHT).abs() < 1e-3);
    assert!(
        (l.patch.bottom() - body().bottom()).abs() < 1e-3,
        "along the bottom of the panel"
    );
    assert_eq!(l.patch.x, master.frame.x, "from under the master");
    assert!(
        (l.patch.right() - l.list.right()).abs() < 1e-3,
        "to the end of the list — the options column keeps its height"
    );
    for s in l.strips.iter().chain(std::iter::once(master)) {
        assert!(
            s.frame.bottom() <= l.patch.y + 1e-3,
            "strip {} runs into the bay: {:?} over {:?}",
            s.index,
            s.frame,
            l.patch
        );
    }
    let options = l.options.as_ref().expect("room for the options column");
    assert_eq!(options.frame.height, body().height);
}

#[test]
fn a_tiny_panel_gives_up_the_bay_before_the_faders() {
    // The fader floor again: a mixer is its faders. Too short for both, the
    // cables go — but only when even a shrunk bay will not fit
    // (`tests/mixer_view.rs`).
    let strips = strips(4);
    let short = Rect::new(10.0, 40.0, 900.0, 120.0);
    let l = mixer_layout(short, &metrics(), &strips, 0);
    assert!(l.patch.is_empty());
    assert_eq!(l.strips[0].frame.height, short.height);
    assert!(l.jacks(0).is_none());
}

#[test]
fn each_strip_has_an_input_and_an_output_jack_in_the_bay_under_it() {
    let strips = strips(4);
    let l = mixer_layout(body(), &metrics(), &strips, 0);
    for s in l.strips.iter().chain(l.master.iter()) {
        let jacks = l.jacks(s.index).expect("a jack pair per strip");
        for (what, p) in [("input", jacks.input), ("output", jacks.output)] {
            assert!(
                l.patch.contains(p[0], p[1]),
                "strip {}'s {what} jack {p:?} is outside the bay {:?}",
                s.index,
                l.patch
            );
            assert!(
                p[0] > s.frame.x && p[0] < s.frame.right(),
                "strip {}'s {what} jack is under a different strip",
                s.index
            );
        }
        assert!(
            jacks.input[0] < jacks.output[0],
            "in on the left, out on the right — the way the strips read"
        );
    }
}

#[test]
fn a_strip_scrolled_away_still_has_jacks_off_the_end_of_the_list() {
    // A cable to a track you have scrolled past runs off the edge of the
    // list towards it, rather than vanishing: where it goes is the point.
    let strips = strips(40);
    let l = mixer_layout(body(), &metrics(), &strips, 5);
    let before = l.jacks(2).expect("scrolled past, still somewhere");
    assert!(before.output[0] < l.list.x);
    let after = l.jacks(38).expect("not reached yet, still somewhere");
    assert!(after.input[0] > l.list.right());
    assert!(l.jacks(strips.len()).is_none(), "no such strip");
}

#[test]
fn every_track_is_wired_to_where_it_goes() {
    let mut strips = strips(3);
    strips[1].sends.push(SendInfo {
        target: 2,
        target_name: "Track 3".into(),
        level_db: -9.0,
        pre_fader: false,
    });
    let mut routes = routes(4);
    routes[0].output = Some(2); // Track 1 -> Track 3
    routes[2].output_on = false; // Track 3's output switched off
    let l = mixer_layout_for(body(), &metrics(), &strips, 0, Some(1));
    let cables = mixer_cables(&l, &strips, &routes, 1);

    let find = |track: usize, role: CableRole| {
        cables
            .iter()
            .find(|c| c.key == CableKey { track, role })
            .unwrap_or_else(|| panic!("no {role:?} cable from {track}"))
    };
    let master_in = l.jacks(3).unwrap().input;
    let track3_in = l.jacks(2).unwrap().input;

    let one = find(100, CableRole::Output);
    assert_eq!(one.from, l.jacks(0).unwrap().output);
    assert_eq!(one.to, Some((track3_in, 102)), "into Track 3, by its id");

    let two = find(101, CableRole::Output);
    assert_eq!(two.to, Some((master_in, 103)), "`None` is the master");

    let three = find(102, CableRole::Output);
    assert_eq!(three.to, None, "switched off: a loose end");

    let the_send = find(101, CableRole::Send(0));
    assert_eq!(the_send.to, Some((track3_in, 102)));
    assert_eq!(the_send.level_db, Some(-9.0));
    assert_eq!(
        the_send.color, strips[1].color,
        "a cable is its track's colour"
    );

    assert!(
        cables.iter().all(|c| c.key.track != 103),
        "the master goes to the speakers, not down a cable"
    );
    assert_eq!(cables.len(), 4);
}

#[test]
fn the_selected_tracks_cables_are_lit_and_so_are_the_ones_arriving_at_it() {
    let strips = strips(3);
    let mut routes = routes(4);
    routes[0].output = Some(1); // Track 1 -> Track 2
    let l = mixer_layout_for(body(), &metrics(), &strips, 0, Some(1));
    let cables = mixer_cables(&l, &strips, &routes, 1);
    let lit = |track| {
        cables
            .iter()
            .find(|c| c.key.track == track && c.key.role == CableRole::Output)
            .unwrap()
            .lit
    };
    assert!(lit(101), "its own output");
    assert!(lit(100), "and what arrives at it");
    assert!(!lit(102), "not the rest");
}

#[test]
fn no_bay_no_cables() {
    let strips = strips(3);
    let l = mixer_layout(Rect::new(10.0, 40.0, 900.0, 120.0), &metrics(), &strips, 0);
    assert!(mixer_cables(&l, &strips, &routes(4), 0).is_empty());
}

#[test]
fn a_knob_on_a_wire_turns_by_dragging_up_and_down() {
    // A knob that rides a swinging wire cannot be turned by angle — the
    // centre it would be measured from is moving. Vertical travel from the
    // press, the way every knob in this window already turns.
    assert_eq!(send_knob_db(-12.0, 0.0), -12.0);
    assert!(send_knob_db(-12.0, -20.0) > -12.0, "up is more");
    assert!(send_knob_db(-12.0, 20.0) < -12.0, "down is less");
    let whole = MAX_SEND_DB - MIN_SEND_DB;
    let up = send_knob_db(MIN_SEND_DB, -SEND_KNOB_TRAVEL);
    assert!(
        (up - MAX_SEND_DB).abs() < 1e-3,
        "one travel is the whole range"
    );
    assert!(
        (send_knob_db(-12.0, -SEND_KNOB_TRAVEL / 2.0) - (-12.0 + whole / 2.0).min(MAX_SEND_DB))
            .abs()
            < 1e-3
    );
    assert_eq!(
        send_knob_db(0.0, -1000.0),
        MAX_SEND_DB,
        "and it stops at the ends"
    );
    assert_eq!(send_knob_db(0.0, 1000.0), MIN_SEND_DB);
}

#[test]
fn each_strip_has_a_row_under_its_name_for_what_feeds_it() {
    // *"'Fed by' list on the strip"* — readable without clicking anything,
    // so on the strip itself, between the name and the pan.
    let strips = strips(3);
    let l = mixer_layout(body(), &metrics(), &strips, 0);
    for s in l.strips.iter().chain(l.master.iter()) {
        assert!(!s.fed.is_empty(), "strip {} has room for it", s.index);
        assert!(s.fed.y >= s.name.bottom() - 1e-3, "under the name");
        assert!(s.fed.bottom() <= s.pan.y + 1e-3, "above the pan");
        assert!(s.fed.x >= s.frame.x && s.fed.right() <= s.frame.right());
    }
}

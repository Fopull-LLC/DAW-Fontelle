//! The mixer panel: a strip per mixer track (TDD §13, §16.1).
//!
//! The last thing outstanding under item 9 of `docs/first-usable-plan.md`, and
//! the last clause of the §3 gate sentence that had nothing on screen —
//! *"balance parts with per-channel gain/pan/mute"*. The rack has carried mute
//! and solo since it was written; a level and a place in the stereo field had
//! nowhere to be set from at all.
//!
//! Geometry, hit-testing and the fader's own arithmetic, all pure, per §2.5.
//! There is no window in this file and no audio device.

use fontelle_ui::canvas::{
    FADER_DETENT_PX, MAX_FADER_DB, MIN_FADER_DB, MixerHit, PAN_DETENT_PX, STRIP_WIDTH, fader_db_at,
    fader_y_of_db, mixer_hit, mixer_layout, pan_at, pan_x_of, unity_fraction,
};
use fontelle_ui::document::MixerStrip;
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
    }
}

/// `count` ordinary tracks and a master, which is what every project has.
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

fn body() -> Rect {
    Rect::new(10.0, 40.0, 900.0, 400.0)
}

fn theme() -> Theme {
    Theme::dark_default()
}

// ---------------------------------------------------------------- layout ---

#[test]
fn every_track_gets_a_strip_and_every_control_is_inside_it() {
    let strips = strips(4);
    let l = mixer_layout(body(), &theme().metrics, &strips, 0);
    assert_eq!(
        l.strips.len() + 1,
        strips.len(),
        "one strip per track, master apart"
    );

    // Edge by edge rather than `intersection(&frame) == r`: `intersection`
    // recomputes a width by subtraction, and a handle whose height is a
    // fraction of the row height comes back a float ulp adrift from a rect
    // that is genuinely inside.
    fn inside(r: Rect, frame: Rect) -> bool {
        r.x >= frame.x - 1e-3
            && r.y >= frame.y - 1e-3
            && r.right() <= frame.right() + 1e-3
            && r.bottom() <= frame.bottom() + 1e-3
    }

    let master = l.master.as_ref().expect("a project always has a master");
    for s in l.strips.iter().chain(std::iter::once(master)) {
        assert!(!s.frame.is_empty(), "strip {} has no room", s.index);
        for (name, r) in [
            ("name", s.name),
            ("fader", s.fader),
            ("handle", s.handle),
            ("meter", s.meter),
            ("pan", s.pan),
            ("mute", s.mute),
            ("solo", s.solo),
            ("value", s.value),
        ] {
            assert!(
                inside(r, s.frame),
                "{name} of strip {} at {r:?} escapes its frame {:?}",
                s.index,
                s.frame
            );
        }
        assert!(
            !s.mute.intersects(&s.solo),
            "mute and solo overlap on strip {}",
            s.index
        );
        assert!(
            !s.fader.intersects(&s.meter),
            "the fader and the meter overlap on strip {}",
            s.index
        );
    }
}

#[test]
fn the_master_is_pinned_to_the_right_and_never_scrolls_away() {
    // It is where everything arrives, not one of the things arriving. A master
    // fader you have to scroll to find is one you cannot use to set the level
    // of the thing you are listening to.
    let strips = strips(40);
    let unscrolled = mixer_layout(body(), &theme().metrics, &strips, 0);
    let scrolled = mixer_layout(body(), &theme().metrics, &strips, 12);

    let a = unscrolled.master.expect("master");
    let b = scrolled.master.expect("master");
    assert_eq!(a.frame, b.frame, "the master strip does not move");
    assert_eq!(a.index, strips.len() - 1, "and it is the last track");
    for s in &scrolled.strips {
        assert!(
            s.frame.right() <= a.frame.x,
            "strip {} runs under the master",
            s.index
        );
    }
}

#[test]
fn a_project_with_more_tracks_than_fit_lays_out_a_screenful_not_all_of_them() {
    // §16.4's rule, applied to the mixer: what is built is what is visible.
    let strips = strips(400);
    let l = mixer_layout(body(), &theme().metrics, &strips, 0);
    assert!(
        l.strips.len() < 20,
        "a 900px panel laid out {} strips",
        l.strips.len()
    );
    assert_eq!(l.total, strips.len(), "but it knows how many there are");
}

#[test]
fn scrolling_moves_which_tracks_are_shown_and_not_where_they_are_drawn() {
    let strips = strips(40);
    let a = mixer_layout(body(), &theme().metrics, &strips, 0);
    let b = mixer_layout(body(), &theme().metrics, &strips, 3);

    assert_eq!(a.strips[0].index, 0);
    assert_eq!(b.strips[0].index, 3);
    assert_eq!(
        a.strips[0].frame, b.strips[0].frame,
        "the first slot is in the same place either way"
    );
}

#[test]
fn a_panel_too_small_for_a_strip_yields_empty_rects_never_negative_ones() {
    let strips = strips(3);
    for (w, h) in [(0.0, 0.0), (10.0, 10.0), (STRIP_WIDTH, 20.0), (40.0, 400.0)] {
        let l = mixer_layout(Rect::new(0.0, 0.0, w, h), &theme().metrics, &strips, 0);
        for s in l.strips.iter().chain(l.master.iter()) {
            for r in [
                s.frame, s.name, s.fader, s.handle, s.meter, s.pan, s.mute, s.solo, s.value,
            ] {
                assert!(r.width >= 0.0 && r.height >= 0.0, "{w}x{h} produced {r:?}");
            }
        }
    }
}

// ------------------------------------------------------------ hit-testing ---

#[test]
fn each_control_reports_itself_and_the_strip_it_belongs_to() {
    let strips = strips(3);
    let l = mixer_layout(body(), &theme().metrics, &strips, 0);
    let s = l.strips[1].clone();

    let centre = |r: Rect| (r.x + r.width / 2.0, r.y + r.height / 2.0);
    let (x, y) = centre(s.mute);
    assert_eq!(mixer_hit(&l, x, y), MixerHit::Mute(1));
    let (x, y) = centre(s.solo);
    assert_eq!(mixer_hit(&l, x, y), MixerHit::Solo(1));
    let (x, y) = centre(s.pan);
    assert_eq!(mixer_hit(&l, x, y), MixerHit::Pan(1));
    let (x, y) = centre(s.fader);
    assert_eq!(mixer_hit(&l, x, y), MixerHit::Fader(1));
    let (x, y) = centre(s.name);
    assert_eq!(mixer_hit(&l, x, y), MixerHit::Name(1));
}

#[test]
fn the_master_answers_by_its_own_index_not_by_a_slot() {
    let strips = strips(3);
    let l = mixer_layout(body(), &theme().metrics, &strips, 0);
    let m = l.master.clone().expect("master");
    let x = m.fader.x + m.fader.width / 2.0;
    let y = m.fader.y + m.fader.height / 2.0;
    assert_eq!(mixer_hit(&l, x, y), MixerHit::Fader(strips.len() - 1));
}

#[test]
fn a_click_in_the_gaps_does_nothing() {
    let strips = strips(2);
    let l = mixer_layout(body(), &theme().metrics, &strips, 0);
    assert_eq!(mixer_hit(&l, -100.0, -100.0), MixerHit::Nothing);
    assert_eq!(
        mixer_hit(&l, body().right() + 50.0, body().y + 5.0),
        MixerHit::Nothing
    );
}

// ------------------------------------------------------- the fader's scale ---

#[test]
fn the_top_of_the_fader_is_the_loudest_it_goes_and_the_bottom_the_quietest() {
    let track = Rect::new(0.0, 100.0, 20.0, 200.0);
    assert_eq!(fader_db_at(track, track.y), MAX_FADER_DB);
    assert_eq!(fader_db_at(track, track.bottom()), MIN_FADER_DB);
    assert!(
        fader_db_at(track, track.y + 50.0) > fader_db_at(track, track.y + 150.0),
        "up is louder"
    );
}

#[test]
fn a_press_outside_the_fader_clamps_to_its_ends() {
    let track = Rect::new(0.0, 100.0, 20.0, 200.0);
    assert_eq!(fader_db_at(track, -1000.0), MAX_FADER_DB);
    assert_eq!(fader_db_at(track, 1000.0), MIN_FADER_DB);
}

#[test]
fn the_fader_and_its_handle_agree_about_where_a_level_is() {
    // The round trip is the whole contract: the handle is drawn where
    // `fader_y_of_db` says, and grabbing it there has to give back the level it
    // already had, or every fader jumps the moment it is touched.
    let track = Rect::new(0.0, 100.0, 20.0, 240.0);
    for db in [-60.0, -48.0, -30.0, -18.0, -10.0, -6.0, -3.0, 0.0, 3.0, 6.0] {
        let y = fader_y_of_db(track, db);
        let back = fader_db_at(track, y);
        assert!(
            (back - db).abs() < 0.25,
            "{db} dB drew at {y} and read back as {back}"
        );
    }
}

#[test]
fn unity_gets_the_top_of_the_travel_because_that_is_where_the_work_happens() {
    // A fader linear in decibels spends most of itself on levels nobody mixes
    // at. This one gives the top four-tenths to -10..+6 dB, which is the range
    // an actual balance decision lives in.
    let fraction = unity_fraction();
    assert!(
        (0.7..0.95).contains(&fraction),
        "unity sits at {fraction} of the travel"
    );

    // Ten pixels of travel is worth fewer decibels near unity than it is down
    // at the quiet end, which is what "the top of the fader is where the work
    // happens" actually means.
    let track = Rect::new(0.0, 0.0, 20.0, 400.0);
    let at_unity = -6.0 - fader_db_at(track, fader_y_of_db(track, -6.0) + 10.0);
    let at_the_bottom = -40.0 - fader_db_at(track, fader_y_of_db(track, -40.0) + 10.0);
    assert!(
        at_unity < at_the_bottom,
        "ten pixels moves {at_unity} dB near unity and {at_the_bottom} dB at -40"
    );
}

#[test]
fn the_fader_has_a_detent_at_unity_so_it_can_be_put_back() {
    // Getting exactly 0.0 dB back by hand on a curved fader is otherwise
    // impossible, and "nearly unity" is a mix that drifts every time it is
    // touched.
    let track = Rect::new(0.0, 0.0, 20.0, 240.0);
    let unity = fader_y_of_db(track, 0.0);
    assert_eq!(fader_db_at(track, unity + FADER_DETENT_PX / 2.0), 0.0);
    assert_eq!(fader_db_at(track, unity - FADER_DETENT_PX / 2.0), 0.0);
    assert_ne!(
        fader_db_at(track, unity + FADER_DETENT_PX * 3.0),
        0.0,
        "and it is a detent, not a dead zone"
    );
}

// ---------------------------------------------------------------- panning ---

#[test]
fn the_pan_strip_runs_left_to_right_through_the_centre() {
    let r = Rect::new(0.0, 0.0, 100.0, 12.0);
    assert_eq!(pan_at(r, r.x), -1.0);
    assert_eq!(pan_at(r, r.right()), 1.0);
    assert_eq!(pan_at(r, r.x + r.width / 2.0), 0.0);
}

#[test]
fn the_pan_has_a_centre_detent_for_the_same_reason_the_fader_has_one() {
    let r = Rect::new(0.0, 0.0, 200.0, 12.0);
    let centre = r.x + r.width / 2.0;
    assert_eq!(pan_at(r, centre + PAN_DETENT_PX / 2.0), 0.0);
    assert_eq!(pan_at(r, centre - PAN_DETENT_PX / 2.0), 0.0);
    assert_ne!(pan_at(r, centre + PAN_DETENT_PX * 4.0), 0.0);
}

#[test]
fn a_pan_outside_the_strip_clamps_rather_than_running_off_the_end() {
    let r = Rect::new(0.0, 0.0, 100.0, 12.0);
    assert_eq!(pan_at(r, -500.0), -1.0);
    assert_eq!(pan_at(r, 500.0), 1.0);
}

#[test]
fn the_pan_marker_and_the_pan_agree_about_where_a_value_is() {
    let r = Rect::new(20.0, 0.0, 160.0, 12.0);
    for pan in [-1.0, -0.5, 0.0, 0.25, 1.0] {
        let x = pan_x_of(r, pan);
        assert!(
            (pan_at(r, x) - pan).abs() < 0.02,
            "{pan} drew at {x} and read back as {}",
            pan_at(r, x)
        );
    }
}

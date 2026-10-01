//! The mixer's patch cables: a wire from every track to where it goes, hung
//! like a real one.
//!
//! > *"right now theres no visual indicator at a glance for mixer track
//! > routing. in fl studio, theres a wire connecting tracks to where they are
//! > being sent to, with a knob for how much is being sent there ... the wires
//! > should have nice physicsy looking animations so as i reorder mixer tracks
//! > or add or change wirings they will kind of snap together and jiggle
//! > around like real wires"*
//!
//! The simulation is pure — a rope of points stepped by a `dt` the caller
//! hands in — so what is tested here is what the eye is promised: a cable
//! ends on its jacks, hangs, comes to rest (and lets the window sleep), jumps
//! when a strip moves under it, plugs itself in when it is new, and falls
//! away when it goes.

use fontelle_ui::cables::{
    CableKey, CableRole, CableSpec, Cables, LOOSE_LENGTH, PLUG_SECONDS, Pt, SEGMENTS,
};

const FLOOR: f32 = 200.0;
const FRAME: f32 = 1.0 / 60.0;

fn output(track: usize, from: Pt, to: Option<(Pt, usize)>) -> CableSpec<usize> {
    CableSpec {
        key: CableKey {
            track,
            role: CableRole::Output,
        },
        from,
        to,
        color: [0x4f, 0x8f, 0xd0, 0xff],
        lit: false,
        level_db: None,
    }
}

fn send(track: usize, index: usize, from: Pt, to: (Pt, usize), level_db: f32) -> CableSpec<usize> {
    CableSpec {
        key: CableKey {
            track,
            role: CableRole::Send(index),
        },
        level_db: Some(level_db),
        ..output(track, from, Some(to))
    }
}

/// Runs the simulation for `seconds` of frames.
fn run(cables: &mut Cables<usize>, seconds: f32) {
    let frames = (seconds / FRAME).round() as usize;
    for _ in 0..frames {
        cables.step(FRAME);
    }
}

fn points(cables: &Cables<usize>, key: CableKey<usize>) -> Vec<Pt> {
    cables
        .lines()
        .into_iter()
        .find(|line| line.key == key)
        .unwrap_or_else(|| panic!("no cable {key:?}"))
        .points
}

fn out_key(track: usize) -> CableKey<usize> {
    CableKey {
        track,
        role: CableRole::Output,
    }
}

fn near(a: Pt, b: Pt, within: f32) -> bool {
    (a[0] - b[0]).hypot(a[1] - b[1]) <= within
}

#[test]
fn a_cable_ends_on_both_of_its_jacks() {
    let mut cables = Cables::new();
    cables.sync(
        vec![output(1, [300.0, 100.0], Some(([40.0, 100.0], 0)))],
        FLOOR,
    );
    run(&mut cables, 2.0);
    let p = points(&cables, out_key(1));
    assert_eq!(p.len(), SEGMENTS + 1);
    assert_eq!(p[0], [300.0, 100.0], "the plug is in the source's jack");
    assert_eq!(*p.last().unwrap(), [40.0, 100.0], "and in the target's");
}

#[test]
fn a_cable_hangs_between_its_jacks_and_never_through_the_floor() {
    let mut cables = Cables::new();
    cables.sync(
        vec![output(1, [300.0, 100.0], Some(([40.0, 100.0], 0)))],
        FLOOR,
    );
    run(&mut cables, 2.0);
    let p = points(&cables, out_key(1));
    let middle = p[SEGMENTS / 2];
    assert!(
        middle[1] > 100.0 + 8.0,
        "a wire with slack sags under its own weight: middle at {middle:?}"
    );
    assert!(
        p.iter().all(|q| q[1] <= FLOOR + 1e-3),
        "nothing falls through the floor of the patch bay: {p:?}"
    );
    assert!(p.iter().all(|q| q[0].is_finite() && q[1].is_finite()));
}

#[test]
fn a_cable_comes_to_rest_so_the_window_can_sleep() {
    // §16.3: a window with nothing moving in it neither requests nor draws a
    // frame. A rope that shivers forever at a tenth of a pixel is a mixer
    // that keeps the laptop's GPU awake.
    let mut cables = Cables::new();
    cables.sync(
        vec![
            output(1, [300.0, 100.0], Some(([40.0, 100.0], 0))),
            output(2, [380.0, 100.0], Some(([40.0, 100.0], 0))),
            send(2, 0, [380.0, 100.0], ([300.0, 100.0], 1), -6.0),
        ],
        FLOOR,
    );
    assert!(cables.is_moving(), "a cable just hung is still swinging");
    run(&mut cables, 6.0);
    assert!(!cables.is_moving(), "and six seconds later it has stopped");

    // Resyncing the very same cables is not a reason to wake.
    cables.sync(
        vec![
            output(1, [300.0, 100.0], Some(([40.0, 100.0], 0))),
            output(2, [380.0, 100.0], Some(([40.0, 100.0], 0))),
            send(2, 0, [380.0, 100.0], ([300.0, 100.0], 1), -6.0),
        ],
        FLOOR,
    );
    assert!(!cables.is_moving(), "nothing changed, nothing moves");
}

#[test]
fn a_strip_moving_under_a_cable_carries_the_plug_and_the_wire_jiggles() {
    // A reorder, a delete, a scroll: the jack is part of the strip, so the
    // plug goes with it at once — and the rope, which has mass, catches up.
    let mut cables = Cables::new();
    cables.sync(
        vec![output(1, [300.0, 100.0], Some(([40.0, 100.0], 0)))],
        FLOOR,
    );
    run(&mut cables, 6.0);
    assert!(!cables.is_moving());
    let before = points(&cables, out_key(1));

    cables.sync(
        vec![output(1, [220.0, 100.0], Some(([40.0, 100.0], 0)))],
        FLOOR,
    );
    let p = points(&cables, out_key(1));
    assert_eq!(p[0], [220.0, 100.0], "the plug moved with its strip");
    assert_eq!(p[1], before[1], "the wire itself has not caught up yet");
    assert!(cables.is_moving(), "and it is on its way");

    run(&mut cables, 0.1);
    let swinging = points(&cables, out_key(1));
    assert_ne!(swinging[SEGMENTS / 2], before[SEGMENTS / 2]);

    run(&mut cables, 6.0);
    assert!(!cables.is_moving(), "until it settles again");
}

#[test]
fn the_first_cables_are_already_hung_when_the_mixer_opens() {
    // Opening a project is not twenty cables plugging themselves in at once.
    let mut cables = Cables::new();
    cables.sync(
        vec![output(1, [300.0, 100.0], Some(([40.0, 100.0], 0)))],
        FLOOR,
    );
    let p = points(&cables, out_key(1));
    assert_eq!(
        *p.last().unwrap(),
        [40.0, 100.0],
        "plugged in from the start"
    );
    assert!(
        p[SEGMENTS / 2][1] > 100.0,
        "and already drooping rather than a ruler-straight line"
    );
}

#[test]
fn a_new_cable_plugs_itself_in() {
    // *"they will kind of snap together"*: a routing made after the mixer is
    // open is seen to travel from where it comes from to where it goes.
    let mut cables = Cables::new();
    cables.sync(
        vec![output(1, [300.0, 100.0], Some(([40.0, 100.0], 0)))],
        FLOOR,
    );
    run(&mut cables, 6.0);

    let to = ([300.0, 100.0], 1);
    cables.sync(
        vec![
            output(1, [300.0, 100.0], Some(([40.0, 100.0], 0))),
            send(2, 0, [380.0, 100.0], to, -6.0),
        ],
        FLOOR,
    );
    let key = CableKey {
        track: 2,
        role: CableRole::Send(0),
    };
    let start = points(&cables, key);
    assert!(
        near(*start.last().unwrap(), [380.0, 100.0], 1.0),
        "the plug starts at the source: {:?}",
        start.last()
    );
    assert!(cables.is_moving());

    run(&mut cables, PLUG_SECONDS / 2.0);
    let halfway = *points(&cables, key).last().unwrap();
    assert!(
        halfway[0] < 380.0 - 1.0 && halfway[0] > 300.0 + 1.0,
        "on its way across: {halfway:?}"
    );

    run(&mut cables, PLUG_SECONDS);
    assert_eq!(
        *points(&cables, key).last().unwrap(),
        [300.0, 100.0],
        "and then it is in, exactly"
    );
}

#[test]
fn rerouting_unplugs_the_far_end_and_plugs_it_in_somewhere_else() {
    let mut cables = Cables::new();
    cables.sync(
        vec![output(3, [460.0, 100.0], Some(([40.0, 100.0], 0)))],
        FLOOR,
    );
    run(&mut cables, 6.0);

    cables.sync(
        vec![output(3, [460.0, 100.0], Some(([300.0, 100.0], 1)))],
        FLOOR,
    );
    let p = points(&cables, out_key(3));
    assert!(
        near(*p.last().unwrap(), [40.0, 100.0], 1.0),
        "the plug leaves from where it was, not from the new jack"
    );
    run(&mut cables, PLUG_SECONDS + 0.05);
    assert_eq!(*points(&cables, out_key(3)).last().unwrap(), [300.0, 100.0]);
}

#[test]
fn a_target_moving_is_not_a_reroute() {
    // The target strip moved (a strip before it was deleted): same socket,
    // new place. The plug stays in it rather than flying across again.
    let mut cables = Cables::new();
    cables.sync(
        vec![output(3, [460.0, 100.0], Some(([300.0, 100.0], 1)))],
        FLOOR,
    );
    run(&mut cables, 6.0);
    cables.sync(
        vec![output(3, [460.0, 100.0], Some(([220.0, 100.0], 1)))],
        FLOOR,
    );
    assert_eq!(*points(&cables, out_key(3)).last().unwrap(), [220.0, 100.0]);
}

#[test]
fn an_unplugged_output_dangles_from_its_strip() {
    // A track whose output is switched off is silent by design — and until
    // now said so nowhere on the strip (docs/handoff.md, "an un-routed mixer
    // track"). A cable hanging loose from it is that mark.
    let mut cables = Cables::new();
    cables.sync(vec![output(1, [300.0, 100.0], None)], FLOOR);
    run(&mut cables, 6.0);
    let p = points(&cables, out_key(1));
    assert_eq!(p[0], [300.0, 100.0], "still plugged in at the strip's end");
    let free = *p.last().unwrap();
    assert!(
        free[1] > 100.0 + LOOSE_LENGTH * 0.5,
        "the free end hangs: {free:?}"
    );
    assert!(free[1] <= FLOOR + 1e-3);
    assert!(!cables.is_moving(), "and it too comes to rest");
}

#[test]
fn a_cable_that_goes_away_falls_and_fades_rather_than_vanishing() {
    let mut cables = Cables::new();
    cables.sync(
        vec![output(1, [300.0, 100.0], Some(([40.0, 100.0], 0)))],
        FLOOR,
    );
    run(&mut cables, 6.0);

    cables.sync(Vec::new(), FLOOR);
    let lines = cables.lines();
    assert_eq!(lines.len(), 1, "still drawn the frame it was removed");
    assert!(cables.is_moving());
    run(&mut cables, 0.1);
    let fading = cables.lines();
    assert!(
        fading[0].alpha < 1.0 && fading[0].alpha > 0.0,
        "fading: {}",
        fading[0].alpha
    );
    run(&mut cables, 2.0);
    assert!(cables.lines().is_empty(), "and then gone");
    assert!(!cables.is_moving());
}

#[test]
fn a_send_carries_a_knob_at_its_origin_and_an_output_does_not() {
    let mut cables = Cables::new();
    cables.sync(
        vec![
            output(2, [380.0, 100.0], Some(([40.0, 100.0], 0))),
            send(2, 0, [380.0, 100.0], ([300.0, 100.0], 1), -12.0),
        ],
        FLOOR,
    );
    run(&mut cables, 6.0);
    let lines = cables.lines();
    let out = lines.iter().find(|l| l.key == out_key(2)).unwrap();
    assert_eq!(out.knob, None, "an output's level is the fader's");
    let send_key = CableKey {
        track: 2,
        role: CableRole::Send(0),
    };
    let s = lines.iter().find(|l| l.key == send_key).unwrap();
    let knob = s.knob.expect("a send has a knob");
    assert_eq!(
        knob, s.points[0],
        "on the plug the wire leaves from — Ty: *\"make the knob be on the \
         origin point that the wire is coming from\"*"
    );
    assert_eq!(s.level_db, Some(-12.0));

    assert_eq!(cables.knob_at(knob[0] + 2.0, knob[1] - 2.0), Some(send_key));
    assert_eq!(cables.knob_at(knob[0] + 40.0, knob[1]), None);
}

#[test]
fn a_long_stall_does_not_throw_the_wires_across_the_screen() {
    // `tick` clamps its dt at a quarter second; a rope integrated over that
    // in one step explodes. Whatever it is handed, it stays in the bay.
    let mut cables = Cables::new();
    cables.sync(
        vec![output(1, [300.0, 100.0], Some(([40.0, 100.0], 0)))],
        FLOOR,
    );
    cables.sync(
        vec![output(1, [600.0, 100.0], Some(([40.0, 100.0], 0)))],
        FLOOR,
    );
    for _ in 0..10 {
        cables.step(0.25);
    }
    let p = points(&cables, out_key(1));
    assert!(
        p.iter()
            .all(|q| q[0] >= 0.0 && q[0] <= 700.0 && q[1] >= 0.0 && q[1] <= FLOOR + 1e-3),
        "{p:?}"
    );
}

#[test]
fn a_whole_new_mixer_arrives_hung_rather_than_plugging_itself_in() {
    // The window lays the mixer out empty before it has read the project,
    // and opening another project replaces every track at once. Neither is a
    // routing somebody just made: nothing flies in, and the old project's
    // cables do not linger falling over the new one's.
    let mut cables = Cables::new();
    cables.sync(Vec::new(), FLOOR);
    cables.sync(
        vec![output(1, [300.0, 100.0], Some(([40.0, 100.0], 0)))],
        FLOOR,
    );
    assert_eq!(
        *points(&cables, out_key(1)).last().unwrap(),
        [40.0, 100.0],
        "the project's cables are in their jacks from the first frame"
    );

    cables.sync(
        vec![output(7, [220.0, 100.0], Some(([40.0, 100.0], 6)))],
        FLOOR,
    );
    let lines = cables.lines();
    assert_eq!(lines.len(), 1, "the old project's cable is simply gone");
    assert_eq!(*lines[0].points.last().unwrap(), [40.0, 100.0]);
}

#[test]
fn a_knob_stays_put_while_its_wire_swings() {
    // At the origin, a knob is somewhere to aim: it does not ride a swing.
    let mut cables = Cables::new();
    cables.sync(
        vec![send(2, 0, [700.0, 120.0], ([40.0, 120.0], 1), -6.0)],
        FLOOR,
    );
    run(&mut cables, 0.1);
    let knob = cables.lines()[0].knob.unwrap();
    assert_eq!(knob, [700.0, 120.0]);
    assert_eq!(cables.knob_at(knob[0], knob[1]).map(|k| k.track), Some(2));
}

fn a_wire(cables: &mut Cables<usize>) -> Vec<Pt> {
    cables.sync(
        vec![output(1, [300.0, 100.0], Some(([40.0, 100.0], 0)))],
        FLOOR,
    );
    run(cables, 6.0);
    points(cables, out_key(1))
}

#[test]
fn a_wire_can_be_found_under_the_pointer_anywhere_along_it() {
    let mut cables = Cables::new();
    let p = a_wire(&mut cables);
    let middle = p[SEGMENTS / 2];
    assert_eq!(
        cables.cable_at(middle[0], middle[1] + 2.0),
        Some(out_key(1))
    );
    let near_end = p[SEGMENTS - 2];
    assert_eq!(cables.cable_at(near_end[0], near_end[1]), Some(out_key(1)));
    assert_eq!(
        cables.cable_at(middle[0], middle[1] - 40.0),
        None,
        "well off the wire is not the wire"
    );
}

#[test]
fn a_held_wire_follows_the_pointer_and_goes_home_when_let_go() {
    // *"i want to be able to click on a wire to pick it up and move where
    // its going to from one place to another."* Held, the far plug is in the
    // hand; let go somewhere that changes nothing, it goes back to its jack.
    let mut cables = Cables::new();
    a_wire(&mut cables);
    cables.hold(out_key(1), [200.0, 30.0]);
    assert!(cables.is_moving(), "picking it up wakes it");
    run(&mut cables, 0.5);
    assert_eq!(*points(&cables, out_key(1)).last().unwrap(), [200.0, 30.0]);
    assert!(
        !cables.lines()[0].plugged,
        "a plug in the hand is drawn as a bare plug"
    );
    cables.hold(out_key(1), [150.0, 60.0]);
    run(&mut cables, 0.1);
    assert_eq!(*points(&cables, out_key(1)).last().unwrap(), [150.0, 60.0]);

    // Syncing while held (a meter tick, a relayout) does not drop it.
    cables.sync(
        vec![output(1, [300.0, 100.0], Some(([40.0, 100.0], 0)))],
        FLOOR,
    );
    run(&mut cables, 0.1);
    assert_eq!(*points(&cables, out_key(1)).last().unwrap(), [150.0, 60.0]);

    cables.let_go();
    run(&mut cables, PLUG_SECONDS + 0.1);
    assert_eq!(
        *points(&cables, out_key(1)).last().unwrap(),
        [40.0, 100.0],
        "back in its own jack"
    );
    assert!(cables.lines()[0].plugged);
}

#[test]
fn a_wire_let_go_over_another_track_plugs_in_there() {
    let mut cables = Cables::new();
    a_wire(&mut cables);
    cables.hold(out_key(1), [120.0, 60.0]);
    run(&mut cables, 0.2);
    cables.let_go();
    // The window has rerouted it: the new routing arrives as a sync.
    cables.sync(
        vec![output(1, [300.0, 100.0], Some(([130.0, 100.0], 5)))],
        FLOOR,
    );
    run(&mut cables, 0.05);
    let end = *points(&cables, out_key(1)).last().unwrap();
    assert!(
        near(end, [120.0, 60.0], 30.0),
        "it starts from the hand, not from the old jack: {end:?}"
    );
    run(&mut cables, PLUG_SECONDS + 0.1);
    assert_eq!(*points(&cables, out_key(1)).last().unwrap(), [130.0, 100.0]);
}

#[test]
fn opening_the_mixer_again_shakes_the_wires_and_they_settle() {
    // *"whenever you swap windows and say open the mixer again the wires
    // should kind of look like theyve been disturbed, then settle back
    // down."*
    let mut cables = Cables::new();
    let rest = a_wire(&mut cables);
    assert!(!cables.is_moving());
    cables.disturb();
    assert!(cables.is_moving(), "a disturbed bay is a moving one");
    run(&mut cables, 0.15);
    let shaken = points(&cables, out_key(1));
    let moved = rest
        .iter()
        .zip(&shaken)
        .map(|(a, b)| (a[0] - b[0]).hypot(a[1] - b[1]))
        .fold(0.0_f32, f32::max);
    assert!(moved > 6.0, "visibly: {moved}");
    assert_eq!(shaken[0], rest[0], "the plugs stay in");
    assert_eq!(shaken[SEGMENTS], rest[SEGMENTS]);
    assert!(shaken.iter().all(|p| p[1] <= FLOOR + 1e-3));

    run(&mut cables, 6.0);
    assert!(!cables.is_moving(), "and it settles");
    let settled = points(&cables, out_key(1));
    for (a, b) in rest.iter().zip(&settled) {
        assert!(near(*a, *b, 1.5), "back where it hung: {a:?} {b:?}");
    }
}

/// Ty, watching the bay: *"its hard to tell which direction the wires are
/// going in sometimes, could you divise a solution that indicates which
/// direction the wire flows being mindful of too much clutter?"* One small
/// chevron per wire, three quarters of the way along, pointing the way the
/// signal goes — along the rope, so it swings with it.
#[test]
fn a_plugged_wire_carries_one_arrow_pointing_towards_where_it_goes() {
    let mut cables = Cables::new();
    cables.sync(
        vec![output(1, [300.0, 100.0], Some(([40.0, 100.0], 0)))],
        FLOOR,
    );
    run(&mut cables, 6.0);
    let line = &cables.lines()[0];
    let (at, dir) = line.arrow().expect("a plugged wire has an arrow");
    assert!(
        (dir[0].hypot(dir[1]) - 1.0).abs() < 1e-3,
        "a unit direction"
    );
    assert!(dir[0] < 0.0, "towards the target, leftwards: {dir:?}");
    assert!(
        (at[0] - 40.0).abs() < (at[0] - 300.0).abs(),
        "nearer where it goes than where it comes from: {at:?}"
    );
    // On the rope, not floating beside it.
    assert!(
        line.points
            .windows(2)
            .any(|w| near(at, w[0], 30.0) && near(at, w[1], 30.0))
    );
}

#[test]
fn a_loose_or_carried_wire_has_no_arrow() {
    // It goes nowhere yet; its bare plug already says so.
    let mut cables = Cables::new();
    cables.sync(vec![output(1, [300.0, 100.0], None)], FLOOR);
    run(&mut cables, 1.0);
    assert_eq!(cables.lines()[0].arrow(), None);

    let mut cables = Cables::new();
    cables.sync(
        vec![output(1, [300.0, 100.0], Some(([40.0, 100.0], 0)))],
        FLOOR,
    );
    run(&mut cables, 2.0);
    cables.hold(out_key(1), [200.0, 40.0]);
    run(&mut cables, 0.2);
    assert_eq!(cables.lines()[0].arrow(), None);
}

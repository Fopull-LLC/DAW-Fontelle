//! The track-options panel: everything about the selected mixer track that a
//! 76-pixel strip has no room for (TDD §13.2, §13.4).
//!
//! Reported from using the window:
//!
//! > *"right now with the mixer tracks on top of not being selectable, there's
//! > no place right now (from what i can see) to actually edit the effect
//! > stack for each track. like if i want to add an eq or reverb effect on a
//! > track i don't see a place to actually do that for my selected track right
//! > now. you can make this new section anchored to the left of the master
//! > that shows the "track options" for your selected tracks"*
//!
//! The strip *does* carry an insert rack — twelve pixels a row, three letters
//! and a switch, and it gives way to the fader the moment the panel is short.
//! That is the right thing for a strip and the wrong thing for the only place
//! a chain can be built. So the chain gets a column of its own, where a row is
//! a row you can read, reorder and throw away.
//!
//! Anchored between the last strip and the master, where the report asks for
//! it: it belongs with the mixer rather than in the editor column, because
//! what it edits is whichever strip you just clicked.
//!
//! Geometry and hit-testing only, per §2.5 — nothing here knows what a
//! `Project` is.

use fontelle_ui::canvas::{
    InsertInfo, MixerHit, OptionsHit, STRIP_WIDTH, mixer_hit, mixer_layout, mixer_layout_for,
};
use fontelle_ui::document::{MixerStrip, SendInfo};
use fontelle_ui::layout::Rect;
use fontelle_ui::theme::{Metrics, Theme};

fn metrics() -> Metrics {
    Theme::dark_default().metrics
}

fn strip(name: &str, inserts: Vec<InsertInfo>) -> MixerStrip {
    MixerStrip {
        name: name.into(),
        gain_db: 0.0,
        pan: 0.0,
        mute: false,
        solo: false,
        is_master: false,
        color: [0x60, 0x60, 0x68, 0xff],
        inserts,
        sends: Vec::new(),
    }
}

fn a_send(target: usize, name: &str, level_db: f32) -> SendInfo {
    SendInfo {
        target,
        target_name: name.into(),
        level_db,
        pre_fader: false,
    }
}

fn an_insert(label: &str) -> InsertInfo {
    InsertInfo {
        label: label.into(),
        bypassed: false,
        mix: 1.0,
        mix_automated: false,
    }
}

/// Two ordinary tracks and a master. The first carries a chain.
fn strips() -> Vec<MixerStrip> {
    vec![
        strip("Drums", vec![an_insert("EQ"), an_insert("Comp")]),
        strip("Bass", Vec::new()),
        MixerStrip {
            is_master: true,
            ..strip("Master", Vec::new())
        },
    ]
}

fn body() -> Rect {
    Rect::new(10.0, 40.0, 900.0, 400.0)
}

fn options(selected: usize) -> fontelle_ui::canvas::TrackOptionsLayout {
    mixer_layout_for(body(), &metrics(), &strips(), 0, Some(selected))
        .options
        .expect("a 900px panel has room for the options column")
}

fn centre(r: Rect) -> (f32, f32) {
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

// ------------------------------------------------------------ where it is ---

#[test]
fn the_panel_sits_past_the_last_strip_with_the_master_on_the_far_side() {
    // The master is pinned to the panel's left edge, so the column takes the
    // other end: strips in the middle, the thing they arrive at on one side
    // and the thing that edits them on the other.
    let l = mixer_layout_for(body(), &metrics(), &strips(), 0, Some(0));
    let o = l.options.expect("options");
    let master = l.master.expect("master");

    assert!(
        o.frame.x >= master.frame.right(),
        "the options {:?} run under the master {:?}",
        o.frame,
        master.frame
    );
    assert!(
        o.frame.x >= l.list.right(),
        "the options {:?} run over the strips {:?}",
        o.frame,
        l.list
    );
    for s in &l.strips {
        assert!(s.frame.right() <= o.frame.x, "strip {} is underneath", s.index);
    }
}

#[test]
fn every_row_in_the_panel_is_inside_it() {
    let o = options(0);
    let mut rects = vec![
        ("title", o.title),
        ("output", o.output),
        ("inserts title", o.inserts_title),
        ("add insert", o.add_insert),
    ];
    for row in &o.inserts {
        rects.push(("insert row", row.frame));
        rects.push(("bypass", row.bypass));
        rects.push(("name", row.name));
        rects.push(("grip", row.grip));
        rects.push(("remove", row.remove));
    }
    for (name, r) in rects {
        if r.is_empty() {
            continue;
        }
        assert_eq!(
            r.intersection(&o.frame),
            r,
            "the {name} {r:?} escapes the panel {:?}",
            o.frame
        );
    }
}

#[test]
fn nothing_in_the_panel_is_drawn_on_top_of_anything_else() {
    let o = options(0);
    let mut named: Vec<(String, Rect)> = vec![
        ("the title".into(), o.title),
        ("the output row".into(), o.output),
        ("the inserts heading".into(), o.inserts_title),
        ("the add-insert row".into(), o.add_insert),
    ];
    for row in &o.inserts {
        named.push((format!("insert row {}", row.slot), row.frame));
    }
    for (i, (a_name, a)) in named.iter().enumerate() {
        for (b_name, b) in named.iter().skip(i + 1) {
            if a.is_empty() || b.is_empty() {
                continue;
            }
            assert!(!a.intersects(b), "{a_name} {a:?} overlaps {b_name} {b:?}");
        }
    }
}

#[test]
fn a_panel_with_no_room_for_the_column_keeps_the_strips_instead() {
    // The strips are what a mixer is. A window dragged narrow loses the
    // options column, not its faders.
    let narrow = Rect::new(0.0, 0.0, STRIP_WIDTH * 3.0, 400.0);
    let l = mixer_layout_for(narrow, &metrics(), &strips(), 0, Some(0));
    assert!(l.options.is_none(), "the options column crowded the strips out");
    assert!(!l.strips.is_empty(), "and the strips survived");
}

#[test]
fn a_tiny_panel_yields_empty_rects_never_negative_ones() {
    for (w, h) in [(0.0, 0.0), (10.0, 10.0), (300.0, 24.0), (900.0, 8.0)] {
        let l = mixer_layout_for(Rect::new(0.0, 0.0, w, h), &metrics(), &strips(), 0, Some(0));
        let Some(o) = l.options else { continue };
        for r in [o.frame, o.title, o.output, o.inserts_title, o.add_insert] {
            assert!(r.width >= 0.0 && r.height >= 0.0, "{w}x{h} produced {r:?}");
        }
        for row in &o.inserts {
            for r in [row.frame, row.bypass, row.name, row.grip, row.remove] {
                assert!(r.width >= 0.0 && r.height >= 0.0, "{w}x{h} produced {r:?}");
            }
        }
    }
}

// ------------------------------------------------------- what it is about ---

#[test]
fn the_panel_shows_the_chain_of_the_selected_track() {
    let two = options(0);
    assert_eq!(two.track, 0);
    assert_eq!(two.inserts.len(), 2, "Drums has an EQ and a compressor");
    assert_eq!(two.inserts[0].slot, 0);
    assert_eq!(two.inserts[1].slot, 1);

    let none = options(1);
    assert_eq!(none.track, 1);
    assert!(none.inserts.is_empty(), "Bass has nothing on it");
    assert!(
        !none.add_insert.is_empty(),
        "an empty chain still has somewhere to put the first effect"
    );
}

#[test]
fn the_rows_are_in_chain_order_top_to_bottom() {
    // Which is the order the sound goes through them, and the only order a
    // rack may draw them in without lying about the signal path.
    let o = options(0);
    assert!(o.inserts[0].frame.y < o.inserts[1].frame.y);
    assert!(
        o.add_insert.y >= o.inserts[1].frame.bottom(),
        "a new effect goes on the end of the chain, so its button does too"
    );
}

#[test]
fn selecting_nothing_falls_back_to_a_track_rather_than_an_empty_column() {
    // A blank column that reserves the width and shows nothing is the worst of
    // both. `mixer_layout` is the four-argument form every caller that
    // predates the panel uses, and it has no selection to pass.
    let l = mixer_layout(body(), &metrics(), &strips(), 0);
    let o = l.options.expect("options");
    assert_eq!(o.track, 0, "the first track, which is always there");
}

#[test]
fn a_selection_past_the_end_falls_back_too() {
    let l = mixer_layout_for(body(), &metrics(), &strips(), 0, Some(99));
    let o = l.options.expect("options");
    assert!(o.track < strips().len());
}

// --------------------------------------------------------- hit-testing it ---

#[test]
fn every_control_in_the_panel_reports_itself() {
    let l = mixer_layout_for(body(), &metrics(), &strips(), 0, Some(0));
    let o = l.options.clone().expect("options");

    let (x, y) = centre(o.title);
    assert_eq!(mixer_hit(&l, x, y), MixerHit::Options(OptionsHit::Rename));
    let (x, y) = centre(o.output);
    assert_eq!(mixer_hit(&l, x, y), MixerHit::Options(OptionsHit::Output));
    let (x, y) = centre(o.add_insert);
    assert_eq!(mixer_hit(&l, x, y), MixerHit::Options(OptionsHit::AddInsert));

    let row = o.inserts[1].clone();
    let (x, y) = centre(row.name);
    assert_eq!(mixer_hit(&l, x, y), MixerHit::Options(OptionsHit::Insert(1)));
    let (x, y) = centre(row.bypass);
    assert_eq!(mixer_hit(&l, x, y), MixerHit::Options(OptionsHit::Bypass(1)));
    let (x, y) = centre(row.remove);
    assert_eq!(mixer_hit(&l, x, y), MixerHit::Options(OptionsHit::Remove(1)));
    let (x, y) = centre(row.grip);
    assert_eq!(mixer_hit(&l, x, y), MixerHit::Options(OptionsHit::Grip(1)));
}

#[test]
fn every_insert_row_carries_its_own_wet_dry() {
    // Asked for from using the mixer: *"i should have a knob to adjust the
    // sound of the dry sound (before the plugin) and the wet sound (after the
    // plugin processes the dry sound) blending like how fl studio and other
    // daws do it."*
    //
    // On the row rather than only inside the editor, and for the reason the
    // send's level is on its row: a mix is set by ear against the rest of the
    // chain, and a control you have to open a window to reach is one you set
    // once and never touch again.
    let l = mixer_layout_for(body(), &metrics(), &strips(), 0, Some(0));
    let o = l.options.clone().expect("options");

    for row in &o.inserts {
        assert!(!row.mix.is_empty(), "slot {} has no mix control", row.slot);
        assert!(
            row.frame.contains(row.mix.x + 1.0, row.mix.y + 1.0),
            "the mix control is outside its own row"
        );
        let (x, y) = centre(row.mix);
        assert_eq!(
            mixer_hit(&l, x, y),
            MixerHit::Options(OptionsHit::InsertMix(row.slot)),
            "the mix control does not answer for itself"
        );
    }
}

#[test]
fn the_mix_control_does_not_sit_on_top_of_the_rest_of_the_row() {
    let l = mixer_layout_for(body(), &metrics(), &strips(), 0, Some(0));
    let o = l.options.expect("options");
    let row = o.inserts[0].clone();
    for (what, other) in [
        ("bypass", row.bypass),
        ("name", row.name),
        ("grip", row.grip),
        ("remove", row.remove),
    ] {
        let overlap = row.mix.intersection(&other);
        assert!(
            overlap.is_empty(),
            "the mix control overlaps the {what} by {overlap:?}"
        );
    }
}

#[test]
fn the_mix_control_is_a_knob_and_turns_like_every_other_knob() {
    // Asked for after seeing it: *"i would like the knob to be not looking like
    // a slider and to actually resemble a knob"*. It is a dial now, and — more
    // to the point — it is **turned** rather than slid: up is more, down is
    // less, from where the value was when the drag started, which is the
    // gesture every other knob in this window already has.
    use fontelle_ui::canvas::{insert_mix_dial, knob_value};
    let l = mixer_layout_for(body(), &metrics(), &strips(), 0, Some(0));
    let o = l.options.expect("options");
    let row = o.inserts[0].clone();

    let dial = insert_mix_dial(row.mix);
    assert!(!dial.is_empty(), "there is no dial to draw");
    assert_eq!(
        dial.width, dial.height,
        "a dial is round, so its box is square"
    );
    assert!(
        row.mix.contains(dial.x + 1.0, dial.y + 1.0),
        "the dial has to be inside the target you grab"
    );
    assert!(
        row.mix.width > dial.width,
        "and there has to be room beside it for the number"
    );

    // Up is more and down is less, relative to where it started.
    assert!(knob_value(0.5, -40.0, false) > 0.5);
    assert!(knob_value(0.5, 40.0, false) < 0.5);
    assert_eq!(knob_value(1.0, -400.0, false), 1.0, "and it stops at wet");
    assert_eq!(knob_value(0.0, 400.0, false), 0.0, "and at dry");
}

#[test]
fn a_press_in_the_panels_empty_space_does_not_reach_the_strips() {
    // The column is over the mixer's own body. A click in it that fell through
    // to "no strip here" would be harmless; one that fell through to a *strip*
    // would move a fader you cannot see.
    let l = mixer_layout_for(body(), &metrics(), &strips(), 0, Some(0));
    let o = l.options.clone().expect("options");
    let x = o.frame.x + o.frame.width / 2.0;
    let y = o.frame.bottom() - 1.0;
    let hit = mixer_hit(&l, x, y);
    assert!(
        matches!(hit, MixerHit::Options(_) | MixerHit::Nothing),
        "a press in the options column answered {hit:?}"
    );
}

#[test]
fn the_panel_can_be_pointed_at_the_master() {
    // It is where every insert that treats the whole mix lives, and the strip
    // has the same twelve-pixel rows every other strip does.
    let l = mixer_layout_for(body(), &metrics(), &strips(), 0, Some(2));
    let o = l.options.expect("options");
    assert_eq!(o.track, 2);
    assert!(!o.add_insert.is_empty());
}

// --------------------------------------------------------------- the sends ---
//
// TDD §13.2's other half. A send is two numbers and a destination, so a
// 76-pixel strip has room for none of it — which is why the column exists at
// all and why the sends live here rather than on the strip.

/// The same rig, with a reverb send on the first track.
fn with_sends() -> Vec<MixerStrip> {
    let mut strips = strips();
    strips[0].sends = vec![a_send(1, "Reverb", -12.0), a_send(2, "Master", -30.0)];
    strips
}

fn sending(selected: usize) -> fontelle_ui::canvas::TrackOptionsLayout {
    mixer_layout_for(body(), &metrics(), &with_sends(), 0, Some(selected))
        .options
        .expect("a 900px panel has room for the options column")
}

#[test]
fn the_panel_shows_the_sends_of_the_selected_track() {
    let o = sending(0);
    assert_eq!(o.sends.len(), 2);
    assert_eq!(o.sends[0].index, 0);
    assert_eq!(o.sends[1].index, 1);
    assert!(!o.add_send.is_empty());

    let none = sending(1);
    assert!(none.sends.is_empty());
    assert!(
        !none.add_send.is_empty(),
        "a track with no sends still has somewhere to make the first one"
    );
}

#[test]
fn the_sends_sit_under_the_effects_rather_than_among_them() {
    // The chain and the sends are two different things about a track, and a
    // column that interleaved them would read as one list of nine rows.
    let o = sending(0);
    assert!(o.sends_title.y >= o.add_insert.bottom() - 0.001);
    assert!(o.sends[0].frame.y >= o.sends_title.bottom() - 0.001);
    assert!(o.add_send.y >= o.sends[1].frame.bottom() - 0.001);
}

#[test]
fn every_send_row_is_inside_the_panel_and_nothing_overlaps() {
    let o = sending(0);
    let mut named: Vec<(String, Rect)> = vec![
        ("the title".into(), o.title),
        ("the output row".into(), o.output),
        ("the effects heading".into(), o.inserts_title),
        ("the add-insert row".into(), o.add_insert),
        ("the sends heading".into(), o.sends_title),
        ("the add-send row".into(), o.add_send),
    ];
    for row in &o.inserts {
        named.push((format!("insert row {}", row.slot), row.frame));
    }
    for row in &o.sends {
        named.push((format!("send row {}", row.index), row.frame));
        for (part, rect) in [
            ("tap", row.tap),
            ("target", row.target),
            ("level", row.level),
            ("remove", row.remove),
        ] {
            // Edge by edge rather than `intersection(&frame) == rect`, for the
            // reason `tests/mixer.rs` gives: `intersection` recomputes a width
            // by subtraction, and a rectangle built from a fraction of another
            // comes back a float ulp adrift from one that is genuinely inside.
            assert!(
                rect.x >= o.frame.x - 1e-3
                    && rect.y >= o.frame.y - 1e-3
                    && rect.right() <= o.frame.right() + 1e-3
                    && rect.bottom() <= o.frame.bottom() + 1e-3,
                "the {part} of send {} escapes the panel: {rect:?} in {:?}",
                row.index,
                o.frame
            );
        }
    }
    for (i, (a_name, a)) in named.iter().enumerate() {
        for (b_name, b) in named.iter().skip(i + 1) {
            if a.is_empty() || b.is_empty() {
                continue;
            }
            assert!(!a.intersects(b), "{a_name} {a:?} overlaps {b_name} {b:?}");
        }
    }
}

#[test]
fn a_send_rows_four_controls_do_not_eat_each_other() {
    let o = sending(0);
    let row = o.sends[0].clone();
    for (a_name, a) in [("tap", row.tap), ("target", row.target), ("level", row.level)] {
        for (b_name, b) in [("target", row.target), ("level", row.level), ("remove", row.remove)] {
            if a == b || a.is_empty() || b.is_empty() {
                continue;
            }
            assert!(!a.intersects(&b), "the {a_name} overlaps the {b_name}");
        }
    }
    assert!(
        row.level.width > row.tap.width,
        "the level is the thing you drag and should get the room"
    );
}

#[test]
fn every_send_control_reports_itself() {
    let l = mixer_layout_for(body(), &metrics(), &with_sends(), 0, Some(0));
    let o = l.options.clone().expect("options");

    let (x, y) = centre(o.add_send);
    assert_eq!(mixer_hit(&l, x, y), MixerHit::Options(OptionsHit::AddSend));

    let row = o.sends[1].clone();
    let (x, y) = centre(row.tap);
    assert_eq!(mixer_hit(&l, x, y), MixerHit::Options(OptionsHit::SendTap(1)));
    let (x, y) = centre(row.target);
    assert_eq!(mixer_hit(&l, x, y), MixerHit::Options(OptionsHit::Send(1)));
    let (x, y) = centre(row.level);
    assert_eq!(
        mixer_hit(&l, x, y),
        MixerHit::Options(OptionsHit::SendLevel(1))
    );
    let (x, y) = centre(row.remove);
    assert_eq!(
        mixer_hit(&l, x, y),
        MixerHit::Options(OptionsHit::SendRemove(1))
    );
}

#[test]
fn a_send_level_reads_back_where_it_was_dragged() {
    // The same claim the fader rests on: what is drawn is what would be read
    // from a press there, or the control jumps the moment it is grabbed.
    use fontelle_ui::canvas::{send_level_at, send_x_of_level};

    let track = Rect::new(10.0, 0.0, 100.0, 12.0);
    for db in [-60.0, -40.0, -12.0, -3.0, 0.0, 6.0] {
        let x = send_x_of_level(track, db);
        let read = send_level_at(track, x);
        assert!(
            (read - db).abs() < 0.6,
            "{db} dB drew at {x} and read back as {read}"
        );
    }
}

#[test]
fn a_send_level_clamps_to_the_ends_of_its_travel() {
    use fontelle_ui::canvas::send_level_at;

    let track = Rect::new(10.0, 0.0, 100.0, 12.0);
    assert!(send_level_at(track, -500.0) <= -60.0);
    assert!(send_level_at(track, 500.0) >= 6.0);
    // A zero-width control answers rather than dividing by its own width.
    assert!(send_level_at(Rect::new(0.0, 0.0, 0.0, 0.0), 5.0).is_finite());
}

#[test]
fn a_column_too_short_for_the_sends_keeps_the_chain() {
    // The chain is what changes the sound of the track itself; a send is what
    // it gives to something else. When only one fits, it is the chain.
    let short = Rect::new(0.0, 0.0, 900.0, 150.0);
    let l = mixer_layout_for(short, &metrics(), &with_sends(), 0, Some(0));
    let Some(o) = l.options else { return };
    if o.sends.is_empty() {
        assert!(
            !o.inserts.is_empty() || o.add_insert.is_empty(),
            "the sends were dropped while the chain had nothing either"
        );
    }
    for row in &o.sends {
        assert!(row.frame.bottom() <= o.frame.bottom() + 1e-3);
        assert!(row.frame.y >= o.frame.y - 1e-3);
    }
}

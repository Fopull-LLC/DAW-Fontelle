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
use fontelle_ui::document::MixerStrip;
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
    }
}

fn an_insert(label: &str) -> InsertInfo {
    InsertInfo {
        label: label.into(),
        bypassed: false,
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
fn the_panel_sits_between_the_last_strip_and_the_master() {
    let l = mixer_layout_for(body(), &metrics(), &strips(), 0, Some(0));
    let o = l.options.expect("options");
    let master = l.master.expect("master");

    assert!(
        o.frame.right() <= master.frame.x,
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

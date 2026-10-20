//! *Find my scale…* in the window over a real song: the scale menu's row
//! opens the list, and choosing a scale sets the key and moves no note.

mod common;

use fontelle_model::Command;
use fontelle_ui::canvas::RollControl;

fn centre(r: fontelle_ui::layout::Rect) -> (f32, f32) {
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

#[test]
fn choosing_a_found_scale_sets_the_key_and_moves_nothing() {
    let mut project = common::a_clip_project(8);
    let clip = project.clips.keys().next().unwrap();
    // A tune in G major: an F# among the naturals, G first and last.
    let tune: Vec<fontelle_model::Note> = [67u8, 69, 71, 72, 74, 76, 78, 79, 67]
        .iter()
        .enumerate()
        .map(|(i, key)| fontelle_model::Note {
            start: i as i64 * fontelle_types::PPQN,
            length: fontelle_types::PPQN,
            key: *key,
            velocity: 100,
            pan: 0,
            fine_pitch: 0,
            release: 0,
            mod_x: 0,
            mod_y: 0,
            slide: false,
            path: Vec::new(),
            channel: None,
            lyric: String::new(),
            voice: None,
            muted: false,
        })
        .collect();
    fontelle_model::AddNotes::new(clip, tune)
        .apply(&mut project)
        .unwrap();
    let session = common::a_session_for(project);
    let mut w = fontelle_ui::Windowless::new(
        Box::new(session),
        fontelle_ui::theme::Theme::dark_default(),
        (1600, 1000),
    );
    let keys = |w: &fontelle_ui::Windowless| {
        let mut k: Vec<u8> = w.document().notes().values().map(|n| n.key).collect();
        k.sort();
        k
    };
    let before = keys(&w);

    let (x, y) = centre(w.roll_chip(RollControl::Scale).unwrap());
    w.click_studio(x, y);
    let find = w
        .menu_rows()
        .into_iter()
        .find(|(_, label)| label.contains("Find my scale"))
        .expect("the scale menu offers to find the scale");
    w.click_studio(centre(find.0).0, centre(find.0).1);
    let rows = w.menu_rows();
    assert!(rows.len() > 2, "{rows:?}");
    assert!(rows[1].1.starts_with("G major"), "{:?}", rows[1].1);
    w.click_studio(centre(rows[1].0).0, centre(rows[1].0).1);

    assert_eq!(
        w.document().song_key(),
        Some(fontelle_types::KeyScale::new(7, "major"))
    );
    assert_eq!(keys(&w), before, "a note moved");
}

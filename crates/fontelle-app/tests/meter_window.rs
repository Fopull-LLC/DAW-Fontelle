//! A time signature change, the way Ty described making one (2026-10-18):
//! the marker where it should start, the signature box's menu, *add a
//! change here*, and the box then sets that change.

mod common;

use fontelle_ui::document::DocumentHost;

/// A transport that is there, so the bar's boxes answer.
struct Still(fontelle_ui::transport::TransportView);

impl fontelle_ui::transport::TransportHost for Still {
    fn view(&mut self) -> fontelle_ui::transport::TransportView {
        self.0
    }
    fn play(&mut self) {}
    fn stop(&mut self) {}
    fn seek(&mut self, sample: i64) {
        self.0.position_sample = sample;
    }
    fn set_looping(&mut self, _on: bool) {}
}

fn centre(r: fontelle_ui::layout::Rect) -> (f32, f32) {
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

fn choose(w: &mut fontelle_ui::Windowless, words: &str) {
    let row = w
        .menu_rows()
        .into_iter()
        .find(|(_, label)| label.contains(words))
        .unwrap_or_else(|| panic!("no {words:?} in {:?}", w.menu_rows()));
    let (x, y) = (row.0.x + 10.0, row.0.y + row.0.height / 2.0);
    w.click_studio(x, y);
}

#[test]
fn a_change_is_added_at_the_marker_and_set_from_the_box() {
    let mut w = fontelle_ui::Windowless::new(
        Box::new(common::a_session_for(common::a_clip_project(16))),
        fontelle_ui::theme::Theme::dark_default(),
        (1600, 1000),
    )
    .with_host(Box::new(Still(fontelle_ui::transport::TransportView {
        available: true,
        length_samples: 48_000 * 60,
        sample_rate: 48_000.0,
        ..Default::default()
    })));
    let (ruler, grid, view) = w.timeline_ruler_and_view();
    // The marker into bar 5.
    let tick = 4 * 4 * fontelle_types::PPQN + 100;
    let x = fontelle_ui::canvas::timeline_tick_to_x(&view, grid, tick);
    w.click_studio(x, ruler.y + ruler.height / 2.0);

    let (bx, by) = centre(w.signature_box());
    w.click_studio(bx, by);
    choose(&mut w, "Add a signature change at bar 5");
    assert_eq!(w.meter_selected(), Some(4));
    assert_eq!(w.document().meter().changes().len(), 1);

    // The box now sets the change.
    w.click_studio(bx, by);
    choose(&mut w, "7/8");
    let meter = w.document().meter();
    assert_eq!(meter.signature(4), (7, 8));
    assert_eq!(meter.signature(3), (4, 4), "the song's own is untouched");

    DocumentHost::undo(w.document_mut());
    assert_eq!(w.document().meter().signature(4), (4, 4));
}

fn studio() -> fontelle_ui::Windowless {
    fontelle_ui::Windowless::new(
        Box::new(common::a_session_for(common::a_clip_project(16))),
        fontelle_ui::theme::Theme::dark_default(),
        (1600, 1000),
    )
    .with_host(Box::new(Still(fontelle_ui::transport::TransportView {
        available: true,
        length_samples: 48_000 * 60,
        sample_rate: 48_000.0,
        ..Default::default()
    })))
}

fn sig(top: u32, bottom: u32) -> fontelle_types::Signature {
    fontelle_types::Signature::new(top, bottom)
}

/// Ty, 2026-10-19: *"they absolutely need to be moveable."* A flag dragged
/// along the ruler takes its change to the bar line nearest where it is let
/// go — one undo.
#[test]
fn a_change_is_dragged_to_another_bar() {
    let mut w = studio();
    w.document_mut().set_meter_change(4, Some(sig(7, 8)));
    w.frame(0.0);
    let (_, flag) = w
        .meter_flags()
        .into_iter()
        .find(|(bar, _)| *bar == 4)
        .unwrap();
    let (ruler, grid, view) = w.timeline_ruler_and_view();
    let target = w.meter().tick_of_bar(6) + 40;
    let to_x = fontelle_ui::canvas::timeline_tick_to_x(&view, grid, target);
    let (fx, fy) = centre(flag);
    w.press_studio(winit::event::MouseButton::Left, fx, fy);
    w.move_studio((fx + to_x) / 2.0, ruler.y + ruler.height / 2.0);
    w.move_studio(to_x, ruler.y + ruler.height / 2.0);
    w.release_studio(winit::event::MouseButton::Left);
    let changes = w.document().meter().changes();
    assert_eq!(changes, vec![fontelle_types::MeterChange::new(6, 7, 8)]);
    assert_eq!(w.meter_selected(), Some(6), "it is still the one chosen");
    DocumentHost::undo(w.document_mut());
    assert_eq!(
        w.document().meter().changes(),
        vec![fontelle_types::MeterChange::new(4, 7, 8)]
    );
}

/// A click that does not travel is still a click: it chooses the flag, and
/// a second one lets it go.
#[test]
fn a_flag_clicked_in_place_is_chosen_not_moved() {
    let mut w = studio();
    w.document_mut().set_meter_change(4, Some(sig(7, 8)));
    w.frame(0.0);
    let (_, flag) = w
        .meter_flags()
        .into_iter()
        .find(|(bar, _)| *bar == 4)
        .unwrap();
    let (fx, fy) = centre(flag);
    w.click_studio(fx, fy);
    assert_eq!(w.meter_selected(), Some(4));
    assert_eq!(w.document().meter().changes().len(), 1);
    w.click_studio(fx, fy);
    assert_eq!(w.meter_selected(), None);
}

/// Any signature: the common ones at a click, anything else typed — any
/// top over any bottom, grouped or not.
#[test]
fn any_signature_is_set_from_the_box() {
    let mut w = studio();
    let (bx, by) = centre(w.signature_box());
    w.click_studio(bx, by);
    choose(&mut w, "6/8");
    assert_eq!(w.document().meter().song(), sig(6, 8));

    w.click_studio(bx, by);
    choose(&mut w, "Type a signature");
    assert!(w.prompt_open());
    w.answer_prompt("3+3+2/8");
    assert_eq!(
        w.document().meter().song(),
        sig(8, 8).with_groups(vec![3, 3, 2])
    );

    // Something it cannot keep is said, and asked again.
    w.click_studio(bx, by);
    choose(&mut w, "Type a signature");
    w.answer_prompt("4/0");
    assert!(w.prompt_open(), "asked again");
    assert_eq!(
        w.document().meter().song(),
        sig(8, 8).with_groups(vec![3, 3, 2]),
        "nothing changed"
    );
    w.answer_prompt("4/3");
    assert_eq!(w.document().meter().song(), sig(4, 3));
}

/// Snapping and stepping follow the bar the edit is in: an arrow with Ctrl
/// moves a clip a bar of the signature it is in.
#[test]
fn ctrl_arrow_moves_a_clip_a_bar_of_its_own_signature() {
    let mut w = studio();
    w.document_mut().set_meter_change(0, Some(sig(7, 8)));
    w.frame(0.0);
    let (_, grid, view) = w.timeline_ruler_and_view();
    // The first clip sits at the top of the song.
    let lane = w.lane_header(0);
    let x = fontelle_ui::canvas::timeline_tick_to_x(&view, grid, 10);
    w.click_studio(x, lane.y + lane.height / 2.0);
    w.hold(false, true);
    w.press_key_arrow(1, 0);
    w.hold(false, false);
    let start = w.document().clips().first().map(|c| c.start);
    assert_eq!(start, Some(7 * fontelle_types::PPQN / 2));
}

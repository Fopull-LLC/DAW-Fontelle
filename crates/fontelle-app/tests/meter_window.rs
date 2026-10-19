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

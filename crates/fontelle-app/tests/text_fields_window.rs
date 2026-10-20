//! Clicking inside a field that is being typed in.
//!
//! Ty, 2026-10-20: *"every typing input right now isnt a box you can click
//! inside, clicking at all actually stops the entering process so this is a
//! ux flaw for users right now."* A press inside the field puts the caret
//! where it landed and keeps the field; a double-click takes the word; a
//! press anywhere else still ends it, as before.

mod common;

use fontelle_ui::layout::Rect;
use winit::event::MouseButton;

/// A transport that is there, so the bar's boxes answer.
struct Still(fontelle_ui::transport::TransportView);

impl fontelle_ui::transport::TransportHost for Still {
    fn view(&mut self) -> fontelle_ui::transport::TransportView {
        self.0
    }
    fn play(&mut self) {}
    fn stop(&mut self) {}
    fn seek(&mut self, _sample: i64) {}
    fn set_looping(&mut self, _on: bool) {}
}

fn studio() -> fontelle_ui::Windowless {
    fontelle_ui::Windowless::new(
        Box::new(common::a_session_for(common::a_clip_project(8))),
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

fn centre(r: Rect) -> (f32, f32) {
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

fn click(w: &mut fontelle_ui::Windowless, x: f32, y: f32) {
    w.press_studio(MouseButton::Left, x, y);
    w.release_studio(MouseButton::Left);
}

fn open_signature_prompt(w: &mut fontelle_ui::Windowless) {
    let (x, y) = centre(w.signature_box());
    w.click_studio(x, y);
    let row = w
        .menu_rows()
        .into_iter()
        .find(|(_, label)| label.contains("Type a signature"))
        .expect("the signature menu offers typing");
    w.click_studio(row.0.x + 10.0, row.0.y + row.0.height / 2.0);
    assert!(w.prompt_open());
}

#[test]
fn a_press_inside_the_prompt_moves_the_caret_and_keeps_the_prompt() {
    let mut w = studio();
    open_signature_prompt(&mut w);
    w.type_into_prompt("3/4");
    let field = w.prompt_field_rect().expect("the prompt has a field");
    // At the text's left edge: the caret goes before the first character.
    click(&mut w, field.x + 5.0, field.y + field.height / 2.0);
    assert!(w.prompt_open(), "the press ended the prompt");
    w.type_into_prompt("1");
    assert_eq!(w.prompt_field().as_deref(), Some("13/4"));
    // Past the end: the caret goes to the end.
    click(&mut w, field.right() - 4.0, field.y + field.height / 2.0);
    w.type_into_prompt("8");
    assert_eq!(w.prompt_field().as_deref(), Some("13/48"));
}

#[test]
fn a_rename_survives_a_press_on_its_own_name_and_ends_on_one_elsewhere() {
    let mut w = studio();
    w.start_lane_rename(0);
    let field = w.rename_field_rect().expect("the lane's name is a field");
    // The rename opens with the whole name selected; a press at the start
    // of it puts a plain caret there.
    click(&mut w, field.x + 8.0, field.y + field.height / 2.0);
    let (text, caret, selection) = w.rename_entry().expect("still renaming");
    assert_eq!(caret, 0, "{text:?}");
    assert!(selection.is_none());
    // A double-click on it takes a word.
    let (x, y) = (field.x + 12.0, field.y + field.height / 2.0);
    click(&mut w, x, y);
    click(&mut w, x, y);
    let (_, _, selection) = w.rename_entry().expect("still renaming");
    assert!(selection.is_some(), "a double-click selected nothing");
    // Anywhere else ends it, as it always did.
    let roll = w.roll_point(0, 60);
    click(&mut w, roll.0, roll.1);
    assert!(w.rename_entry().is_none(), "a press elsewhere kept it");
}

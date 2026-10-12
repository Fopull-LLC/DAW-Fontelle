//! What the roll's corner says under the singer chip, as lines: a long
//! message is broken into lines a roll can hold, rather than one plate
//! running off the roll's left edge ("… is not installed on this
//! computer, so this track is silent — get it from Voicebanks → Find
//! singers, or choose another singer. Its notes and settings are kept." is
//! over a thousand pixels at the corner's size).

use fontelle_ui::document::VocalView;
use fontelle_ui::render::vocal::{CORNER_LINE_CHARS, corner_lines};

#[test]
fn a_long_message_is_broken_into_lines_the_roll_can_hold() {
    let message = "\u{201c}KasaneTeto_tougou_110401\u{201d} is not installed on this computer, \
                   so this track is silent \u{2014} get it from Voicebanks \u{2192} Find singers, \
                   or choose another singer. Its notes and settings are kept.";
    let view = VocalView {
        message: Some(message.to_string()),
        ..VocalView::default()
    };
    let lines = corner_lines(&view);
    assert!(lines.len() > 1, "{lines:?}");
    assert!(
        lines.iter().all(|l| l.chars().count() <= CORNER_LINE_CHARS),
        "{lines:?}"
    );
    assert_eq!(lines.join(" "), message, "nothing lost, words kept whole");
}

#[test]
fn a_short_message_stays_one_line() {
    let view = VocalView {
        message: Some("Choose a singer for this track".to_string()),
        ..VocalView::default()
    };
    assert_eq!(corner_lines(&view), ["Choose a singer for this track"]);
}

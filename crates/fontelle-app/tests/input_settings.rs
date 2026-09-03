//! The settings tab's rows, and what pressing one does (TDD §14.3, §18).
//!
//! Reported from playing a keyboard: *"the options [should] also be
//! configurable in a settings tab so I can adjust like my velocity for example
//! on my midi input, since every single input device will register
//! differently."*
//!
//! The window half is `fontelle-ui/tests/settings_tab.rs`, and the device half
//! is `fontelle-midi/tests/input_settings.rs`. This is the middle: the rows a
//! person reads, the step a click takes, and the file it all ends up in.
//! Everything here is a pure function over [`MidiInputSettings`], so what a
//! click does is checkable without a window and without a keyboard.

use fontelle_app::settings::{
    MidiInputSettings, SETTING_ROWS, Settings, SettingRow, VelocityCurveSetting,
};

/// A row's value, given only the MIDI half of the settings — which is all
/// every test in this file is about. `SettingRow::value` reads the whole file
/// because the tab now also lists the import folders; this puts the half
/// under test into an otherwise-default one.
fn value(row: SettingRow, s: &MidiInputSettings) -> String {
    let settings = Settings {
        midi_input: *s,
        ..Settings::default()
    };
    row.value(&settings)
}

/// `row` stepped `times` times in the direction `delta` says.
fn stepped(mut s: MidiInputSettings, row: SettingRow, delta: i32, times: usize) -> MidiInputSettings {
    for _ in 0..times {
        row.nudge(&mut s, delta);
    }
    s
}

// ------------------------------------------------------------- the rows ---

#[test]
fn every_row_says_what_it_is_and_what_it_is_at() {
    // The panel is 248 pixels wide and draws a name against a value. A row
    // with no name is a control nobody can identify; a row with no value is
    // one nobody can tell the state of.
    let s = MidiInputSettings::default();
    for row in SETTING_ROWS {
        assert!(!row.label().is_empty(), "{row:?} has no name");
        if let SettingRow::Heading(_) = row {
            continue;
        }
        assert!(
            !value(row, &s).is_empty(),
            "{row:?} does not say what it is at"
        );
    }
}

#[test]
fn a_heading_is_a_row_a_click_does_nothing_to() {
    // It says which of these settings belong together, which is the whole
    // difference between "Transpose" and "the MIDI keyboard's transpose".
    let before = MidiInputSettings::default();
    let after = stepped(before, SettingRow::Heading("MIDI input"), 1, 5);
    assert_eq!(after, before);
}

// ------------------------------------------------------ stepping a choice ---

#[test]
fn the_velocity_curve_steps_through_its_four_and_comes_back_round() {
    // A choice wraps: there is no "past the end" of four names, and a control
    // that stops at one end is one you have to know to go back through.
    let mut s = MidiInputSettings::default();
    let seen: Vec<VelocityCurveSetting> = (0..4)
        .map(|_| {
            let now = s.velocity_curve;
            SettingRow::VelocityCurve.nudge(&mut s, 1);
            now
        })
        .collect();
    assert_eq!(seen, VelocityCurveSetting::ALL.to_vec());
    assert_eq!(
        s.velocity_curve,
        VelocityCurveSetting::Linear,
        "four steps through four values is back where it started"
    );

    SettingRow::VelocityCurve.nudge(&mut s, -1);
    assert_eq!(
        s.velocity_curve,
        VelocityCurveSetting::Fixed,
        "and backwards from the first is the last"
    );
}

#[test]
fn the_channel_steps_from_every_channel_through_the_sixteen_a_keyboard_prints() {
    // Counted from 1, because that is what is written on the front of every
    // keyboard ever made. The 0..=15 the wire carries is `InputSettings`'
    // business and nobody else's.
    let mut s = MidiInputSettings::default();
    assert_eq!(value(SettingRow::ChannelFilter, &s), "All");
    SettingRow::ChannelFilter.nudge(&mut s, 1);
    assert_eq!(s.channel_filter, Some(1));
    assert_eq!(value(SettingRow::ChannelFilter, &s), "1");

    let s = stepped(s, SettingRow::ChannelFilter, 1, 15);
    assert_eq!(s.channel_filter, Some(16));
    let s = stepped(s, SettingRow::ChannelFilter, 1, 1);
    assert_eq!(s.channel_filter, None, "past the last one is all of them");
}

// ----------------------------------------------------- stepping a number ---

#[test]
fn a_number_stops_at_its_ends_rather_than_wrapping_round_to_the_other_one() {
    // The opposite rule from a choice, and for a plain reason: 127 and 0 are
    // the two ends of a range and not two of a set, so running off one and
    // arriving at the other is a control that cannot be trusted to a held
    // press.
    let s = stepped(MidiInputSettings::default(), SettingRow::VelocityMin, -1, 5);
    assert_eq!(s.velocity_min, 0);
    let s = stepped(s, SettingRow::VelocityMax, 1, 5);
    assert_eq!(s.velocity_max, 127);
}

#[test]
fn the_velocity_window_cannot_be_closed_past_itself() {
    // A window whose bottom is above its top lets nothing through at all,
    // which looks exactly like a broken keyboard. The two ends push each
    // other rather than crossing.
    let s = MidiInputSettings {
        velocity_max: 10,
        ..MidiInputSettings::default()
    };
    let s = stepped(s, SettingRow::VelocityMin, 1, 40);
    assert!(
        s.velocity_min <= s.velocity_max,
        "the window closed past itself: {}..{}",
        s.velocity_min,
        s.velocity_max
    );
    assert_eq!(
        fontelle_midi::InputSettings::from(s).velocity_range,
        (s.velocity_min, s.velocity_max)
    );
}

#[test]
fn transpose_reads_with_its_sign_and_its_unit() {
    // "+0" against "0" is the difference between a number that can go either
    // way and one that might not.
    let mut s = MidiInputSettings::default();
    assert_eq!(value(SettingRow::Transpose, &s), "+0 st");
    SettingRow::Transpose.nudge(&mut s, -1);
    assert_eq!(value(SettingRow::Transpose, &s), "-1 st");
    let s = stepped(s, SettingRow::Transpose, 1, 13);
    assert_eq!(value(SettingRow::Transpose, &s), "+12 st");
}

#[test]
fn transpose_stops_two_octaves_out_either_way() {
    let s = stepped(MidiInputSettings::default(), SettingRow::Transpose, 1, 100);
    assert_eq!(s.transpose_semitones, 24);
    let s = stepped(s, SettingRow::Transpose, -1, 100);
    assert_eq!(s.transpose_semitones, -24);
}

// -------------------------------------------- and it ends up in the file ---

#[test]
fn the_settings_survive_being_written_and_read_back() {
    let mut settings = Settings {
        midi_input: MidiInputSettings {
            velocity_curve: VelocityCurveSetting::Fixed,
            velocity_min: 12,
            velocity_max: 118,
            fixed_velocity: 90,
            transpose_semitones: -7,
            channel_filter: Some(10),
        },
        ..Settings::default()
    };
    settings.format_version = fontelle_app::settings::SETTINGS_FORMAT_VERSION;
    let back = Settings::from_json(&settings.to_json()).expect("this must read back");
    assert_eq!(back.midi_input, settings.midi_input);
}

#[test]
fn a_settings_file_written_before_this_existed_still_reads() {
    // The field is `#[serde(default)]` for exactly this: somebody's list of
    // soundfont folders must not become unreadable because a MIDI option was
    // added underneath it.
    let older = r#"{"format_version": 1, "soundfont_dirs": [], "projects_dir": null, "theme": null}"#;
    let settings = Settings::from_json(older).expect("a version 1 file still reads");
    assert_eq!(settings.midi_input, MidiInputSettings::default());
}

#[test]
fn a_channel_reaches_the_wire_counted_from_zero() {
    // The one conversion in the whole feature, and the one worth a test: a
    // person choosing "channel 1" means the channel a keyboard calls 1, and
    // MIDI calls that 0.
    let s = MidiInputSettings {
        channel_filter: Some(1),
        ..MidiInputSettings::default()
    };
    assert_eq!(fontelle_midi::InputSettings::from(s).channel_filter, Some(0));
    let s = MidiInputSettings {
        channel_filter: Some(16),
        ..MidiInputSettings::default()
    };
    assert_eq!(fontelle_midi::InputSettings::from(s).channel_filter, Some(15));
}

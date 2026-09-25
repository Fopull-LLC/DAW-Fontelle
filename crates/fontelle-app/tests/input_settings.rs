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
    MidiInputSettings, SETTING_ROWS, SettingRow, Settings, VelocityCurveSetting,
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
fn stepped(
    mut s: MidiInputSettings,
    row: SettingRow,
    delta: i32,
    times: usize,
) -> MidiInputSettings {
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
        assert!(
            !row.label(&Settings::default()).is_empty(),
            "{row:?} has no name"
        );
        if let SettingRow::Heading(_) = row {
            continue;
        }
        assert!(
            !value(row, &s).is_empty(),
            "{row:?} does not say what it is at"
        );
    }
}

/// Reported from using the window:
///
/// > *"please also remove unneccessary things from the settings this is
/// > supposed to be your settings across all projects but it seems like it has
/// > \[FL-]specific stuff to the project or specific intentions such as
/// > transposition? why is that in the settings right now? thats supposed to
/// > just be a piano roll tool!"*
///
/// It is not the piano roll's — this row transposes what a **MIDI keyboard**
/// sends, which is a property of the hardware and belongs in a settings file
/// that outlives every project. But it was called *"Transpose"*, sitting one
/// panel away from the roll's own transposer also called *"Transpose"*, and
/// the heading above it was doing all the work of telling them apart. A
/// heading three rows up is not enough: the row has to say it.
#[test]
fn the_keyboards_transpose_row_says_it_is_the_keyboards() {
    let label = SettingRow::Transpose
        .label(&Settings::default())
        .to_lowercase();
    assert!(
        label.contains("keyboard"),
        "a settings row called {:?} reads as the piano roll's transposer, \
         which is a different thing in a different place",
        SettingRow::Transpose.label(&Settings::default())
    );
    // And no other row is named so vaguely that it could be read as somebody's
    // song rather than as their setup.
    for row in SETTING_ROWS {
        assert_ne!(
            row.label(&Settings::default()).to_lowercase(),
            "transpose",
            "{row:?} is named after a piano roll tool"
        );
    }
}

#[test]
fn a_heading_is_a_row_a_click_does_nothing_to() {
    // It says which of these settings belong together.
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
    let older =
        r#"{"format_version": 1, "soundfont_dirs": [], "projects_dir": null, "theme": null}"#;
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
    assert_eq!(
        fontelle_midi::InputSettings::from(s).channel_filter,
        Some(0)
    );
    let s = MidiInputSettings {
        channel_filter: Some(16),
        ..MidiInputSettings::default()
    };
    assert_eq!(
        fontelle_midi::InputSettings::from(s).channel_filter,
        Some(15)
    );
}

// ---------------------------------------------- controls, not click-steps ---
//
// A settings row is not a button you click to iterate a list (the report this
// answers): a number is a slider you drag, a choice is a drop-down, a switch
// flips. These pure helpers are what a drag and a menu write through, so what
// each gesture does is checkable without a window.

#[test]
fn a_numbers_fraction_reads_where_its_handle_sits() {
    // The four ends, so a slider that is drawn from the fraction lands the
    // handle where the value is.
    let s = MidiInputSettings {
        velocity_min: 0,
        ..MidiInputSettings::default()
    };
    assert_eq!(SettingRow::VelocityMin.fraction(&s), Some(0.0));
    let mut s = MidiInputSettings {
        velocity_min: 127,
        velocity_max: 127,
        ..MidiInputSettings::default()
    };
    assert_eq!(SettingRow::VelocityMin.fraction(&s), Some(1.0));
    // Transpose is centred: zero is the middle of two octaves either way.
    s.transpose_semitones = 0;
    assert_eq!(SettingRow::Transpose.fraction(&s), Some(0.5));
    // A choice, a switch and a heading have no handle to place.
    assert_eq!(SettingRow::VelocityCurve.fraction(&s), None);
    assert_eq!(SettingRow::Heading("MIDI input").fraction(&s), None);
}

#[test]
fn dragging_a_slider_to_a_fraction_sets_the_value() {
    let mut s = MidiInputSettings::default();
    // The far right of the transpose slider is +24; the middle is 0.
    SettingRow::Transpose.set_fraction(&mut s, 1.0);
    assert_eq!(s.transpose_semitones, 24);
    SettingRow::Transpose.set_fraction(&mut s, 0.5);
    assert_eq!(s.transpose_semitones, 0);
    SettingRow::Transpose.set_fraction(&mut s, 0.0);
    assert_eq!(s.transpose_semitones, -24);
    // Fixed velocity never reaches zero — a note-off by convention.
    SettingRow::FixedVelocity.set_fraction(&mut s, 0.0);
    assert_eq!(s.fixed_velocity, 1);
    SettingRow::FixedVelocity.set_fraction(&mut s, 1.0);
    assert_eq!(s.fixed_velocity, 127);
}

#[test]
fn the_velocity_window_still_cannot_be_closed_past_itself_by_a_drag() {
    // The same coupling `nudge` keeps: the two ends push each other rather
    // than crossing, so a slider cannot make a window that lets nothing
    // through.
    let mut s = MidiInputSettings {
        velocity_min: 40,
        velocity_max: 80,
        ..MidiInputSettings::default()
    };
    // Drag the top down below the bottom: the bottom comes with it.
    SettingRow::VelocityMax.set_fraction(&mut s, 0.0);
    assert_eq!(s.velocity_max, 0);
    assert_eq!(s.velocity_min, 0);
    // And the bottom up past the top: the top comes with it.
    let mut s = MidiInputSettings {
        velocity_min: 40,
        velocity_max: 80,
        ..MidiInputSettings::default()
    };
    SettingRow::VelocityMin.set_fraction(&mut s, 1.0);
    assert_eq!(s.velocity_min, 127);
    assert_eq!(s.velocity_max, 127);
}

#[test]
fn a_choice_lists_its_options_and_which_one_it_is_on() {
    let mut s = MidiInputSettings::default();
    let (curves, at) = SettingRow::VelocityCurve.choices(&s).expect("a choice");
    assert_eq!(curves, ["Linear", "Soft", "Hard", "Fixed"]);
    assert_eq!(at, 0, "starts on Linear");
    // The channel is All plus the sixteen a keyboard prints.
    let (channels, at) = SettingRow::ChannelFilter.choices(&s).expect("a choice");
    assert_eq!(channels.len(), 17);
    assert_eq!(channels[0], "All");
    assert_eq!(channels[16], "16");
    assert_eq!(at, 0, "starts on All");
    s.channel_filter = Some(3);
    assert_eq!(SettingRow::ChannelFilter.choices(&s).unwrap().1, 3);
    // A slider is not a choice.
    assert!(SettingRow::VelocityMin.choices(&s).is_none());
}

#[test]
fn choosing_a_drop_down_row_sets_it_in_one_press() {
    let mut s = MidiInputSettings::default();
    SettingRow::VelocityCurve.choose(&mut s, 2);
    assert_eq!(s.velocity_curve, VelocityCurveSetting::Hard);
    // Channel option 0 is All (no filter); option 5 is channel 5.
    SettingRow::ChannelFilter.choose(&mut s, 5);
    assert_eq!(s.channel_filter, Some(5));
    SettingRow::ChannelFilter.choose(&mut s, 0);
    assert_eq!(s.channel_filter, None);
}

#[test]
fn every_row_knows_which_kind_of_control_it_is() {
    use fontelle_app::settings::SettingControlKind as K;
    assert_eq!(SettingRow::Heading("x").control_kind(), K::Heading);
    assert_eq!(SettingRow::VelocityCurve.control_kind(), K::Choice);
    assert_eq!(SettingRow::ChannelFilter.control_kind(), K::Choice);
    assert_eq!(SettingRow::VelocityMin.control_kind(), K::Slider);
    assert_eq!(SettingRow::Transpose.control_kind(), K::Slider);
    assert_eq!(SettingRow::CheckForUpdates.control_kind(), K::Switch);
    assert_eq!(SettingRow::YourName.control_kind(), K::Text);
    assert_eq!(SettingRow::PluginFolder.control_kind(), K::Button);
    assert_eq!(SettingRow::Extension(0).control_kind(), K::Button);
    assert_eq!(
        SettingRow::Folder(fontelle_app::settings::FolderKind::Midi).control_kind(),
        K::Button
    );
}

// ------------------------------------------- sharing (collab-plan §10.4) ---

/// F47. Two rows under their own heading: the name the people you share
/// with see, and the relay — blank meaning Floptle Cloud. Both are typed,
/// so both are text rows, the first this page has had.
#[test]
fn your_name_and_the_relay_are_text_rows_under_sharing() {
    use fontelle_app::settings::SettingControlKind as K;
    assert_eq!(SettingRow::YourName.control_kind(), K::Text);
    assert_eq!(SettingRow::Relay.control_kind(), K::Text);
    let at = SETTING_ROWS
        .iter()
        .position(|row| *row == SettingRow::Heading("Sharing"))
        .expect("a Sharing heading");
    assert_eq!(SETTING_ROWS[at + 1], SettingRow::YourName);
    assert_eq!(SETTING_ROWS[at + 2], SettingRow::Relay);
    let settings = Settings::default();
    assert!(!SettingRow::YourName.value(&settings).is_empty());
    assert_eq!(SettingRow::Relay.value(&settings), "Floptle Cloud");
    assert_eq!(SettingRow::Relay.text(&settings), "");
}

/// A name is kept trimmed; a blank one goes back to the default rather than
/// showing nobody as "". The relay likewise: blank is Floptle Cloud.
#[test]
fn a_typed_name_is_kept_and_a_blank_one_goes_back_to_the_default() {
    let mut settings = Settings::default();
    SettingRow::YourName.set_text(&mut settings, "  Bob  ");
    assert_eq!(settings.display_name.as_deref(), Some("Bob"));
    assert_eq!(settings.your_name(), "Bob");
    assert_eq!(SettingRow::YourName.value(&settings), "Bob");
    assert_eq!(SettingRow::YourName.text(&settings), "Bob");
    SettingRow::YourName.set_text(&mut settings, "   ");
    assert_eq!(settings.display_name, None);
    assert!(!settings.your_name().trim().is_empty());

    SettingRow::Relay.set_text(&mut settings, " relay.example.com:7788 ");
    assert_eq!(settings.relay.as_deref(), Some("relay.example.com:7788"));
    assert_eq!(SettingRow::Relay.value(&settings), "relay.example.com:7788");
    SettingRow::Relay.set_text(&mut settings, "");
    assert_eq!(settings.relay, None);
}

/// §4.5 names a studio by an id of its own, minted once and kept: the record
/// of what two copies last agreed on is only as good as the name it is under.
#[test]
fn a_studio_keeps_one_install_id_for_good() {
    let mut settings = Settings::default();
    assert_eq!(settings.install, None);
    let first = settings.install_id();
    assert_eq!(settings.install_id(), first);
    let back = Settings::from_json(&settings.to_json()).expect("reads back");
    assert_eq!(back.install, Some(first));
}

/// Settings format 7 carries the three, and a format-6 file still reads.
#[test]
fn the_sharing_settings_survive_the_file_and_an_older_one_reads() {
    assert_eq!(fontelle_app::settings::SETTINGS_FORMAT_VERSION, 7);
    let mut settings = Settings::default();
    SettingRow::YourName.set_text(&mut settings, "Alice");
    SettingRow::Relay.set_text(&mut settings, "10.0.0.2:7788");
    let back = Settings::from_json(&settings.to_json()).expect("reads back");
    assert_eq!(back.display_name.as_deref(), Some("Alice"));
    assert_eq!(back.relay.as_deref(), Some("10.0.0.2:7788"));
    let older =
        r#"{"format_version": 6, "soundfont_dirs": [], "projects_dir": null, "theme": null}"#;
    let settings = Settings::from_json(older).expect("a version 6 file still reads");
    assert_eq!(settings.display_name, None);
    assert_eq!(settings.relay, None);
}

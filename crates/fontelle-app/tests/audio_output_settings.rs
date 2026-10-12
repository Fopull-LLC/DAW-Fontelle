//! The audio output in Settings: the backend, the device, the buffer, and
//! what it is doing.
//!
//! Reported: *"audio drivers not configurable enough so pretty sure its
//! defaulting to default audio drivers for a lot of users causing things to
//! sound like failing audio drivers sometimes"*. There was nothing to
//! configure: the default backend's default device at 128 frames, and a
//! dropout said nowhere but a terminal nobody had open.

mod common;

use std::path::PathBuf;

use fontelle_app::settings::{AudioOutputSettings, SETTINGS_FORMAT_VERSION, Settings};
use fontelle_ui::StudioHost;
use fontelle_ui::canvas::SettingControl;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fontelle-output-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn studio(dir: &std::path::Path) -> fontelle_app::Session {
    common::a_session_for(common::a_clip_project(1)).with_settings_path(dir.join("settings.json"))
}

fn row(session: &fontelle_app::Session, name: &str) -> usize {
    session
        .settings()
        .iter()
        .position(|r| r.name == name)
        .unwrap_or_else(|| panic!("no {name:?} row"))
}

fn choice(session: &fontelle_app::Session, name: &str) -> (Vec<String>, usize) {
    match session.setting_controls()[row(session, name)].clone() {
        SettingControl::Choice { options, chosen } => (options, chosen),
        other => panic!("{name} is {other:?}"),
    }
}

#[test]
fn audio_output_has_its_own_section_with_every_part_of_the_output() {
    let dir = scratch("rows");
    let session = studio(&dir);
    let entries = session.settings();
    let heading = entries
        .iter()
        .position(|r| r.name == "Audio output")
        .expect("an Audio output heading");
    for name in [
        "Backend",
        "Output device",
        "Buffer size",
        "Sample rate",
        "Playing through",
        "Dropouts",
    ] {
        assert!(row(&session, name) > heading, "{name} is under the heading");
    }

    let (backends, chosen) = choice(&session, "Backend");
    assert!(backends[0].starts_with("Automatic"), "{backends:?}");
    assert_eq!(chosen, 0);
    assert_eq!(
        &backends[1..],
        fontelle_engine::output_host_names().as_slice()
    );

    let (devices, chosen) = choice(&session, "Output device");
    assert_eq!(devices[0], "System default");
    assert_eq!(chosen, 0);

    let (buffers, chosen) = choice(&session, "Buffer size");
    assert_eq!(chosen, 0);
    assert!(
        buffers[0].starts_with(&format!(
            "Automatic ({} frames",
            fontelle_engine::DEFAULT_OUTPUT_BUFFER
        )),
        "{buffers:?}"
    );
    assert!(
        buffers.iter().any(|b| b.starts_with("128 frames")),
        "{buffers:?}"
    );
    assert!(
        buffers.iter().all(|b| b.contains(" ms")),
        "each says its latency: {buffers:?}"
    );

    let rate = row(&session, "Sample rate");
    assert_eq!(entries[rate].detail, "48 kHz");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_buffer_size_chosen_is_remembered() {
    let dir = scratch("buffer");
    let mut session = studio(&dir);
    let (buffers, _) = choice(&session, "Buffer size");
    let at = buffers
        .iter()
        .position(|b| b.starts_with("1024 frames"))
        .expect("1024 is offered");
    session.choose_setting(row(&session, "Buffer size"), at);
    assert_eq!(choice(&session, "Buffer size").1, at);

    let (read, _) = Settings::load_from(&dir.join("settings.json"));
    assert_eq!(read.audio_output.buffer_frames, Some(1024));

    session.choose_setting(row(&session, "Buffer size"), 0);
    let (read, _) = Settings::load_from(&dir.join("settings.json"));
    assert_eq!(
        read.audio_output.buffer_frames, None,
        "Automatic is no number"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_backend_chosen_is_remembered_and_forgets_the_device_of_the_last_one() {
    let dir = scratch("backend");
    let mut session = studio(&dir);
    let (backends, _) = choice(&session, "Backend");
    let alsa_or_first = backends
        .iter()
        .position(|b| b == "ALSA")
        .unwrap_or(1)
        .min(backends.len() - 1);
    session.choose_setting(row(&session, "Backend"), alsa_or_first);
    let (read, _) = Settings::load_from(&dir.join("settings.json"));
    assert_eq!(
        read.audio_output.host.as_deref(),
        Some(backends[alsa_or_first].as_str())
    );
    assert_eq!(choice(&session, "Backend").1, alsa_or_first);
    assert_eq!(read.audio_output.device, None);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn without_an_output_open_the_rows_say_so_rather_than_inventing_one() {
    let dir = scratch("closed");
    let session = studio(&dir);
    let entries = session.settings();
    assert_eq!(entries[row(&session, "Playing through")].detail, "Not open");
    assert_eq!(entries[row(&session, "Dropouts")].detail, "None");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_choice_is_kept_in_the_settings_file_and_an_older_file_has_none() {
    const { assert!(SETTINGS_FORMAT_VERSION >= 9) };
    let settings = Settings {
        audio_output: AudioOutputSettings {
            host: Some("JACK".into()),
            device: Some("system".into()),
            buffer_frames: Some(256),
        },
        ..Settings::default()
    };
    let back = Settings::from_json(&settings.to_json()).expect("reads back");
    assert_eq!(back.audio_output, settings.audio_output);
    let older =
        r#"{"format_version": 8, "soundfont_dirs": [], "projects_dir": null, "theme": null}"#;
    let read = Settings::from_json(older).expect("a version 8 file still reads");
    assert_eq!(read.audio_output, AudioOutputSettings::default());
}

// ------------------------------------------------ Windows's clicks, 2026-10 ---

fn studio_with(dir: &std::path::Path, output: AudioOutputSettings) -> fontelle_app::Session {
    let settings = Settings {
        audio_output: output,
        ..Settings::default()
    };
    std::fs::write(dir.join("settings.json"), settings.to_json()).unwrap();
    studio(dir)
}

/// *"clicky/crackly audio on Windows"*: whose click it was, and how busy the
/// audio thread is, said under the output — a device that drops buffers
/// wants another backend, a Fontelle that is late wants a larger buffer.
#[test]
fn the_output_says_how_busy_it_is_with_whose_dropouts_beside_it() {
    let dir = scratch("load");
    let session = studio(&dir);
    let heading = row(&session, "Audio output");
    let load = row(&session, "Processing load");
    assert!(load > heading);
    assert_eq!(load, row(&session, "Dropouts") + 1, "beside the dropouts");
    assert_eq!(session.settings()[load].detail, "Not open");
    let help = fontelle_app::settings::SettingRow::Dropouts.help();
    assert!(help.contains("device") && help.contains("late"), "{help}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn asio_offers_the_drivers_own_panel_and_its_own_buffer() {
    let dir = scratch("asio");
    let session = studio_with(
        &dir,
        AudioOutputSettings {
            host: Some(fontelle_engine::ASIO_HELPER_HOST.to_string()),
            ..AudioOutputSettings::default()
        },
    );
    let panel = row(&session, "Driver settings");
    assert_eq!(panel, row(&session, "Output device") + 1);
    let (buffers, chosen) = choice(&session, "Buffer size");
    assert_eq!(chosen, 0);
    assert_eq!(buffers[0], "The driver's own (set in its control panel)");
    std::fs::remove_dir_all(&dir).unwrap();

    // Any other backend has no such row.
    let dir = scratch("no-asio");
    let session = studio(&dir);
    assert!(
        !session
            .settings()
            .iter()
            .any(|r| r.name == "Driver settings"),
        "only under ASIO"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn each_backend_of_fontelles_own_says_what_its_automatic_is() {
    for (host, automatic) in [
        (
            fontelle_engine::WASAPI_LOW_LATENCY_HOST,
            fontelle_engine::LOW_LATENCY_AUTOMATIC,
        ),
        (
            fontelle_engine::WASAPI_EXCLUSIVE_HOST,
            fontelle_engine::EXCLUSIVE_AUTOMATIC,
        ),
    ] {
        let dir = scratch("wasapi");
        let session = studio_with(
            &dir,
            AudioOutputSettings {
                host: Some(host.to_string()),
                ..AudioOutputSettings::default()
            },
        );
        let (buffers, _) = choice(&session, "Buffer size");
        assert!(
            buffers[0].starts_with(&format!("Automatic ({automatic} frames")),
            "{host}: {buffers:?}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

#[test]
fn where_asio_is_offered_steinbergs_mark_is_named_as_theirs() {
    let help = fontelle_app::settings::SettingRow::AudioBackend.help();
    if cfg!(windows) {
        assert!(
            help.contains("ASIO is a registered trademark of Steinberg Media Technologies GmbH"),
            "{help}"
        );
    }
    assert!(fontelle_engine::ASIO_HELPER_HOST.starts_with("ASIO\u{ae}"));
}

#[test]
fn the_word_about_dropouts_names_whose_they_were_and_the_cure_for_that() {
    let late = fontelle_engine::OutputHealth {
        late: 9,
        device_xruns: 1,
        ..Default::default()
    };
    let said = fontelle_app::dropout_words(&late);
    assert!(said.contains("Fontelle was late with 9"), "{said}");
    assert!(said.contains("larger buffer"), "{said}");

    let device = fontelle_engine::OutputHealth {
        late: 0,
        device_xruns: 12,
        ..Default::default()
    };
    let said = fontelle_app::dropout_words(&device);
    assert!(said.contains("device dropped 12"), "{said}");
    assert!(said.contains("another backend"), "{said}");
    assert!(
        !said.contains("larger buffer"),
        "not the cure for this: {said}"
    );
}

//! Steinberg's ASIO Compatible Logo, used as their ASIO Usage Guidelines ask
//! (`docs/asio.md` §1): their own file, unaltered; beside "ASIO" where it is
//! offered and nowhere else; its notice kept apart from Fontelle's licences.
//!
//! One test, because the helper is found through the environment.

mod common;

use std::path::{Path, PathBuf};

use fontelle_ui::StudioHost;
use fontelle_ui::canvas::SettingBadge;
use sha2::{Digest, Sha256};

/// The logo as ASIO SDK 2.3.4 ships it, `Steinberg ASIO Logo Artwork/
/// ASIO-compatible-logo-Steinberg-TM-BW.png`.
const SDK_LOGO_SHA256: &str = "5b5c4aae4baeaed70da70966713e1dbd255977649f37df4c503752f2207a5c86";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[test]
fn the_asio_logo_is_steinbergs_unaltered_and_shown_only_where_asio_is_offered() {
    // The file, and what the program carries, are Steinberg's byte for byte.
    let file = repo().join("assets/trademarks/asio/ASIO-compatible-logo-Steinberg-TM-BW.png");
    assert_eq!(hex(&std::fs::read(&file).unwrap()), SDK_LOGO_SHA256);
    assert_eq!(hex(fontelle_ui::trademarks::ASIO_LOGO_PNG), SDK_LOGO_SHA256);
    let logo = fontelle_ui::trademarks::asio_logo().expect("a debug build carries it");
    assert_eq!(
        (logo.width, logo.height),
        fontelle_ui::trademarks::ASIO_LOGO_SIZE
    );
    assert_eq!((logo.width, logo.height), (474, 443));
    // Not tinted: its black is the file's black, top left.
    assert_eq!(&logo.data.data()[..4], &[12, 12, 12, 255]);

    // Whose it is, said apart from Fontelle's licences.
    let notice = std::fs::read_to_string(repo().join("assets/trademarks/asio/NOTICE")).unwrap();
    assert!(
        notice.contains("ASIO is a registered trademark of Steinberg Media Technologies GmbH.")
    );
    assert!(notice.contains("NOT covered by Fontelle's licences"));
    assert!(notice.contains(SDK_LOGO_SHA256));
    let readme = std::fs::read_to_string(repo().join("README.md")).unwrap();
    assert!(
        readme.contains("assets/trademarks/asio/"),
        "the README's licence section"
    );
    let licences = std::fs::read_to_string(repo().join("licenses/TRADEMARKS.md")).unwrap();
    assert!(licences.contains("assets/trademarks/asio/"));
    let release = std::fs::read_to_string(repo().join(".github/workflows/release.yml")).unwrap();
    assert!(
        release.contains("assets/trademarks/asio"),
        "the Windows zip carries the notice"
    );

    // No helper beside Fontelle: no ASIO, so no logo and no credit.
    let dir = std::env::temp_dir().join(format!("fontelle-asio-tm-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // SAFETY: this binary has one test.
    unsafe { std::env::set_var("FONTELLE_ASIO_HELPER", dir.join("missing")) };
    let session = common::a_session_for(common::a_clip_project(1))
        .with_settings_path(dir.join("settings.json"));
    assert!(session.setting_badges().iter().all(Option::is_none));
    assert!(!session.asio_credit());
    assert!(!session.settings().iter().any(|r| r.name == "ASIO\u{ae}"));
    drop(session);

    // A helper there: beside the backend, and in the credits.
    let helper = dir.join("fontelle-asio");
    std::fs::write(&helper, "a helper").unwrap();
    // SAFETY: as above.
    unsafe { std::env::set_var("FONTELLE_ASIO_HELPER", &helper) };
    let session = common::a_session_for(common::a_clip_project(1))
        .with_settings_path(dir.join("settings.json"));
    assert!(session.asio_credit());
    let rows = session.settings();
    let badges = session.setting_badges();
    let marked: Vec<&str> = rows
        .iter()
        .zip(&badges)
        .filter(|(_, b)| **b == Some(SettingBadge::AsioCompatible))
        .map(|(r, _)| r.name.as_str())
        .collect();
    assert_eq!(marked, ["Backend"]);
    // The attribution, on its own line right under it.
    let at = rows.iter().position(|r| r.name == "Backend").unwrap();
    assert_eq!(rows[at + 1].name, "ASIO\u{ae}");
    let help = session.setting_help();
    assert_eq!(
        help[at + 1],
        "ASIO is a registered trademark of Steinberg Media Technologies GmbH"
    );
    drop(session);

    // With ASIO chosen, the driver's panel row carries it too.
    let settings = fontelle_app::settings::Settings {
        audio_output: fontelle_app::settings::AudioOutputSettings {
            host: Some(fontelle_engine::ASIO_HELPER_HOST.to_string()),
            ..Default::default()
        },
        ..Default::default()
    };
    std::fs::write(dir.join("settings.json"), settings.to_json()).unwrap();
    let session = common::a_session_for(common::a_clip_project(1))
        .with_settings_path(dir.join("settings.json"));
    let rows = session.settings();
    let marked: Vec<&str> = rows
        .iter()
        .zip(&session.setting_badges())
        .filter(|(_, b)| b.is_some())
        .map(|(r, _)| r.name.as_str())
        .collect();
    // Once a page: the guidelines want it in the dialog that configures
    // ASIO, and the driver's row is in the same one.
    assert_eq!(marked, ["Backend"]);
    std::fs::remove_dir_all(&dir).ok();
}

//! The install ping (hub card 0428, `contracts/install-ping.md` in the
//! floptle-platform hub): one anonymous request a day so installs can be
//! counted, said on the start menu before the first one, off by a setting
//! or `FOPULL_NO_PING`.

use fontelle_app::install_ping::{
    Ping, PingAnswer, PingFile, body, due, new_install_id, no_ping_from_env, utc_date,
};

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("fontelle-ping-{}-{name}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_body_is_exactly_the_contracts() {
    let text = body("3f2b9c4e-8a1d-4c6b-9e2f-7d5a1b0c3e48", "0.30.0", "stable");
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    let object = json.as_object().unwrap();
    let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
    keys.sort();
    assert_eq!(
        keys,
        ["arch", "channel", "install_id", "os", "product", "version"]
    );
    assert_eq!(json["product"], "fontelle");
    assert_eq!(json["version"], "0.30.0");
    assert_eq!(json["channel"], "stable");
    assert!(["windows", "macos", "linux", "other"].contains(&json["os"].as_str().unwrap()));
    let arch = json["arch"].as_str().unwrap();
    assert!(!arch.is_empty() && arch.len() <= 16);
    assert!(text.len() <= 2048);
}

#[test]
fn an_install_id_is_a_random_uuid_v4() {
    let a = new_install_id().unwrap();
    let b = new_install_id().unwrap();
    assert_ne!(a, b);
    let parts: Vec<&str> = a.split('-').collect();
    assert_eq!(
        parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
        [8, 4, 4, 4, 12]
    );
    assert!(parts[2].starts_with('4'), "{a}: not version 4");
    assert!("89ab".contains(&parts[3][..1]), "{a}: not the RFC variant");
    assert!(a.chars().all(|c| c == '-' || c.is_ascii_hexdigit()));
}

#[test]
fn the_environment_turns_it_off_with_anything_but_zero() {
    assert!(!no_ping_from_env(None));
    assert!(!no_ping_from_env(Some("")));
    assert!(!no_ping_from_env(Some("0")));
    assert!(no_ping_from_env(Some("1")));
    assert!(no_ping_from_env(Some("yes")));
}

#[test]
fn the_day_is_the_utc_date() {
    let at = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_792_454_400 + 3600);
    assert_eq!(utc_date(at), "2026-10-20");
    assert_eq!(utc_date(std::time::UNIX_EPOCH), "1970-01-01");
}

/// Never before the notice has been shown, never while off, and once a day.
#[test]
fn it_is_due_once_a_day_after_the_notice_and_only_when_on() {
    let file = PingFile {
        install_id: "x".into(),
        last_sent: Some("2026-10-19".into()),
    };
    assert!(due(true, false, Some(&file), "2026-10-20"));
    assert!(!due(true, false, Some(&file), "2026-10-19"), "twice a day");
    assert!(
        !due(false, false, Some(&file), "2026-10-20"),
        "before the notice"
    );
    assert!(!due(true, true, Some(&file), "2026-10-20"), "while off");
    assert!(due(true, false, None, "2026-10-20"), "a first ping");
}

/// A 204 writes the day down; a server error is tried twice more, then left
/// for tomorrow; a 400 or a 429 is not tried again.
#[test]
fn the_answers_are_followed_as_the_contract_says() {
    for (answers, sends, sent) in [
        (vec![204], 1, true),
        (vec![500, 204], 2, true),
        (vec![500, 503, 502], 3, false),
        (vec![400], 1, false),
        (vec![429], 1, false),
    ] {
        let dir = scratch(&format!("answers-{}", answers.len()));
        let path = dir.join("install-ping.json");
        let mut left = answers.clone().into_iter();
        let mut count = 0;
        let mut waits = Vec::new();
        let ping = Ping {
            path: path.clone(),
            version: "0.30.0".into(),
            channel: "stable".into(),
            today: "2026-10-20".into(),
        };
        let answer = ping.run(
            |body| {
                count += 1;
                assert!(body.contains("\"fontelle\""));
                Ok(left.next().unwrap())
            },
            |wait| waits.push(wait),
        );
        assert_eq!(count, sends, "{answers:?}");
        let file = PingFile::read(&path).expect("the id is kept whatever the answer");
        assert_eq!(
            file.last_sent.as_deref() == Some("2026-10-20"),
            sent,
            "{answers:?}"
        );
        if answers.len() > 1 {
            assert!(waits[0] >= std::time::Duration::from_secs(50), "{waits:?}");
        }
        assert_eq!(answer == PingAnswer::Recorded, sent);
        std::fs::remove_dir_all(&dir).ok();
    }
}

/// The id is made once and reused; turning the ping off deletes it.
#[test]
fn the_id_is_kept_and_forgotten_when_turned_off() {
    let dir = scratch("keep");
    let path = dir.join("install-ping.json");
    let ping = |today: &str| Ping {
        path: path.clone(),
        version: "0.30.0".into(),
        channel: "stable".into(),
        today: today.into(),
    };
    let mut ids = Vec::new();
    for today in ["2026-10-20", "2026-10-21"] {
        ping(today).run(
            |body| {
                let json: serde_json::Value = serde_json::from_str(body).unwrap();
                ids.push(json["install_id"].as_str().unwrap().to_string());
                Ok(204)
            },
            |_| {},
        );
    }
    assert_eq!(ids[0], ids[1], "a second day is the same install");
    fontelle_app::install_ping::forget(&path);
    assert!(!path.exists(), "turned off, the id is deleted");
    std::fs::remove_dir_all(&dir).ok();
}

// ------------------------------------------------- the notice and the switch

mod common;

use fontelle_ui::layout::Rect;

type Links = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

fn studio(dir: &std::path::Path, said: bool, on: bool) -> (fontelle_ui::Windowless, Links) {
    let links: Links = Default::default();
    let mut session = common::a_session_for(common::a_clip_project(4))
        .with_settings_path(dir.join("settings.json"))
        .with_links_to(links.clone());
    session.settings_mut().ping_notice_said = said;
    session.settings_mut().install_ping = on;
    let w = fontelle_ui::Windowless::new(
        Box::new(session),
        fontelle_ui::theme::Theme::dark_default(),
        (1280, 720),
    );
    (w, links)
}

fn centre(r: Rect) -> (f32, f32) {
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

/// The first start menu says what is sent and how to stop it, once, and
/// the line opens the privacy page; nothing is said while it is off.
#[test]
fn the_start_menu_says_it_once_before_anything_is_sent() {
    let dir = scratch("notice");
    let (mut w, links) = studio(&dir, false, true);
    w.open_welcome();
    let line = w
        .welcome_layout()
        .and_then(|l| l.support)
        .expect("the notice is on the start menu");
    assert_eq!(
        w.welcome_notice(),
        Some(fontelle_ui::canvas::StartLine::Ping)
    );
    assert!(w.document().ping_notice_said(), "said as it goes up");
    let (x, y) = centre(line);
    w.click_studio(x, y);
    assert_eq!(
        links.lock().unwrap().as_slice(),
        [fontelle_app::install_ping::PRIVACY_URL]
    );

    let (mut w, _) = studio(&dir, true, true);
    w.open_welcome();
    assert_ne!(
        w.welcome_notice(),
        Some(fontelle_ui::canvas::StartLine::Ping)
    );

    let (mut w, _) = studio(&dir, false, false);
    w.open_welcome();
    assert_ne!(
        w.welcome_notice(),
        Some(fontelle_ui::canvas::StartLine::Ping),
        "off, there is nothing to say"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Settings has the switch; turning it off deletes the id beside the
/// settings file.
#[test]
fn the_switch_turns_it_off_and_forgets_the_id() {
    let dir = scratch("switch");
    let (mut w, _) = studio(&dir, true, true);
    let id = dir.join(fontelle_app::install_ping::FILE_NAME);
    std::fs::write(&id, r#"{"install_id":"x","last_sent":null}"#).unwrap();
    w.open_settings("Settings");
    let row = w
        .setting_labels()
        .iter()
        .position(|(name, _)| name == "Count this install")
        .expect("the switch is in Settings");
    w.press_setting(row);
    assert!(!id.exists(), "the id was kept");
    std::fs::remove_dir_all(&dir).ok();
}

/// Against the live endpoint, by hand only — it counts this machine as an
/// install: `cargo test -p fontelle-app --test install_ping -- --ignored`.
#[test]
#[ignore]
fn the_live_endpoint_records_a_ping_through_curl() {
    let id = new_install_id().unwrap();
    let body = body(&id, env!("CARGO_PKG_VERSION"), "stable");
    assert_eq!(fontelle_app::install_ping::send_with_curl(&body), Ok(204));
    assert_eq!(
        fontelle_app::install_ping::send_with_curl(&body),
        Ok(204),
        "a second the same day is a 204 too"
    );
}

// ------------------------------------------------ counted on the first open
//
// Hub card 0645, Ty 2026-11-05: opt out, counted on the first open. Until
// then the first launch only showed the notice and the ping waited for a
// second launch that many never made — 51 fresh downloads, 16 counted. The
// contract (§4, PR #4): the first ping goes in the first launch, about 60 s
// after the notice appears or when the screen showing it is left, whichever
// is first; off in that window means no request.

type Sent = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

/// A first launch: nothing said, the ping on, its requests written down
/// rather than sent, and the notice's grace `grace` long.
fn first_launch(
    dir: &std::path::Path,
    grace: std::time::Duration,
) -> (fontelle_app::Session, Sent) {
    let sent: Sent = Default::default();
    let to = sent.clone();
    let mut session = common::a_session_for(common::a_clip_project(4))
        .with_settings_path(dir.join("settings.json"))
        .with_ping_sender(std::sync::Arc::new(move |body: &str| {
            to.lock().unwrap().push(body.to_string());
            Ok(204)
        }))
        .with_ping_grace(grace);
    session.settings_mut().ping_notice_said = false;
    session.settings_mut().install_ping = true;
    (session, sent)
}

fn window(session: fontelle_app::Session) -> fontelle_ui::Windowless {
    fontelle_ui::Windowless::new(
        Box::new(session),
        fontelle_ui::theme::Theme::dark_default(),
        (1280, 720),
    )
}

/// Turns the loop until `sent` has `n` requests or two seconds pass.
fn wait_for(w: &mut fontelle_ui::Windowless, sent: &Sent, n: usize) {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while sent.lock().unwrap().len() < n && std::time::Instant::now() < until {
        w.idle(std::time::Duration::from_millis(20));
    }
}

#[test]
fn the_first_launch_is_counted_a_moment_after_the_notice() {
    let dir = scratch("first-open");
    let (mut session, sent) = first_launch(&dir, std::time::Duration::from_millis(150));
    // `main` asks at launch, before any menu: nothing said yet, nothing sent.
    session.start_install_ping();
    let mut w = window(session);
    w.idle(std::time::Duration::from_millis(30));
    assert!(
        sent.lock().unwrap().is_empty(),
        "before the notice: nothing"
    );
    w.open_welcome();
    assert!(
        sent.lock().unwrap().is_empty(),
        "not the moment it is shown"
    );
    wait_for(&mut w, &sent, 1);
    let sent = sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 1, "once, in this same launch: {sent:?}");
    assert!(sent[0].contains("\"product\":\"fontelle\""), "{}", sent[0]);
    let file = fontelle_app::install_ping::PingFile::read(
        &dir.join(fontelle_app::install_ping::FILE_NAME),
    )
    .expect("the id is kept");
    assert_eq!(
        file.last_sent.as_deref(),
        Some(fontelle_app::install_ping::utc_date(std::time::SystemTime::now()).as_str())
    );
    // Turning the loop on sends nothing more.
    w.idle(std::time::Duration::from_millis(200));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn leaving_the_start_menu_sends_it_without_waiting_out_the_grace() {
    let dir = scratch("left-menu");
    let (session, sent) = first_launch(&dir, std::time::Duration::from_secs(3600));
    let mut w = window(session);
    w.open_welcome();
    w.idle(std::time::Duration::from_millis(50));
    assert!(sent.lock().unwrap().is_empty());
    w.leave_welcome();
    wait_for(&mut w, &sent, 1);
    assert_eq!(sent.lock().unwrap().len(), 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn turned_off_while_the_notice_is_up_nothing_is_sent() {
    let dir = scratch("off-in-grace");
    let (session, sent) = first_launch(&dir, std::time::Duration::from_millis(150));
    let mut w = window(session);
    w.open_welcome();
    w.open_settings("Settings");
    let row = w
        .setting_labels()
        .iter()
        .position(|(name, _)| name == "Count this install")
        .expect("the switch is in Settings");
    w.press_setting(row);
    w.leave_welcome();
    w.idle(std::time::Duration::from_millis(400));
    assert!(sent.lock().unwrap().is_empty(), "off means no request");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_launch_straight_into_a_project_says_the_notice_too_and_is_counted() {
    let dir = scratch("no-menu");
    let (mut session, sent) = first_launch(&dir, std::time::Duration::from_millis(150));
    // `main`, when no start menu will be shown: the notice, then the
    // launch's ask — which must wait out the notice's grace, not send now.
    session.say_ping_notice_without_menu();
    session.start_install_ping();
    std::thread::sleep(std::time::Duration::from_millis(40));
    assert!(sent.lock().unwrap().is_empty(), "not the moment it is said");
    assert!(fontelle_ui::document::StudioHost::ping_notice_said(
        &session
    ));
    let said = fontelle_ui::document::StudioHost::take_session_notices(&mut session);
    assert!(
        said.iter()
            .any(|s| s.contains("anonymous") && s.contains("Settings")),
        "{said:?}"
    );
    let mut w = window(session);
    wait_for(&mut w, &sent, 1);
    assert_eq!(sent.lock().unwrap().len(), 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn off_there_is_no_notice_without_the_menu_and_nothing_sent() {
    let dir = scratch("no-menu-off");
    let (mut session, sent) = first_launch(&dir, std::time::Duration::from_millis(50));
    session.settings_mut().install_ping = false;
    session.say_ping_notice_without_menu();
    assert!(!fontelle_ui::document::StudioHost::ping_notice_said(
        &session
    ));
    let mut w = window(session);
    w.idle(std::time::Duration::from_millis(300));
    assert!(sent.lock().unwrap().is_empty());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_session_with_no_sender_never_sends() {
    // Only `main` gives the session curl: no test can count itself as an
    // install by showing a start menu.
    let dir = scratch("no-sender");
    let mut session = common::a_session_for(common::a_clip_project(4))
        .with_settings_path(dir.join("settings.json"))
        .with_ping_grace(std::time::Duration::ZERO);
    session.settings_mut().ping_notice_said = false;
    let mut w = window(session);
    w.open_welcome();
    w.leave_welcome();
    w.idle(std::time::Duration::from_millis(200));
    assert!(
        !dir.join(fontelle_app::install_ping::FILE_NAME).exists(),
        "no id even made"
    );
    std::fs::remove_dir_all(&dir).ok();
}

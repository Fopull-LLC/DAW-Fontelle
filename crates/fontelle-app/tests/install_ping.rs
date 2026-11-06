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
        ..PingFile::default()
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
            milestones: Vec::new(),
            used: Vec::new(),
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
        milestones: Vec::new(),
        used: Vec::new(),
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

// ------------------------------------------- milestones and features (§9)
//
// Hub card 0646, Ty 2026-11-05: *"we should measure started using it things
// like opened project played note, used a plugin, collabed with a friend etc.
// so we can see how far people are really getting with the program and if
// they eventually drop it and where they did."* The contract's §9: fixed
// keys only, a milestone sent until a 204 and then never again, today's
// features resent when the set grows, at most 8 sends a day, off sends
// nothing and keeps nothing.

use fontelle_app::install_ping::{Feature, Milestone, Usage};

fn a_ping(path: &std::path::Path, today: &str, milestones: &[&str], used: &[&str]) -> Ping {
    Ping {
        path: path.to_path_buf(),
        version: "0.34.0".into(),
        channel: "stable".into(),
        today: today.into(),
        milestones: milestones.iter().map(|s| s.to_string()).collect(),
        used: used.iter().map(|s| s.to_string()).collect(),
    }
}

/// Runs `ping`, answering `status`, and gives back the one body it sent.
fn run_once(ping: &Ping, status: u16) -> (PingAnswer, serde_json::Value) {
    let mut bodies = Vec::new();
    let answer = ping.run(
        |body| {
            bodies.push(body.to_string());
            Ok(status)
        },
        |_| {},
    );
    assert_eq!(bodies.len(), 1, "{bodies:?}");
    (answer, serde_json::from_str(&bodies[0]).unwrap())
}

fn keys(json: &serde_json::Value, field: &str) -> Vec<String> {
    json.get(field)
        .and_then(|v| v.as_array())
        .map(|a| a.iter().map(|k| k.as_str().unwrap().to_string()).collect())
        .unwrap_or_default()
}

#[test]
fn the_keys_are_the_contracts_table() {
    // contracts/install-ping.md §9, `fontelle`'s row. A key is never
    // renamed once the server lists it, so this list only ever grows.
    let milestones: Vec<&str> = Milestone::ALL.iter().map(|m| m.key()).collect();
    assert_eq!(
        milestones,
        [
            "project_created",
            "project_opened",
            "project_saved",
            "note_played",
            "notes_written",
            "clip_placed",
            "audio_recorded",
            "soundfont_loaded",
            "instrument_edited",
            "plugin_loaded",
            "effect_added",
            "audio_exported",
            "collab_hosted",
            "collab_joined",
            "tour_finished",
            "audio_unavailable",
        ]
    );
    let used: Vec<&str> = Feature::ALL.iter().map(|f| f.key()).collect();
    assert_eq!(
        used,
        [
            "playback",
            "piano_roll",
            "recording",
            "instrument_editor",
            "flopsynth",
            "plugins",
            "effects",
            "mixer",
            "export",
            "collab",
            "vocal",
            "audio_clips",
        ]
    );
    for key in milestones.iter().chain(&used) {
        assert!(
            key.len() <= 32
                && key.starts_with(|c: char| c.is_ascii_lowercase())
                && key
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
            "{key}: not a §1 key"
        );
    }
    // Everything at once still fits the contract's 2048 bytes.
    let all = body_with_everything();
    assert!(all.len() <= 2048, "{} bytes", all.len());
}

fn body_with_everything() -> String {
    let milestones: Vec<String> = Milestone::ALL.iter().map(|m| m.key().into()).collect();
    let used: Vec<String> = Feature::ALL.iter().map(|f| f.key().into()).collect();
    fontelle_app::install_ping::body_with(
        "3f2b9c4e-8a1d-4c6b-9e2f-7d5a1b0c3e48",
        "0.34.0-beta.12",
        "stable",
        &milestones,
        &used,
    )
}

#[test]
fn a_ping_with_nothing_to_report_is_the_old_body_exactly() {
    let dir = scratch("usage-none");
    let path = dir.join(fontelle_app::install_ping::FILE_NAME);
    let (_, json) = run_once(&a_ping(&path, "2026-11-05", &[], &[]), 204);
    assert!(json.get("milestones").is_none(), "{json}");
    assert!(json.get("used").is_none(), "{json}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_milestone_is_sent_once() {
    let dir = scratch("usage-once");
    let path = dir.join(fontelle_app::install_ping::FILE_NAME);
    let (answer, json) = run_once(
        &a_ping(
            &path,
            "2026-11-05",
            &["project_created", "note_played"],
            &[],
        ),
        204,
    );
    assert_eq!(answer, PingAnswer::Recorded);
    assert_eq!(
        keys(&json, "milestones"),
        ["note_played", "project_created"]
    );
    let file = PingFile::read(&path).unwrap();
    assert!(file.milestones.contains("project_created"));
    assert!(file.pending.is_empty(), "{:?}", file.pending);
    // Reached again — another new song, the next day — it is not news.
    let (_, json) = run_once(
        &a_ping(
            &path,
            "2026-11-06",
            &["project_created", "notes_written"],
            &[],
        ),
        204,
    );
    assert_eq!(keys(&json, "milestones"), ["notes_written"]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_milestone_is_reported_again_after_a_refused_send() {
    for refused in [400, 429, 503] {
        let dir = scratch(&format!("usage-refused-{refused}"));
        let path = dir.join(fontelle_app::install_ping::FILE_NAME);
        let ping = a_ping(&path, "2026-11-05", &["plugin_loaded"], &[]);
        let mut sent = 0;
        let answer = ping.run(
            |_| {
                sent += 1;
                Ok(refused)
            },
            |_| {},
        );
        assert_ne!(answer, PingAnswer::Recorded);
        let file = PingFile::read(&path).unwrap();
        assert!(
            file.pending.contains("plugin_loaded"),
            "{refused}: kept to send again: {file:?}"
        );
        assert!(file.milestones.is_empty());
        // The next launch never reached it in memory; the file remembers.
        let (_, json) = run_once(&a_ping(&path, "2026-11-06", &[], &[]), 204);
        assert_eq!(keys(&json, "milestones"), ["plugin_loaded"], "{refused}");
        let file = PingFile::read(&path).unwrap();
        assert!(file.milestones.contains("plugin_loaded"));
        assert!(file.pending.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[test]
fn todays_features_are_carried_and_tomorrow_starts_empty() {
    let dir = scratch("usage-used");
    let path = dir.join(fontelle_app::install_ping::FILE_NAME);
    let (_, json) = run_once(&a_ping(&path, "2026-11-05", &[], &["playback"]), 204);
    assert_eq!(keys(&json, "used"), ["playback"]);
    // A later send the same day, from a later launch that has only seen the
    // mixer so far: the day's set only grows.
    let (_, json) = run_once(&a_ping(&path, "2026-11-05", &[], &["mixer"]), 204);
    assert_eq!(keys(&json, "used"), ["mixer", "playback"]);
    // The next day is a new set.
    let (_, json) = run_once(&a_ping(&path, "2026-11-06", &[], &["effects"]), 204);
    assert_eq!(keys(&json, "used"), ["effects"]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn no_more_than_eight_requests_a_day() {
    let dir = scratch("usage-cap");
    let path = dir.join(fontelle_app::install_ping::FILE_NAME);
    let mut requests = 0;
    for _ in 0..12 {
        a_ping(&path, "2026-11-05", &["note_played"], &[]).run(
            |_| {
                requests += 1;
                Ok(500)
            },
            |_| {},
        );
    }
    assert_eq!(requests, 8, "retries count: every request is one of the 8");
    // A new day, a new 8.
    let (answer, _) = run_once(&a_ping(&path, "2026-11-06", &["note_played"], &[]), 204);
    assert_eq!(answer, PingAnswer::Recorded);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_milestone_waits_out_its_batch_and_features_their_half_hour() {
    use std::time::{Duration, Instant};
    let t0 = Instant::now();
    let mut usage = Usage::default();
    assert!(!usage.wants_send(t0), "nothing reached, nothing to send");
    assert!(usage.reach(Milestone::ProjectCreated, || false, t0));
    assert!(!usage.wants_send(t0 + Duration::from_secs(5)), "batched");
    // A second milestone inside the batch does not push it back.
    assert!(usage.reach(
        Milestone::NotePlayed,
        || false,
        t0 + Duration::from_secs(20)
    ));
    assert!(usage.wants_send(t0 + Duration::from_secs(31)));
    assert!(
        !usage.reach(
            Milestone::NotePlayed,
            || false,
            t0 + Duration::from_secs(32)
        ),
        "the same one twice in a run is reached once"
    );
    usage.sending(t0 + Duration::from_secs(31));
    assert!(!usage.wants_send(t0 + Duration::from_secs(40)));
    // Already acknowledged in an earlier run: not reached at all.
    assert!(!usage.reach(
        Milestone::ProjectSaved,
        || true,
        t0 + Duration::from_secs(41)
    ));
    assert!(!usage.wants_send(t0 + Duration::from_secs(120)));

    // A feature, just after a send: grown, but not before half an hour.
    usage.use_feature(Feature::Mixer, "2026-11-05", t0 + Duration::from_secs(60));
    assert!(!usage.wants_send(t0 + Duration::from_secs(600)));
    assert!(usage.wants_send(t0 + Duration::from_secs(31 + 30 * 60)));
    usage.sending(t0 + Duration::from_secs(31 + 30 * 60));
    // The same feature again is not growth.
    usage.use_feature(Feature::Mixer, "2026-11-05", t0 + Duration::from_secs(4000));
    assert!(!usage.wants_send(t0 + Duration::from_secs(9000)));
    assert_eq!(usage.used(), ["mixer"]);
    // The next UTC day starts a new set.
    usage.use_feature(
        Feature::Export,
        "2026-11-06",
        t0 + Duration::from_secs(9001),
    );
    assert_eq!(usage.used(), ["export"]);
}

#[test]
fn every_edit_named_by_the_counting_exists() {
    // The edits are counted by their wire names; a renamed command must not
    // silently stop being counted.
    for tag in fontelle_app::install_ping::COUNTED_EDITS {
        assert!(
            fontelle_model::wire::Edit::TAGS.contains(tag),
            "{tag} is not an edit"
        );
    }
}

// --------------------------------------------- the same, through a session

/// A studio whose notice has been said and whose ping is on, its requests
/// written down and answered `status`, its batch `batch` long.
fn counted(dir: &std::path::Path, status: u16, batch: std::time::Duration) -> (Session, Sent) {
    let sent: Sent = Default::default();
    let to = sent.clone();
    let mut session = common::a_session_for(common::a_clip_project(4))
        .with_settings_path(dir.join("settings.json"))
        .with_ping_sender(std::sync::Arc::new(move |body: &str| {
            to.lock().unwrap().push(body.to_string());
            Ok(status)
        }))
        .with_ping_grace(std::time::Duration::ZERO)
        .with_usage_batch(batch);
    session.settings_mut().ping_notice_said = true;
    session.settings_mut().install_ping = true;
    (session, sent)
}

use fontelle_app::Session;
use fontelle_ui::document::{DocumentHost, StudioHost};

/// Pumps `session` until `sent` has `n` requests or `within` passes.
fn pump_for(session: &mut Session, sent: &Sent, n: usize, within: std::time::Duration) {
    let until = std::time::Instant::now() + within;
    while sent.lock().unwrap().len() < n && std::time::Instant::now() < until {
        StudioHost::pump(session);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn bodies(sent: &Sent) -> Vec<serde_json::Value> {
    sent.lock()
        .unwrap()
        .iter()
        .map(|b| serde_json::from_str(b).unwrap())
        .collect()
}

fn a_note() -> fontelle_model::Note {
    fontelle_model::Note {
        start: 0,
        length: 480,
        key: 60,
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
    }
}

#[test]
fn what_is_done_before_the_days_ping_goes_with_it() {
    let dir = scratch("usage-session-first");
    let (mut session, sent) = counted(&dir, 204, std::time::Duration::from_millis(50));
    StudioHost::audition_on(&mut session, 60, 100, 0);
    session.start_install_ping();
    pump_for(&mut session, &sent, 1, std::time::Duration::from_secs(2));
    let sent = bodies(&sent);
    assert_eq!(sent.len(), 1);
    assert_eq!(keys(&sent[0], "milestones"), ["note_played"], "{}", sent[0]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_milestone_later_in_the_day_follows_after_its_batch_and_only_once() {
    let dir = scratch("usage-session-later");
    let (mut session, sent) = counted(&dir, 204, std::time::Duration::from_millis(100));
    session.start_install_ping();
    pump_for(&mut session, &sent, 1, std::time::Duration::from_secs(2));
    assert_eq!(sent.lock().unwrap().len(), 1, "the day's ping");
    // A note drawn in the roll: an edit, counted by what it is.
    DocumentHost::edit(
        &mut session,
        fontelle_ui::canvas::RollEdit::Add { note: a_note() },
    );
    StudioHost::pump(&mut session);
    assert_eq!(sent.lock().unwrap().len(), 1, "batched, not sent at once");
    pump_for(&mut session, &sent, 2, std::time::Duration::from_secs(2));
    let all = bodies(&sent);
    assert_eq!(all.len(), 2, "{all:?}");
    assert_eq!(keys(&all[1], "milestones"), ["notes_written"], "{}", all[1]);
    assert!(
        keys(&all[1], "used").contains(&"piano_roll".to_string()),
        "{}",
        all[1]
    );
    // A second note is not a second first.
    DocumentHost::edit(
        &mut session,
        fontelle_ui::canvas::RollEdit::Add { note: a_note() },
    );
    pump_for(
        &mut session,
        &sent,
        3,
        std::time::Duration::from_millis(400),
    );
    assert_eq!(sent.lock().unwrap().len(), 2, "sent once");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_refused_milestone_is_reported_by_the_next_launch() {
    let dir = scratch("usage-session-refused");
    {
        let (mut session, sent) = counted(&dir, 400, std::time::Duration::from_millis(50));
        StudioHost::audition_on(&mut session, 60, 100, 0);
        session.start_install_ping();
        pump_for(&mut session, &sent, 1, std::time::Duration::from_secs(2));
        assert_eq!(keys(&bodies(&sent)[0], "milestones"), ["note_played"]);
        // Refused: no more this run, whatever else is reached.
        session.reached(Milestone::ProjectSaved);
        pump_for(
            &mut session,
            &sent,
            2,
            std::time::Duration::from_millis(400),
        );
        assert_eq!(sent.lock().unwrap().len(), 1, "a 400 stops this run");
    }
    let (mut session, sent) = counted(&dir, 204, std::time::Duration::from_millis(50));
    session.start_install_ping();
    pump_for(&mut session, &sent, 1, std::time::Duration::from_secs(2));
    let sent = bodies(&sent);
    assert_eq!(
        keys(&sent[0], "milestones"),
        ["note_played", "project_saved"],
        "{}",
        sent[0]
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn off_nothing_is_sent_and_nothing_is_kept() {
    let dir = scratch("usage-session-off");
    let (mut session, sent) = counted(&dir, 204, std::time::Duration::ZERO);
    session.settings_mut().install_ping = false;
    StudioHost::audition_on(&mut session, 60, 100, 0);
    session.reached(Milestone::ProjectCreated);
    session.used(Feature::Mixer);
    session.start_install_ping();
    pump_for(
        &mut session,
        &sent,
        1,
        std::time::Duration::from_millis(300),
    );
    assert!(sent.lock().unwrap().is_empty());
    assert!(
        !dir.join(fontelle_app::install_ping::FILE_NAME).exists(),
        "no id, no reported set"
    );
    // Turned on again later in the run: what was done while it was off was
    // not kept for it.
    session.settings_mut().install_ping = true;
    session.start_install_ping();
    pump_for(&mut session, &sent, 1, std::time::Duration::from_secs(2));
    let sent = bodies(&sent);
    assert_eq!(sent.len(), 1);
    assert!(keys(&sent[0], "milestones").is_empty(), "{}", sent[0]);
    assert!(keys(&sent[0], "used").is_empty(), "{}", sent[0]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn turning_it_off_forgets_what_was_waiting() {
    let dir = scratch("usage-session-switch");
    let (mut session, sent) = counted(&dir, 204, std::time::Duration::from_millis(50));
    session.start_install_ping();
    pump_for(&mut session, &sent, 1, std::time::Duration::from_secs(2));
    session.reached(Milestone::ProjectSaved);
    let path = dir.join(fontelle_app::install_ping::FILE_NAME);
    assert!(
        PingFile::read(&path)
            .unwrap()
            .pending
            .contains("project_saved")
    );
    let mut w = window(session);
    w.open_settings("Settings");
    let row = w
        .setting_labels()
        .iter()
        .position(|(name, _)| name == "Count this install")
        .expect("the switch");
    w.press_setting(row);
    assert!(!path.exists());
    w.idle(std::time::Duration::from_millis(400));
    assert_eq!(sent.lock().unwrap().len(), 1, "nothing after off");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_tours_last_step_is_a_milestone() {
    let dir = scratch("usage-tour");
    let (mut session, sent) = counted(&dir, 204, std::time::Duration::from_millis(50));
    session.start_install_ping();
    pump_for(&mut session, &sent, 1, std::time::Duration::from_secs(2));
    let mut w = window(session);
    w.begin_tour();
    let steps = fontelle_ui::canvas::tour_steps().len();
    w.tour_step_to(steps - 2);
    w.idle(std::time::Duration::from_millis(200));
    assert_eq!(sent.lock().unwrap().len(), 1, "not finished yet");
    w.tour_step_to(steps - 1);
    wait_for(&mut w, &sent, 2);
    let all = bodies(&sent);
    assert_eq!(all.len(), 2, "{all:?}");
    assert_eq!(keys(&all[1], "milestones"), ["tour_finished"], "{}", all[1]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_studio_that_could_not_open_its_output_is_reported_by_the_next_launch() {
    // `main`'s failed start: no window, no session, and an exit nothing may
    // delay — so it is written down, not sent, and the next launch says it.
    let dir = scratch("usage-no-audio");
    let path = dir.join(fontelle_app::install_ping::FILE_NAME);
    let mut settings = fontelle_app::settings::Settings {
        install_ping: true,
        ping_notice_said: true,
        ..Default::default()
    };
    // No file yet: nothing is made before the first ping has gone.
    fontelle_app::install_ping::reached_without_a_studio(
        &settings,
        &path,
        Milestone::AudioUnavailable,
    );
    assert!(!path.exists());
    std::fs::write(
        &path,
        r#"{"install_id":"3f2b9c4e-8a1d-4c6b-9e2f-7d5a1b0c3e48"}"#,
    )
    .unwrap();
    settings.install_ping = false;
    fontelle_app::install_ping::reached_without_a_studio(
        &settings,
        &path,
        Milestone::AudioUnavailable,
    );
    assert!(PingFile::read(&path).unwrap().pending.is_empty(), "off");
    settings.install_ping = true;
    fontelle_app::install_ping::reached_without_a_studio(
        &settings,
        &path,
        Milestone::AudioUnavailable,
    );
    let (_, json) = run_once(&a_ping(&path, "2026-11-06", &[], &[]), 204);
    assert_eq!(keys(&json, "milestones"), ["audio_unavailable"]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn saving_a_song_is_a_milestone() {
    let dir = scratch("usage-save");
    let sent: Sent = Default::default();
    let to = sent.clone();
    let mut session =
        common::a_session_in(common::a_clip_project(2), Some(dir.join("Song.fontelle")))
            .with_settings_path(dir.join("settings.json"))
            .with_ping_sender(std::sync::Arc::new(move |body: &str| {
                to.lock().unwrap().push(body.to_string());
                Ok(204)
            }))
            .with_usage_batch(std::time::Duration::from_millis(50));
    session.settings_mut().ping_notice_said = true;
    session.settings_mut().install_ping = true;
    DocumentHost::save(&mut session).unwrap();
    session.start_install_ping();
    pump_for(&mut session, &sent, 1, std::time::Duration::from_secs(2));
    assert_eq!(keys(&bodies(&sent)[0], "milestones"), ["project_saved"]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_second_launch_the_same_day_reports_what_it_reaches() {
    let dir = scratch("usage-second-launch");
    {
        let (mut session, sent) = counted(&dir, 204, std::time::Duration::from_millis(50));
        session.start_install_ping();
        pump_for(&mut session, &sent, 1, std::time::Duration::from_secs(2));
        assert_eq!(sent.lock().unwrap().len(), 1);
    }
    let (mut session, sent) = counted(&dir, 204, std::time::Duration::from_millis(50));
    // Today's first has gone: this launch asks and sends nothing...
    session.start_install_ping();
    pump_for(
        &mut session,
        &sent,
        1,
        std::time::Duration::from_millis(200),
    );
    assert!(sent.lock().unwrap().is_empty());
    // ...until it has news.
    StudioHost::audition_on(&mut session, 60, 100, 0);
    pump_for(&mut session, &sent, 1, std::time::Duration::from_secs(2));
    let all = bodies(&sent);
    assert_eq!(all.len(), 1);
    assert_eq!(keys(&all[0], "milestones"), ["note_played"]);
    std::fs::remove_dir_all(&dir).ok();
}

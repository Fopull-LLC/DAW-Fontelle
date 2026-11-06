//! Help ▸ Report a bug, Request a feature, Support Fontelle, and the start
//! menu's one quiet line — pressed in the window the way a person would
//! (hub card 0427).
//!
//! Ty: *"accessibility is the product; nothing may nag or gate. The line is
//! a courtesy, said once."* So these tests hold the studio to: a link goes
//! where it says with no more than the address carries; the line shows from
//! the fifth launch, once, and never over work a crash or an unsaved song
//! put at risk.

mod common;

use std::sync::{Arc, Mutex};

use fontelle_ui::canvas::{HelpLink, WelcomeLayout};
use fontelle_ui::document::{DocumentHost, StudioHost};
use fontelle_ui::layout::Rect;

fn centre(r: Rect) -> (f32, f32) {
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

type Links = Arc<Mutex<Vec<String>>>;

/// A studio over `session`, its links written down rather than opened.
fn studio(session: fontelle_app::Session) -> (fontelle_ui::Windowless, Links) {
    let links: Links = Arc::default();
    let session = session.with_links_to(Arc::clone(&links));
    let w = fontelle_ui::Windowless::new(
        Box::new(session),
        fontelle_ui::theme::Theme::dark_default(),
        (1280, 720),
    );
    (w, links)
}

fn a_session() -> fontelle_app::Session {
    common::a_session_for(common::a_clip_project(4))
}

/// A session on its `n`th launch, the way `main` counts one.
fn launch(n: u32, said: bool) -> fontelle_app::Session {
    let mut session = a_session();
    session.settings_mut().launches = n - 1;
    session.settings_mut().support_line_said = said;
    // These are about the support line; the install ping's notice, which
    // shares its row, has been said.
    session.settings_mut().ping_notice_said = true;
    session.count_launch();
    session
}

fn plugin(project: &mut fontelle_model::Project, key: fontelle_types::PluginKey, name: &str) {
    use fontelle_model::Command;
    let master = project.mixer.master.expect("a master");
    fontelle_model::AddPluginInsert::new(master, fontelle_types::PluginState::new(key, name))
        .apply(project)
        .expect("an insert goes on the master");
}

// ------------------------------------------------------- what is sent

#[test]
fn the_system_is_named_plainly_with_the_distribution_where_there_is_one() {
    use fontelle_app::desktop::os_label;
    let fedora = "NAME=\"Fedora Linux\"\nVERSION_ID=42\nPRETTY_NAME=\"Fedora Linux 42 (Workstation Edition)\"\nHOME_URL=\"https://fedoraproject.org/\"\n";
    assert_eq!(
        os_label("linux", Some(fedora)),
        "Linux (Fedora Linux 42 (Workstation Edition))"
    );
    assert_eq!(
        os_label("linux", Some("PRETTY_NAME='CachyOS'\n")),
        "Linux (CachyOS)"
    );
    assert_eq!(
        os_label("linux", Some("PRETTY_NAME=Arch Linux\n")),
        "Linux (Arch Linux)"
    );
    assert_eq!(os_label("linux", Some("NAME=x\n")), "Linux");
    assert_eq!(os_label("linux", Some("PRETTY_NAME=\"\"\n")), "Linux");
    assert_eq!(os_label("linux", None), "Linux");
    assert_eq!(os_label("windows", None), "Windows");
    assert_eq!(os_label("macos", None), "macOS");
}

#[test]
fn the_formats_are_the_songs_and_never_a_plugins_name() {
    use fontelle_types::{PluginFormat, PluginKey};
    let mut project = common::a_clip_project(4);
    assert_eq!(fontelle_app::plugin_formats_label(&project), "");
    plugin(
        &mut project,
        PluginKey::new(PluginFormat::Vst3, "secret-vst3-id"),
        "Secret Synth",
    );
    plugin(&mut project, PluginKey::clap("com.u-he.diva"), "Diva");
    plugin(&mut project, PluginKey::clap("com.example.other"), "Other");
    // In the formats' own order, each once.
    assert_eq!(fontelle_app::plugin_formats_label(&project), "CLAP, VST3");

    let session = common::a_session_for(project);
    let env = StudioHost::report_environment(&session);
    assert_eq!(env.plugins, "CLAP, VST3");
    assert_eq!(env.version, env!("CARGO_PKG_VERSION"));
    assert!(!env.os.is_empty());
    for secret in ["Diva", "Secret", "u-he", "secret-vst3-id", "/"] {
        assert!(
            !format!("{}{}", env.os, env.plugins).contains(secret) || secret == "/",
            "{secret} leaked"
        );
    }
    assert!(!env.plugins.contains('/'));
}

// ------------------------------------------------------- the guide's links

#[test]
fn each_link_in_the_guide_opens_its_page() {
    use fontelle_types::{PluginFormat, PluginKey};
    let mut project = common::a_clip_project(4);
    plugin(
        &mut project,
        PluginKey::new(PluginFormat::Lv2, "urn:x"),
        "X",
    );
    let (mut w, links) = studio(common::a_session_for(project));
    w.open_help();
    let layout = w.help_layout();
    let at = |link: HelpLink| {
        layout
            .links
            .iter()
            .find(|(l, _)| *l == link)
            .map(|(_, r)| centre(*r))
            .expect("the link is on the page")
    };
    let version = env!("CARGO_PKG_VERSION");

    let (x, y) = at(HelpLink::ReportBug);
    w.click_studio(x, y);
    let (x, y) = at(HelpLink::RequestFeature);
    w.click_studio(x, y);
    let (x, y) = at(HelpLink::Support);
    w.click_studio(x, y);

    let opened = links.lock().unwrap().clone();
    assert_eq!(opened.len(), 3, "{opened:?}");
    assert!(
        opened[0].starts_with("https://fopull.com/roadmap/fontelle/report?kind=bug&"),
        "{}",
        opened[0]
    );
    assert!(
        opened[1].starts_with("https://fopull.com/roadmap/fontelle/report?kind=feature&"),
        "{}",
        opened[1]
    );
    for report in &opened[..2] {
        assert!(
            report.contains(&format!("env_version={version}")),
            "{report}"
        );
        assert!(report.contains("env_os="), "{report}");
        assert!(report.contains("env_plugin=LV2"), "{report}");
    }
    assert_eq!(
        opened[2],
        "https://fopull.com/support?product=fontelle&utm_source=fontelle-app&utm_medium=help-menu"
    );
    for report in &opened[..2] {
        assert!(
            report.ends_with("&utm_source=fontelle-app&utm_medium=help-menu"),
            "{report}"
        );
    }
}

// --------------------------------------------------------- the quiet line

fn line_of(w: &fontelle_ui::Windowless) -> Option<WelcomeLayout> {
    w.welcome_layout().filter(|l| l.support.is_some())
}

#[test]
fn a_launch_is_counted_and_counting_stops_once_the_line_is_said() {
    let mut session = a_session();
    assert_eq!(session.settings_mut().launches, 0);
    session.count_launch();
    session.count_launch();
    assert_eq!(StudioHost::launch_count(&session), 2);
    session.settings_mut().support_line_said = true;
    session.count_launch();
    assert_eq!(
        StudioHost::launch_count(&session),
        2,
        "nothing left to count for"
    );
}

#[test]
fn the_fourth_launch_says_nothing() {
    let (mut w, _) = studio(launch(4, false));
    w.open_welcome();
    assert!(w.welcome_layout().is_some(), "the menu is up");
    assert!(line_of(&w).is_none());
    assert!(!w.document().support_line_said());
}

#[test]
fn the_fifth_says_it_once_and_marks_it_said() {
    let (mut w, links) = studio(launch(5, false));
    w.open_welcome();
    let l = line_of(&w).expect("the line, on the fifth launch");
    // Said as soon as it is shown, so it never repeats, pressed or not.
    assert!(w.document().support_line_said());

    let (x, y) = centre(l.support.unwrap());
    w.click_studio(x, y);
    assert_eq!(
        links.lock().unwrap().as_slice(),
        [
            "https://fopull.com/support?product=fontelle&utm_source=fontelle-app&utm_medium=start-menu"
        ]
    );
    assert!(
        w.welcome_layout().is_some(),
        "a link does not close the menu"
    );

    let (x, y) = centre(l.support_dismiss.unwrap());
    w.click_studio(x, y);
    assert!(w.welcome_layout().is_some(), "dismissing it is not leaving");
    assert!(line_of(&w).is_none(), "gone");
    assert_eq!(links.lock().unwrap().len(), 1, "the × opens nothing");
}

#[test]
fn the_sixth_after_it_was_said_says_nothing() {
    let (mut w, _) = studio(launch(6, true));
    w.open_welcome();
    assert!(line_of(&w).is_none());
}

#[test]
fn a_fifth_launch_offering_a_recovery_waits_for_the_next_clean_one() {
    let recovery = fontelle_app::crashlog::Recovery {
        backup: std::path::PathBuf::from("/nowhere/autosave.fontelle"),
        home: None,
        name: "Song".to_string(),
    };
    let (mut w, _) = studio(launch(5, false).with_recovery(recovery));
    w.open_welcome();
    assert!(
        w.welcome_layout().unwrap().recover_button.is_some(),
        "the recovery is offered"
    );
    assert!(line_of(&w).is_none(), "and nothing else is said over it");
    assert!(!w.document().support_line_said(), "deferred, not spent");

    let (mut w, _) = studio(launch(6, false));
    w.open_welcome();
    assert!(line_of(&w).is_some(), "the next clean launch says it");
}

#[test]
fn not_over_a_song_with_unsaved_changes() {
    let mut session = launch(5, false);
    DocumentHost::set_tempo(&mut session, 97.0);
    assert!(DocumentHost::is_dirty(&session), "an edit is unsaved");
    let (mut w, _) = studio(session);
    w.open_welcome();
    assert!(line_of(&w).is_none());
    assert!(!w.document().support_line_said());
}

// ---------------------------------------------- the launch after an update
//
// Hub card 0620: *"The moment after an update installs is the one time a
// returning user is reading something from us. The 'updated to X' note gets
// a 'What's new' link and one quiet line under it … At most once every 30
// days, closable, never during playback or recording, never modal."*

/// A session that last ran as `before` and is now launched as `now`, the
/// support line last said `said_days_ago` (or never), the way `main`
/// launches it.
fn updated(before: Option<&str>, now: &str, said_days_ago: Option<u32>) -> fontelle_app::Session {
    let mut session = a_session();
    let today = fontelle_app::settings::today();
    {
        let settings = session.settings_mut();
        settings.last_version = before.map(str::to_string);
        // Past the fifth launch and its line, so only the update speaks.
        settings.launches = 20;
        settings.support_line_said = true;
        settings.support_line_day = said_days_ago.map(|d| today - d);
        settings.ping_notice_said = true;
    }
    session.note_version(now);
    session.count_launch();
    session
}

#[test]
fn a_new_version_is_noticed_once_and_written_down() {
    let mut session = updated(Some("0.33.0"), "0.34.0", None);
    assert!(StudioHost::just_updated(&session));
    assert_eq!(
        session.settings_mut().last_version.as_deref(),
        Some("0.34.0")
    );
    // The next launch of the same version is an ordinary one.
    session.note_version("0.34.0");
    assert!(!StudioHost::just_updated(&session));
}

#[test]
fn the_same_or_an_older_version_is_not_an_update() {
    assert!(!StudioHost::just_updated(&updated(
        Some("0.34.0"),
        "0.34.0",
        None
    )));
    assert!(!StudioHost::just_updated(&updated(
        Some("0.35.0"),
        "0.34.0",
        None
    )));
}

#[test]
fn a_fresh_install_is_not_an_update_but_a_copy_from_before_this_was_kept_is() {
    // Nothing written down and no sign of a start menu ever shown: new here.
    let mut fresh = a_session();
    fresh.note_version("0.34.0");
    assert!(!StudioHost::just_updated(&fresh));
    assert_eq!(fresh.settings_mut().last_version.as_deref(), Some("0.34.0"));
    // 0.33.0 and before kept no version, but they did say the ping's notice:
    // that copy has run before, so this launch is its update.
    let older = updated(None, "0.34.0", None);
    assert!(StudioHost::just_updated(&older));
}

#[test]
fn the_start_menu_after_an_update_offers_whats_new_and_the_quiet_line() {
    let (mut w, links) = studio(updated(Some("0.33.0"), "0.34.0", None));
    w.open_welcome();
    let l = line_of(&w).expect("the quiet line, after an update");
    assert_eq!(
        w.welcome_notice(),
        Some(fontelle_ui::canvas::StartLine::AfterUpdate)
    );
    assert_eq!(
        w.document().days_since_support_line(),
        Some(0),
        "stamped today as it goes up"
    );
    let button = l.update_button.expect("What's new, in the update row");
    let (x, y) = centre(button);
    w.click_studio(x, y);
    let (x, y) = centre(l.support.unwrap());
    w.click_studio(x, y);
    assert_eq!(
        links.lock().unwrap().as_slice(),
        [
            // The releases page (hub card 0665); this window is told no
            // version, so it opens at the newest.
            "https://fopull.com/fontelle/releases?utm_source=fontelle-app&utm_medium=update",
            "https://fopull.com/support?product=fontelle&utm_source=fontelle-app&utm_medium=update",
        ]
    );
    assert!(w.welcome_layout().is_some(), "links do not close the menu");
}

#[test]
fn after_an_update_the_line_waits_a_month_from_the_last_time_it_was_said() {
    let (mut w, _) = studio(updated(Some("0.33.0"), "0.34.0", Some(12)));
    w.open_welcome();
    assert!(line_of(&w).is_none(), "said twelve days ago");
    assert!(
        w.welcome_layout().unwrap().update_button.is_some(),
        "What's new is still offered"
    );

    let (mut w, _) = studio(updated(Some("0.33.0"), "0.34.0", Some(31)));
    w.open_welcome();
    assert!(line_of(&w).is_some(), "a month on");
}

#[test]
fn the_fifth_launch_line_stamps_the_day_too() {
    // So an update the week after the fifth launch does not say it again.
    let (mut w, _) = studio(launch(5, false));
    w.open_welcome();
    assert!(line_of(&w).is_some());
    assert_eq!(w.document().days_since_support_line(), Some(0));
}

#[test]
fn an_ordinary_launch_says_nothing_of_updates() {
    let (mut w, _) = studio(updated(Some("0.34.0"), "0.34.0", None));
    w.open_welcome();
    assert!(line_of(&w).is_none());
}

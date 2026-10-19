//! Help ▸ Report a bug, Request a feature, Support Fontelle, and the start
//! menu's one quiet line — pressed in the window the way a person would
//! (hub card 0427).
//!
//! Ty: *"accessibility is the product; nothing may nag or gate. The line is
//! a courtesy, said once."* So these tests hold the studio to: a link goes
//! where it says with no more than the address carries; the line shows from
//! the tenth launch, once, and never over work a crash or an unsaved song
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
    assert_eq!(opened[2], "https://fopull.com/support?product=fontelle");
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
fn the_ninth_launch_says_nothing() {
    let (mut w, _) = studio(launch(9, false));
    w.open_welcome();
    assert!(w.welcome_layout().is_some(), "the menu is up");
    assert!(line_of(&w).is_none());
    assert!(!w.document().support_line_said());
}

#[test]
fn the_tenth_says_it_once_and_marks_it_said() {
    let (mut w, links) = studio(launch(10, false));
    w.open_welcome();
    let l = line_of(&w).expect("the line, on the tenth launch");
    // Said as soon as it is shown, so it never repeats, pressed or not.
    assert!(w.document().support_line_said());

    let (x, y) = centre(l.support.unwrap());
    w.click_studio(x, y);
    assert_eq!(
        links.lock().unwrap().as_slice(),
        ["https://fopull.com/support?product=fontelle"]
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
fn the_eleventh_after_it_was_said_says_nothing() {
    let (mut w, _) = studio(launch(11, true));
    w.open_welcome();
    assert!(line_of(&w).is_none());
}

#[test]
fn a_tenth_launch_offering_a_recovery_waits_for_the_next_clean_one() {
    let recovery = fontelle_app::crashlog::Recovery {
        backup: std::path::PathBuf::from("/nowhere/autosave.fontelle"),
        home: None,
        name: "Song".to_string(),
    };
    let (mut w, _) = studio(launch(10, false).with_recovery(recovery));
    w.open_welcome();
    assert!(
        w.welcome_layout().unwrap().recover_button.is_some(),
        "the recovery is offered"
    );
    assert!(line_of(&w).is_none(), "and nothing else is said over it");
    assert!(!w.document().support_line_said(), "deferred, not spent");

    let (mut w, _) = studio(launch(11, false));
    w.open_welcome();
    assert!(line_of(&w).is_some(), "the next clean launch says it");
}

#[test]
fn not_over_a_song_with_unsaved_changes() {
    let mut session = launch(10, false);
    DocumentHost::set_tempo(&mut session, 97.0);
    assert!(DocumentHost::is_dirty(&session), "an edit is unsaved");
    let (mut w, _) = studio(session);
    w.open_welcome();
    assert!(line_of(&w).is_none());
    assert!(!w.document().support_line_said());
}

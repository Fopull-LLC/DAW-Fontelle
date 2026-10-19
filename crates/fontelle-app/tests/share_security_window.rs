//! Who may come into a shared song, from the window (Ty, 2026-10-19):
//!
//! > *"removing someone shouldnt block them from the session, that should be
//! > something totally different, and we should try our best to implement
//! > security so that users can be blocked from sessions and stuff like if
//! > there are bad actors and stuff, maybe making password only sessions,
//! > etc. just ensure odd gaps like this dont make it in"*
//!
//! Two (or three) real studios in one process over the in-memory hub, each
//! behind a window with no window: the Share panel's controls pressed where
//! they are drawn, the password typed into the prompt a key at a time, the
//! host's card answered by its buttons, and the settings page's Unblock.

mod common;

use std::path::PathBuf;

use fontelle_app::Session;
use fontelle_app::collab::{CollabOptions, Identity};
use fontelle_net::MemoryHub;
use fontelle_types::PersistentId;
use fontelle_ui::Windowless;
use fontelle_ui::document::SessionRole;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-share-security-{name}-{}",
        std::process::id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("creatable");
    path
}

fn options(name: &str) -> CollabOptions {
    let mut options = CollabOptions::new(name, PersistentId::derived(name));
    options.idle_break = std::time::Duration::ZERO;
    options.hello_again = std::time::Duration::ZERO;
    options
}

fn window(session: Session) -> Windowless {
    Windowless::new(
        Box::new(session),
        fontelle_ui::theme::Theme::dark_default(),
        (1600, 1000),
    )
}

fn centre(r: fontelle_ui::layout::Rect) -> (f32, f32) {
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

struct Rig {
    hub: MemoryHub,
    now: u64,
    host: Windowless,
    others: Vec<Windowless>,
    dir: PathBuf,
}

impl Drop for Rig {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

impl Rig {
    /// Alice shares a saved song (with `password`, when there is one), her
    /// settings in a file of their own.
    fn new(name: &str, password: Option<&str>) -> Rig {
        let dir = scratch(name);
        std::fs::create_dir_all(dir.join("alice")).unwrap();
        let mut host = common::a_session_for(common::a_clip_project(4))
            .with_settings_path(dir.join("alice-settings.json"));
        host.set_projects_dir(Some(dir.join("alice")));
        host.save_as("Song").unwrap();
        let hub = MemoryHub::new();
        let mut alice = options("Alice");
        alice.password = password.map(str::to_string);
        host.share(Box::new(hub.server_endpoint()), alice).unwrap();
        Rig {
            hub,
            now: 0,
            host: window(host),
            others: Vec::new(),
            dir,
        }
    }

    /// `name` starts joining, as `identity` when given; their window is
    /// `self.others[n]`, `n` returned.
    fn join(&mut self, name: &str, identity: Option<Identity>) -> usize {
        let folder = self.dir.join(format!("{name}-{}", self.others.len()));
        std::fs::create_dir_all(&folder).unwrap();
        let mut joiner = common::a_session_for(common::a_clip_project(4));
        joiner.set_projects_dir(Some(folder));
        let mut options = options(name);
        if let Some(identity) = identity {
            options.identity = identity;
        }
        joiner.join(Box::new(self.hub.connect()), options).unwrap();
        self.others.push(window(joiner));
        self.others.len() - 1
    }

    fn tick(&mut self, ticks: usize) {
        for _ in 0..ticks {
            self.now += 1;
            self.hub.set_now(self.now);
            self.host.pump_session();
            for other in &mut self.others {
                other.pump_session();
            }
        }
    }

    /// Takes the copy the join offers, and waits for the song to open.
    fn take_the_song(&mut self, who: usize) {
        for _ in 0..200 {
            self.tick(1);
            if self.others[who].question().is_some() {
                self.others[who].answer_question(0);
            }
            if self.others[who].document().session().role == SessionRole::Joined {
                break;
            }
        }
        assert_eq!(
            self.others[who].document().session().role,
            SessionRole::Joined,
            "{:?}",
            self.others[who].document().session().status
        );
    }

    fn press_panel(&mut self, rect: fontelle_ui::layout::Rect) {
        let (x, y) = centre(rect);
        self.host.click_studio(x, y);
    }
}

/// Everything the prompt shows: its rows and its field.
fn prompt_text(w: &Windowless) -> String {
    let mut all: Vec<String> = w.menu_rows().into_iter().map(|(_, s)| s).collect();
    all.extend(w.prompt_field());
    all.join(" | ")
}

/// The host sets a password from the panel; the prompt shows dots, never
/// the word; the panel says a password is on and never what it is. The
/// next person to join is asked for it — dots again — told when it is
/// wrong, and let in when it is right.
#[test]
fn a_password_set_on_the_panel_is_asked_for_and_never_shown() {
    let mut rig = Rig::new("password", None);
    rig.host.open_share_panel();
    let controls = rig.host.share_panel().controls;
    rig.press_panel(controls[0]);
    assert!(
        prompt_text(&rig.host).to_lowercase().contains("password"),
        "{}",
        prompt_text(&rig.host)
    );
    rig.host.type_into_prompt("s3cret");
    let shown = prompt_text(&rig.host);
    assert!(!shown.contains("s3cret"), "{shown}");
    assert!(shown.contains(&"\u{2022}".repeat(6)), "{shown}");
    rig.host.type_into_prompt("\n");
    let session = rig.host.document().session();
    assert!(session.password_on);
    assert!(!format!("{session:?}").contains("s3cret"));

    let carol = rig.join("Carol", None);
    rig.tick(30);
    let asked = prompt_text(&rig.others[carol]);
    assert!(asked.to_lowercase().contains("password"), "{asked}");
    rig.others[carol].type_into_prompt("nope");
    assert!(!prompt_text(&rig.others[carol]).contains("nope"));
    rig.others[carol].type_into_prompt("\n");
    rig.tick(30);
    let again = prompt_text(&rig.others[carol]);
    assert!(again.contains("isn't right"), "{again}");
    rig.others[carol].type_into_prompt("s3cret\n");
    rig.take_the_song(carol);
}

/// Escape at the password prompt abandons the join, cleanly: the studio is
/// alone again and says why.
#[test]
fn escape_at_the_password_abandons_the_join() {
    let mut rig = Rig::new("password-escape", Some("pw"));
    let bob = rig.join("Bob", None);
    rig.tick(30);
    assert!(
        prompt_text(&rig.others[bob])
            .to_lowercase()
            .contains("password")
    );
    rig.others[bob].type_into_prompt("\u{1b}");
    rig.tick(5);
    let session = rig.others[bob].document().session();
    assert_eq!(session.role, SessionRole::Alone);
    assert!(session.status.contains("cancelled"), "{}", session.status);
    assert!(rig.host.document().session().peers.is_empty());
}

/// *Ask first* on the panel: a newcomer raises a card on the host — their
/// name, Let in (Enter), Block, Turn away (Escape) — and waits, told
/// whom for.
#[test]
fn ask_first_raises_a_card_the_host_answers() {
    let mut rig = Rig::new("ask-first", None);
    rig.host.open_share_panel();
    let controls = rig.host.share_panel().controls;
    rig.press_panel(controls[1]);
    assert!(rig.host.document().session().ask_first);

    let carol = rig.join("Carol", None);
    rig.tick(30);
    let card = rig.host.question().expect("a card");
    assert!(
        card.lines.join(" ").contains("Carol wants to join"),
        "{card:?}"
    );
    assert_eq!(card.buttons, vec!["Let in", "Block", "Turn away"]);
    // Ty, 2026-10-19: letting someone in shouldn't need a click — Enter
    // lets them in, Escape still turns them away.
    assert_eq!(card.default, 0, "Enter is Let in");
    let status = rig.others[carol].document().session().status;
    assert!(
        status.contains("Waiting for Alice to let you in"),
        "{status}"
    );
    rig.host.answer_question(0);
    rig.take_the_song(carol);

    let dave = rig.join("Dave", None);
    rig.tick(30);
    rig.host.answer_question(2);
    rig.tick(30);
    let status = rig.others[dave].document().session().status;
    assert!(status.contains("didn't let you in"), "{status}");
    assert!(rig.host.question().is_none());
}

/// Block on a person's row: they are let go of, written into the settings
/// by name, and kept out of the next session too — until the settings
/// page's Unblock.
#[test]
fn block_from_a_row_lands_in_settings_and_unblock_lets_them_back() {
    let mut rig = Rig::new("block", None);
    let bob_key = Identity::generate();
    let bob = rig.join("Bob", Some(bob_key.clone()));
    rig.tick(20);
    rig.take_the_song(bob);
    rig.host.open_share_panel();
    let row = rig.host.share_panel().rows[0];
    rig.press_panel(row.block);
    rig.tick(20);
    assert_ne!(
        rig.others[bob].document().session().role,
        SessionRole::Joined
    );
    assert!(rig.host.document().session().peers.is_empty());

    let written = std::fs::read_to_string(rig.dir.join("alice-settings.json")).unwrap();
    let hex: String = bob_key
        .public_key()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert!(written.contains(&hex), "the key is in the settings file");

    // Back with the same key: refused, and told only that.
    let again = rig.join("Bob", Some(bob_key.clone()));
    rig.tick(40);
    let status = rig.others[again].document().session().status;
    assert_eq!(status, "You can't join this session.");

    // Settings ▸ Sharing lists him, with Unblock.
    rig.host.open_settings("Sharing");
    let rows = rig.host.setting_labels();
    let at = rows
        .iter()
        .position(|(label, caption)| label == "Bob" && caption == "Unblock")
        .unwrap_or_else(|| panic!("{rows:?}"));
    rig.host.press_setting(at);
    assert!(
        !rig.host
            .setting_labels()
            .iter()
            .any(|(_, caption)| caption == "Unblock")
    );
    let back = rig.join("Bob", Some(bob_key));
    rig.tick(20);
    rig.take_the_song(back);
}

/// *Lock*: nobody new, in a sentence that says so.
#[test]
fn the_lock_keeps_newcomers_out() {
    let mut rig = Rig::new("lock", None);
    rig.host.open_share_panel();
    let controls = rig.host.share_panel().controls;
    rig.press_panel(controls[2]);
    assert!(rig.host.document().session().locked);
    let carol = rig.join("Carol", None);
    rig.tick(40);
    let status = rig.others[carol].document().session().status;
    assert_eq!(status, "This session isn't taking new people right now.");
}

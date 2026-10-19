//! Working on one song together (`docs/collab-plan.md`).
//!
//! **The shape** (§2): the studio that pressed *Share* is the host, and its
//! document is the one truth. Every edit anybody makes is a `Command`
//! already; the host's history becomes a numbered stream of them
//! (`Msg::Applied`), and each joiner applies that stream the way redo applies
//! a command — same ids, same result. A joiner's own edits are applied on its
//! screen at once, sent to the host as proposals, and put back in the host's
//! order if the host ordered something else first. Undo is each person's own.
//!
//! This module is the two machines that keep that true — [`Host`] and
//! [`Joiner`] — over any `fontelle_net::Transport`. They work on the
//! document and history the session hands them and hand back what the
//! session has to do about it ([`Effect`]); the session owns everything else
//! (the graph, the window's revision, opening a bundle).
//!
//! Where this departs from the plan, and why, is the plan's §19.

pub(crate) mod files;
pub mod identity;
pub(crate) mod join;
pub(crate) mod sealed;
pub(crate) mod singers;
#[cfg(test)]
mod tests;

pub use files::{FetchQuestion, Trouble};
pub use identity::Identity;

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use fontelle_model::wire::{AssetEntry, Edit, Msg, PROTOCOL, ProjectHead, SingerNeed, SingerOffer};
use fontelle_model::{Command, History, Project};
use fontelle_net::{Channel, Incoming, PeerId, SERVER, Transport};
use fontelle_types::{ChannelId, PersistentId, PluginState};

use files::{Files, Place};

/// Where a plugin's state sits in the song, by ids every copy shares: a
/// channel's instrument, or an insert by the slot's own id (§5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PluginPlace {
    Channel(ChannelId),
    Insert(PersistentId),
}

impl PluginPlace {
    fn slot_mut(self, song: &mut Project) -> Option<&mut Option<PluginState>> {
        match self {
            PluginPlace::Channel(id) => {
                song.channels.get_mut(id).map(|channel| &mut channel.plugin)
            }
            PluginPlace::Insert(id) => song
                .mixer
                .tracks
                .values_mut()
                .flat_map(|track| track.inserts.iter_mut())
                .find(|insert| insert.id == id)
                .map(|insert| &mut insert.plugin),
        }
    }
}

/// A plugin's state this studio read off its own plugin and wrote into its
/// copy outside any edit (a save, a backup), and the state the host's song
/// had there when it did.
struct KeptHere {
    place: PluginPlace,
    stream: Option<PluginState>,
    kept: PluginState,
}

/// Every file the song names — see `fontelle_model::Project::files`.
pub fn song_files(song: &Project) -> Vec<fontelle_types::AssetRef> {
    song.files()
}

/// What a joiner is told the song needs (§8.2): each file once, by what is
/// in it, with its size and what it is called — its place in the bundle for
/// a collected file, its own name for a soundfont, which goes to the bank.
///
/// Made from the song itself rather than kept beside it, so it can never
/// list a file the song has stopped using or miss one it has started to
/// (§18, F25). Smallest first: the order a joiner fetches in, so the drum
/// hit arrives before the soundfont (§7.2).
pub fn manifest(song: &Project) -> Vec<AssetEntry> {
    let mut entries: Vec<AssetEntry> = Vec::new();
    for file in song.files() {
        if file.content_hash == 0 || entries.iter().any(|e| e.hash == file.content_hash) {
            continue;
        }
        entries.push(files::entry_of(&file));
    }
    entries.sort_by_key(|entry| (entry.size, entry.hash));
    entries
}

/// The song goes over in pieces this big, under the relay's reliable-frame
/// cap in both directions (64 KB peer to host, 128 KB the other way; §8.4).
pub const CHUNK: usize = 48 * 1024;

/// Who somebody is in a session, and how long they wait before letting go of
/// an edit left in the hand.
#[derive(Clone)]
pub struct CollabOptions {
    /// This studio's key pair: what it proves itself with, and what a host
    /// blocks ([`identity`]). A fresh one unless the session hands over the
    /// install's own.
    pub identity: Identity,
    /// The code this join was typed with (a host's is its relay's, asked of
    /// the link). Signed into a joiner's proof and salted into a password's
    /// keys, so neither is good for another session.
    pub code: String,
    /// Keys a host refuses, from the settings: everybody it blocked before.
    pub blocked: Vec<Vec<u8>>,
    /// The password a host shares with, if any. Never sent.
    pub password: Option<String>,
    /// How long a host waits for a connection to say hello and answer its
    /// challenge before letting go of it — long enough for a person to type
    /// a password.
    pub answer_within: Duration,
    /// What the others see this person called.
    pub name: String,
    /// This studio, for the record of what two copies last agreed on (§4.5).
    pub install: PersistentId,
    /// The Fontelle this is. v1 shares only between identical ones (§8.3).
    pub fontelle: String,
    /// Panic when the song's hash on the wire disagrees with this copy,
    /// rather than asking for a fresh one (§5.6). For tests: a drift nobody
    /// notices is the bug this exists to catch.
    pub strict: bool,
    /// How long an edit may sit in the hand, unchanged, before it is let go
    /// of and sent. A drag moves every frame; a typed tempo or a key press has
    /// no mouse-up to end it and would otherwise never leave.
    pub idle_break: Duration,
    /// Files bigger than this are asked about before they are fetched (§7.1,
    /// decision 4: ask, with the size and the minutes, and no cap). A
    /// soundfont is always asked about: it goes into the bank, outside the
    /// song's own folder.
    pub ask_above: u64,
    /// How long a join waits for the host's welcome before saying hello
    /// again. A hello sent while the host was away from the relay — the relay
    /// holds the lobby, and lets the join in — went nowhere, and nothing else
    /// would ever ask again.
    pub hello_again: Duration,
    /// How fast the relay lets this studio send, in bytes a second — what a
    /// fetch's minutes are worked out at (`fontelle_net::Relay::pace`).
    pub pace: u64,
}

impl CollabOptions {
    pub fn new(name: impl Into<String>, install: PersistentId) -> Self {
        Self {
            identity: Identity::generate(),
            code: String::new(),
            blocked: Vec::new(),
            password: None,
            answer_within: Duration::from_secs(120),
            name: name.into(),
            install,
            fontelle: env!("CARGO_PKG_VERSION").to_string(),
            strict: false,
            idle_break: Duration::from_millis(400),
            ask_above: 64 * 1024 * 1024,
            hello_again: Duration::from_secs(5),
            pace: fontelle_net::CLOUD_PACE,
        }
    }
}

impl std::fmt::Debug for CollabOptions {
    // By hand, to leave the password out: a debug print is the last place
    // it should turn up.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CollabOptions")
            .field("name", &self.name)
            .field("install", &self.install)
            .field("identity", &self.identity)
            .field("code", &self.code)
            .field("password", &self.password.as_ref().map(|_| "…"))
            .field("fontelle", &self.fontelle)
            .finish_non_exhaustive()
    }
}

/// What somebody turned away, blocked or kept out of a locked session is
/// told. Blocked people hear [`CANT_JOIN`] and nothing more — never that
/// they were blocked, nor by what.
pub const CANT_JOIN: &str = "You can't join this session.";
pub const LOCKED: &str = "This session isn't taking new people right now.";
/// A wrong password: the joiner may try again, on the same connection, up
/// to [`MOST_TRIES`] times. Both sides are the same build (the version check
/// comes first), so the joiner knows this sentence for the retryable one.
pub const WRONG_PASSWORD: &str = "That password isn't right.";
/// Wrong passwords one connection may give before it is let go of.
pub const MOST_TRIES: u8 = 5;

/// The host's answer to somebody waiting to be let in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Knock {
    LetIn,
    TurnAway,
    Block,
}

/// Somebody blocked: the key they proved, and the name they gave, for the
/// settings' list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockedKey {
    pub key: Vec<u8>,
    pub name: String,
}

/// A join waiting on the person's password: which asking this is, and
/// whether the last one given was wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PasswordAsk {
    pub attempt: u32,
    pub wrong: bool,
}

/// Somebody in the session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Peer {
    pub peer: u16,
    pub name: String,
    /// Which of the theme's clip colours marks them.
    pub colour: u8,
    /// Whether the host refuses their edits (§10.1, F49). Only the host
    /// knows this of everybody; a joiner knows it of itself
    /// ([`Collab::view_only`]).
    pub view_only: bool,
}

/// What a joiner can say to the join's question (§4.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JoinAnswer {
    /// Copy the song into *Shared*.
    Copy,
    /// Replace the copy at this path with the host's, after a backup.
    Update(PathBuf),
    /// Make the copy at this path its own song and copy the host's beside it.
    KeepBoth(PathBuf),
    /// Of several copies, this is the one.
    Pick(PathBuf),
    Cancel,
}

/// How a copy on this machine stands to the host's (§4.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    Same,
    /// Yours has not changed since you last shared; theirs has.
    YouAreBehind,
    /// Theirs has not changed since you last shared; yours has.
    TheyAreBehind,
    BothChanged,
    /// Neither copy remembers sharing with the other.
    Unknown,
}

/// The question a join asks before it writes anything.
#[derive(Clone, Debug)]
pub struct JoinQuestion {
    pub song: String,
    pub from: String,
    /// What the prompt says, a sentence a line.
    pub lines: Vec<String>,
    /// The answers, **the safe one first**, with what each button says.
    pub buttons: Vec<(JoinAnswer, String)>,
    /// Which button Enter presses.
    pub default: usize,
    pub relation: Option<Relation>,
}

/// What the session has to do after the collaboration has had its turn.
pub(crate) enum Effect {
    /// Somebody else's edits changed the document: it is unsaved, and the
    /// studio has to catch up — as far as the edits [`Reach`]: the timeline,
    /// the live values, or the whole graph (which ends every sounding voice).
    /// A copy rebuilt from scratch (a resync, a refusal) reaches the graph.
    ///
    /// [`Reach`]: fontelle_model::wire::Reach
    Changed(fontelle_model::wire::Reach),
    /// The join's copy is on disk: open it, then hand it back to
    /// [`Collab::went_live`].
    Open(PathBuf),
    /// Something to put on the status line.
    Say(String),
    /// A file the song names has arrived whole: load it into what plays it.
    FileArrived(u64),
}

enum Role {
    Host(Box<Host>),
    Joiner(Box<Joiner>),
}

/// One shared session, from this studio's side.
pub(crate) struct Collab {
    /// The link, behind the one place frames are sealed and opened.
    transport: sealed::Sealed,
    options: CollabOptions,
    role: Role,
    /// Sentences for the person, taken by the window (toasts).
    notices: Vec<String>,
    /// Why the session is over, once it is.
    ended: Option<String>,
    /// Whether this studio ended it — Stop sharing, Leave, Cancel — rather
    /// than somebody else or the network. The person who did it needs no
    /// card to tell them.
    ended_here: bool,
    /// The history's generation last seen with an edit in the hand, and
    /// since when — see [`CollabOptions::idle_break`].
    held: Option<(u64, Instant)>,
    /// The song's files on their way in and out (§7.2).
    files: Files,
    /// Where everybody is in the song (phase 2.2).
    presence: Presences,
    /// What everybody has said, oldest first, and how much of it this
    /// person has not looked at (phase 2.3).
    chat: VecDeque<ChatLine>,
    unread: usize,
}

/// A line of the session's chat.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatLine {
    /// Who said it, by their number (the host is 0).
    pub peer: u16,
    pub name: String,
    pub text: String,
    /// Said by this person.
    pub mine: bool,
}

/// The longest line, in characters: a sentence or three, not a document.
pub const CHAT_LONGEST: usize = 500;
/// How many lines a session keeps.
pub const CHAT_KEPT: usize = 200;

/// Where everybody is in the song: this studio's own, as last told and as
/// last sent, and everybody else's by their number.
#[derive(Default)]
struct Presences {
    mine: Option<fontelle_model::wire::Presence>,
    sent: Option<fontelle_model::wire::Presence>,
    sent_at: Option<Instant>,
    count: usize,
    others: BTreeMap<u16, fontelle_model::wire::Presence>,
    /// What arrived while a drag was in the hand, besides whereabouts: kept,
    /// in order, until the hand is empty (F17).
    held: VecDeque<Incoming>,
}

/// How often this studio's whereabouts may go: fifteen times a second is a
/// cursor that moves smoothly enough, and a few hundred bytes a second.
const PRESENCE_EVERY: Duration = Duration::from_millis(1000 / 15);

impl Collab {
    pub fn host(transport: Box<dyn Transport>, options: CollabOptions) -> Self {
        let host = Host {
            blocked: options.blocked.iter().cloned().collect(),
            password: options.password.clone().filter(|p| !p.is_empty()),
            ..Host::default()
        };
        Self {
            transport: sealed::Sealed::new(transport, sealed::End::Host),
            options,
            role: Role::Host(Box::new(host)),
            notices: Vec::new(),
            ended: None,
            ended_here: false,
            held: None,
            files: Files::default(),
            presence: Presences::default(),
            chat: VecDeque::new(),
            unread: 0,
        }
    }

    /// Starts a join: says hello, and waits for the song.
    pub fn join(
        transport: Box<dyn Transport>,
        options: CollabOptions,
        projects_dir: PathBuf,
    ) -> Self {
        let mut transport = sealed::Sealed::new(transport, sealed::End::Joiner);
        let mut joiner = Joiner::new(projects_dir);
        joiner.say_hello(&mut transport, &options);
        Self {
            transport,
            options,
            role: Role::Joiner(Box::new(joiner)),
            notices: Vec::new(),
            ended: None,
            ended_here: false,
            held: None,
            files: Files::default(),
            presence: Presences::default(),
            chat: VecDeque::new(),
            unread: 0,
        }
    }

    /// Whether this studio is the one sharing.
    pub fn is_host(&self) -> bool {
        matches!(self.role, Role::Host(_))
    }

    /// The code to give out, while the relay has one for this share.
    pub fn code(&self) -> Option<String> {
        if !self.is_host() || self.ended.is_some() {
            return None;
        }
        self.transport.lobby_code()
    }

    /// Who is sharing, once the host has said hello.
    pub fn host_name(&self) -> Option<&str> {
        match &self.role {
            Role::Joiner(joiner) if !joiner.host_name.is_empty() => Some(&joiner.host_name),
            _ => None,
        }
    }

    /// Hands the transport what to call when a message lands (F48).
    pub fn set_wake(&mut self, wake: fontelle_net::Wake) {
        self.transport.set_wake(wake);
    }

    /// Sharing, or holding the song open as a joiner — and not over.
    pub fn is_live(&self) -> bool {
        self.ended.is_none()
            && match &self.role {
                Role::Host(_) => true,
                Role::Joiner(joiner) => joiner.stage == Stage::Live,
            }
    }

    pub fn ended(&self) -> Option<&str> {
        self.ended.as_deref()
    }

    /// Where this studio is in the song now; it goes when it has changed,
    /// no faster than [`PRESENCE_EVERY`].
    pub fn set_presence(&mut self, presence: fontelle_model::wire::Presence) {
        self.presence.mine = Some(presence);
    }

    /// Where everybody else is, by their number (the host is 0).
    pub fn presences(&self) -> Vec<(u16, fontelle_model::wire::Presence)> {
        if self.ended.is_some() {
            return Vec::new();
        }
        self.presence
            .others
            .iter()
            .map(|(peer, presence)| (*peer, presence.clone()))
            .collect()
    }

    /// How many times this studio's whereabouts have gone — for the tests.
    #[cfg(test)]
    pub fn presence_sent(&self) -> usize {
        self.presence.count
    }

    /// Sends this studio's whereabouts, when they have changed and the last
    /// went long enough ago.
    fn send_presence(&mut self) {
        let presence = &mut self.presence;
        if presence.mine.is_none() || presence.mine == presence.sent {
            return;
        }
        if presence
            .sent_at
            .is_some_and(|at| at.elapsed() < PRESENCE_EVERY)
        {
            return;
        }
        let mine = presence.mine.clone().expect("just checked");
        let bytes = Msg::Presence {
            peer: 0,
            presence: mine.clone(),
        }
        .to_bytes();
        let to: Vec<PeerId> = match &self.role {
            Role::Host(host) => host
                .peers
                .iter()
                .filter(|(_, p)| p.welcomed)
                .map(|(id, _)| *id)
                .collect(),
            Role::Joiner(joiner) if joiner.stage == Stage::Live => vec![SERVER],
            Role::Joiner(_) => Vec::new(),
        };
        for peer in to {
            self.transport.send(peer, Channel::Unreliable, &bytes);
        }
        presence.sent = Some(mine);
        presence.sent_at = Some(Instant::now());
        presence.count += 1;
    }

    /// What the person numbered `peer` may touch (host only).
    pub fn set_permissions(
        &mut self,
        peer: u16,
        permissions: fontelle_model::wire::Permissions,
    ) -> Result<(), String> {
        let Role::Host(host) = &mut self.role else {
            return Err("only the person sharing the song decides that".into());
        };
        let (id, key) = host
            .peers
            .iter()
            .find(|(_, p)| p.welcomed && p.space == peer)
            .map(|(&id, p)| (id, p.key.clone()))
            .ok_or("that person has left")?;
        host.allowed.insert(key, permissions);
        send(&mut self.transport, id, &Msg::Permissions { permissions });
        Ok(())
    }

    /// What the person numbered `peer` may touch, as the host has it.
    pub fn permissions_of(&self, peer: u16) -> fontelle_model::wire::Permissions {
        match &self.role {
            Role::Host(host) => host
                .peers
                .values()
                .find(|p| p.welcomed && p.space == peer)
                .and_then(|p| host.allowed.get(&p.key).copied())
                .unwrap_or_default(),
            Role::Joiner(_) => Default::default(),
        }
    }

    /// What the host lets this studio touch (a joiner's).
    pub fn my_permissions(&self) -> fontelle_model::wire::Permissions {
        match &self.role {
            Role::Joiner(joiner) => joiner.permissions,
            Role::Host(_) => Default::default(),
        }
    }

    /// The plugins installed here, which anybody may add (host only).
    pub fn set_plugins_here(&mut self, plugins: Vec<fontelle_types::PluginKey>) {
        if let Role::Host(host) = &mut self.role {
            host.plugins_here = plugins;
        }
    }

    /// Says `text` to everybody in the session: trimmed, cut at
    /// [`CHAT_LONGEST`], and nothing at all when it is blank.
    pub fn say(&mut self, text: &str) {
        let text: String = text.trim().chars().take(CHAT_LONGEST).collect();
        if text.is_empty() || self.ended.is_some() {
            return;
        }
        let bytes = Msg::Chat {
            peer: 0,
            text: text.clone(),
        }
        .to_bytes();
        let me = match &mut self.role {
            Role::Host(host) => {
                for (peer, who) in &host.peers {
                    if who.welcomed {
                        self.transport.send(*peer, Channel::Reliable, &bytes);
                    }
                }
                0
            }
            Role::Joiner(joiner) => {
                self.transport.send(SERVER, Channel::Reliable, &bytes);
                joiner.space
            }
        };
        let name = self.options.name.clone();
        keep_line(
            &mut self.chat,
            ChatLine {
                peer: me,
                name,
                text,
                mine: true,
            },
        );
    }

    /// What has been said, oldest first.
    pub fn chat(&self) -> Vec<ChatLine> {
        self.chat.iter().cloned().collect()
    }

    /// How many lines from somebody else have come since [`read_chat`].
    ///
    /// [`read_chat`]: Collab::read_chat
    pub fn chat_unread(&self) -> usize {
        self.unread
    }

    pub fn read_chat(&mut self) {
        self.unread = 0;
    }

    /// Whether somebody else, or the network, ended the session.
    pub fn ended_by_others(&self) -> bool {
        self.ended.is_some() && !self.ended_here
    }

    /// Over, and still sending what was said last (see [`Collab::pump`]).
    pub fn flushing(&self) -> bool {
        self.ended.is_some() && self.transport.backlog() > 0
    }

    /// Waits, for no longer than `at_most`, for an ended session's last
    /// words to go — what closing the window does, so the goodbye is not
    /// taken with the process.
    pub fn flush(&mut self, at_most: Duration) {
        let until = Instant::now() + at_most;
        while self.flushing() && Instant::now() < until {
            let _ = self.transport.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    pub fn question(&self) -> Option<&JoinQuestion> {
        match &self.role {
            Role::Joiner(joiner) if self.ended.is_none() && joiner.answer.is_none() => {
                joiner.question.as_ref()
            }
            _ => None,
        }
    }

    pub fn peers(&self) -> Vec<Peer> {
        match &self.role {
            Role::Host(host) => host
                .peers
                .values()
                .filter(|peer| peer.welcomed)
                .map(|peer| peer.public())
                .collect(),
            Role::Joiner(joiner) => joiner.peers.values().cloned().collect(),
        }
    }

    pub fn take_notices(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notices)
    }

    /// Whether the host refuses this studio's edits (§10.1, F49).
    pub fn view_only(&self) -> bool {
        match &self.role {
            Role::Joiner(joiner) => joiner.view_only,
            Role::Host(_) => false,
        }
    }

    /// The host's *view only* switch on somebody's row (§10.1, F49): their
    /// every proposal is refused while it is on, and they are told.
    pub fn set_view_only(&mut self, peer: u16, view_only: bool) -> Result<(), String> {
        let Role::Host(host) = &mut self.role else {
            return Err("only the person sharing the song decides who edits it".into());
        };
        let (&id, who) = host
            .peers
            .iter_mut()
            .find(|(_, p)| p.welcomed && p.space == peer)
            .ok_or("that person has left")?;
        if who.view_only != view_only {
            who.view_only = view_only;
            send(&mut self.transport, id, &Msg::ViewOnly { view_only });
        }
        Ok(())
    }

    /// The host's *remove* on somebody's row (§10.1, F49): they are told who
    /// did it, and let go of — a kick, and nothing more. The code is
    /// unchanged and anybody who has it can come back, them included (Ty,
    /// 2026-10-19: *"removing someone shouldnt block them from the session,
    /// that should be something totally different"*). Keeping somebody out
    /// is [`block`](Self::block).
    pub fn remove(&mut self, peer: u16, doc: &mut Project) -> Result<(), String> {
        self.kick(peer, doc, false).map(|_| ())
    }

    /// The host's *block* on somebody's row: removed, and the key they
    /// proved refused for the rest of the session — and, once the session
    /// has written it into the settings, every session after. They are told
    /// only what a removal tells them; trying again, only [`CANT_JOIN`].
    pub fn block(&mut self, peer: u16, doc: &mut Project) -> Result<BlockedKey, String> {
        self.kick(peer, doc, true)
    }

    /// Lets the key in again (host only): unblocked in the settings.
    pub fn unblock(&mut self, key: &[u8]) {
        if let Role::Host(host) = &mut self.role {
            host.blocked.remove(key);
        }
    }

    /// The session's password, set, changed, or cleared with `None` (host
    /// only). **Those already in keep the keys they came in with**: every
    /// connection was sealed as it was proven — under its own exchange, and
    /// the password it proved if there was one — and stays so, whatever the
    /// password becomes; nobody is ever in the clear. Only somebody joining
    /// from now on is asked for the new one — and a challenge already out
    /// under the old one is answered with a fresh challenge, not let through.
    pub fn set_password(&mut self, password: Option<String>) {
        if let Role::Host(host) = &mut self.role {
            host.password = password.filter(|p| !p.is_empty());
            host.password_epoch += 1;
        }
    }

    /// Whether the session asks for a password. Never what it is.
    pub fn has_password(&self) -> bool {
        matches!(&self.role, Role::Host(host) if host.password.is_some())
    }

    /// *Ask before letting people in* (host only).
    pub fn set_ask_first(&mut self, on: bool) {
        if let Role::Host(host) = &mut self.role {
            host.ask_first = on;
        }
    }

    pub fn asks_first(&self) -> bool {
        matches!(&self.role, Role::Host(host) if host.ask_first)
    }

    /// *Lock*: nobody new comes in (host only). Those in stay in.
    pub fn set_locked(&mut self, on: bool) {
        if let Role::Host(host) = &mut self.role {
            host.locked = on;
        }
    }

    pub fn is_locked(&self) -> bool {
        matches!(&self.role, Role::Host(host) if host.locked)
    }

    /// Who is waiting to be let in, first first (host only).
    pub fn knocks(&self) -> Vec<String> {
        match &self.role {
            Role::Host(host) => host
                .knocks
                .iter()
                .filter_map(|id| host.peers.get(id))
                .map(|p| p.name.clone())
                .collect(),
            Role::Joiner(_) => Vec::new(),
        }
    }

    /// The host's answer to the first person waiting. It takes effect on the
    /// next turn, which has the song to send; a block is returned at once,
    /// for the settings.
    pub fn answer_knock(&mut self, knock: Knock, _doc: &mut Project) -> Option<BlockedKey> {
        let Role::Host(host) = &mut self.role else {
            return None;
        };
        let id = loop {
            let id = host.knocks.pop_front()?;
            if host.peers.contains_key(&id) {
                break id;
            }
        };
        host.decided.push_back((id, knock));
        let who = &host.peers[&id];
        let blocked = (knock == Knock::Block).then(|| BlockedKey {
            key: who.key.clone(),
            name: who.name.clone(),
        });
        if let Some(blocked) = &blocked {
            host.blocked.insert(blocked.key.clone());
        }
        blocked
    }

    /// The password this join is waiting on, while it is (a joiner's).
    pub fn password_wanted(&self) -> Option<PasswordAsk> {
        match &self.role {
            Role::Joiner(joiner) if self.ended.is_none() && joiner.challenge.is_some() => {
                Some(PasswordAsk {
                    attempt: joiner.asks,
                    wrong: joiner.wrong,
                })
            }
            _ => None,
        }
    }

    /// The person's password for this join. Stretched here and never sent:
    /// what goes is a proof of it, and the connection is sealed under it.
    pub fn give_password(&mut self, password: &str) {
        if let Role::Joiner(joiner) = &mut self.role
            && let Some((nonce, theirs)) = joiner.challenge.take()
        {
            joiner.wrong = false;
            joiner.password = Some(password.to_string());
            joiner.answer(&mut self.transport, &self.options, &nonce, &theirs);
        }
    }

    /// Whose word this join is waiting on, while the host decides.
    pub fn waiting_for_host(&self) -> Option<&str> {
        match &self.role {
            Role::Joiner(joiner) if self.ended.is_none() => joiner.waiting_for.as_deref(),
            _ => None,
        }
    }

    fn kick(&mut self, peer: u16, doc: &mut Project, block: bool) -> Result<BlockedKey, String> {
        let Role::Host(host) = &mut self.role else {
            return Err("only the person sharing the song can remove anybody".into());
        };
        let id = host
            .peers
            .iter()
            .find(|(_, p)| p.welcomed && p.space == peer)
            .map(|(&id, _)| id)
            .ok_or("that person has left")?;
        let who = host.peers.remove(&id).expect("just found");
        // Gone from the room: coming back is a new arrival, asked about
        // again if the host asks, and kept out while the session is locked.
        host.members.remove(&who.key);
        if block {
            host.blocked.insert(who.key.clone());
        }
        self.presence.others.remove(&who.space);
        if let Some(lost) = self.files.relink(id, None) {
            self.notices.push(lost);
        }
        send(
            &mut self.transport,
            id,
            &Msg::Removed {
                by: self.options.name.clone(),
            },
        );
        self.transport.disconnect(id);
        // They part holding the song as it is now (§4.5).
        doc.meta.shared_revision = Some((who.install, doc.sync_hash()));
        host.broadcast(&mut self.transport, &Msg::Left { peer: who.space });
        self.notices.push(if block {
            format!("You blocked {}.", who.name)
        } else {
            format!("You removed {}.", who.name)
        });
        Ok(BlockedKey {
            key: who.key,
            name: who.name,
        })
    }

    /// How far along the file `hash` is, while it is on its way here.
    pub fn fetching(&self, hash: u64) -> Option<f32> {
        self.files.progress(hash)
    }

    /// The singers the song uses and how each is got (F93): the host's own
    /// list, or what the host said.
    pub fn singers(&self) -> &[SingerNeed] {
        match &self.role {
            Role::Host(host) => &host.singers,
            Role::Joiner(joiner) => &joiner.singers,
        }
    }

    /// The host says which singers its song uses, and offers the packed
    /// banks it may send; said to everybody whenever the list changes.
    pub fn offer_singers(&mut self, singers: Vec<SingerNeed>, packs: Vec<(u64, PathBuf)>) {
        let Role::Host(host) = &mut self.role else {
            return;
        };
        for (hash, path) in packs {
            self.files.offer(hash, path);
        }
        if host.singers != singers {
            host.singers = singers;
            host.broadcast(
                &mut self.transport,
                &Msg::Singers {
                    singers: host.singers.clone(),
                },
            );
        }
    }

    /// Fetches the singer `need`, whose terms the person has agreed to.
    pub fn fetch_singer(&mut self, need: &SingerNeed) {
        if let SingerOffer::Send {
            hash,
            size,
            file_name,
            ..
        } = &need.offer
        {
            self.files.want_pack(
                AssetEntry {
                    hash: *hash,
                    size: *size,
                    file_name: file_name.clone(),
                    kind: fontelle_types::AssetKind::Sample,
                },
                SERVER,
            );
        }
    }

    /// Where the packed singer `hash` landed, once it is here whole.
    pub fn singer_pack(&self, hash: u64) -> Option<PathBuf> {
        self.files.pack(hash).map(PathBuf::from)
    }

    /// Why the file `hash` is not here and not coming, if that is so.
    pub fn file_trouble(&self, hash: u64) -> Option<files::Trouble> {
        self.files.trouble(hash)
    }

    /// The files waiting on the person before they are fetched.
    pub fn fetch_question(&self) -> Option<FetchQuestion> {
        self.files.question(self.options.pace)
    }

    pub fn answer_fetch(&mut self, fetch: bool) {
        // Whoever was waiting on this studio for a file it turned down hears
        // that it is not coming, rather than waiting at 0 % for good.
        for (peer, hash) in self.files.answer(fetch) {
            send(&mut self.transport, peer, &Msg::AssetMissing { hash });
        }
    }

    /// Asks again about every file `doc` names that is not here — a "Not
    /// now" used to be for the rest of the session. A joiner asks the host;
    /// a host forgets its refusals, so the next edit naming a file asks
    /// again. The files found here already are handed back, to be loaded.
    pub fn fetch_missing_again(&mut self, doc: &Project, place: &files::Place) -> Vec<u64> {
        self.files.ask_again();
        if !matches!(self.role, Role::Joiner(_)) || self.ended.is_some() {
            return Vec::new();
        }
        let here = place.with(doc);
        let wanted = files::missing(doc, &here);
        self.files
            .want(wanted, SERVER, &here, self.options.ask_above)
    }

    /// The files that have arrived whole, by hash.
    pub fn files_received(&self) -> &[u64] {
        self.files.received()
    }

    /// One turn: what this studio did goes out, and what arrived comes in.
    pub fn pump(&mut self, doc: &mut Project, history: &mut History, place: &Place) -> Vec<Effect> {
        if self.ended.is_some() {
            // What was sent last — the goodbye, the edits before it — may
            // still be waiting for the relay's pace, and goes out only as
            // the link is looked at. Nothing that arrives matters now.
            if self.flushing() {
                let _ = self.transport.poll();
            } else {
                // Then the link is let go of, which closes the connection:
                // the relay hears the host has gone now, not when the studio
                // is next shared or closed.
                self.transport.let_go(Box::new(Closed));
            }
            return Vec::new();
        }
        self.let_go_of_idle_edits(history);
        self.send_presence();
        // What the link has to say for itself — the relay lost, and got
        // back under which code — is the person's to read (F38).
        self.notices.extend(self.transport.take_notices());
        let mut effects = Vec::new();
        let mut turn = Turn {
            transport: &mut self.transport,
            options: &self.options,
            files: &mut self.files,
            place,
            notices: &mut self.notices,
            effects: &mut effects,
            presence: &mut self.presence,
            chat: &mut self.chat,
            unread: &mut self.unread,
        };
        match &mut self.role {
            Role::Host(host) => {
                if let Some(why) = host.pump(&mut turn, doc, history) {
                    self.end(why, history);
                    return effects;
                }
            }
            Role::Joiner(joiner) => {
                if let Some(why) = joiner.pump(&mut turn, doc, history) {
                    self.end(why, history);
                    return effects;
                }
            }
        }
        let ahead = (self.options.pace / 4) as usize;
        self.files
            .pump(&mut self.transport, &place.with(doc), ahead);
        once_changed(effects)
    }

    /// An edit left in the hand with nothing more coming is let go of, so it
    /// is sent (see [`CollabOptions::idle_break`]).
    fn let_go_of_idle_edits(&mut self, history: &mut History) {
        if !history.gesture_in_hand() {
            self.held = None;
            return;
        }
        let generation = history.generation();
        match self.held {
            Some((seen, since))
                if seen == generation && since.elapsed() >= self.options.idle_break =>
            {
                history.break_gesture();
                self.held = None;
            }
            Some((seen, _)) if seen == generation => {}
            _ => self.held = Some((generation, Instant::now())),
        }
    }

    /// Says this studio wrote a plugin's state into its copy, read off its
    /// own plugin and outside any edit — `before` is what was there.
    ///
    /// > *"the drift must never revert the user's latest knob value"*
    ///
    /// A joiner's copy is made again from the host's song whenever an edit
    /// of somebody else's arrives under one of its own, and on a fresh copy;
    /// the song's state of the plugin then went back into it and into the
    /// plugin, and the knob turned in its window here was undone. Kept here,
    /// it is put back for as long as the song has not changed the slot. A
    /// host's copy is the song, and needs nothing.
    pub fn plugin_kept_here(
        &mut self,
        place: PluginPlace,
        before: Option<&PluginState>,
        now: &PluginState,
    ) {
        let Role::Joiner(joiner) = &mut self.role else {
            return;
        };
        if joiner.stage != Stage::Live {
            return;
        }
        match joiner.kept_here.iter_mut().find(|kept| kept.place == place) {
            Some(kept) => kept.kept = now.clone(),
            None => joiner.kept_here.push(KeptHere {
                place,
                stream: before.cloned(),
                kept: now.clone(),
            }),
        }
    }

    /// The answer to the join's question. What it writes happens once the
    /// whole song has arrived.
    pub fn answer(
        &mut self,
        answer: JoinAnswer,
        history: &mut History,
    ) -> Result<Vec<Effect>, String> {
        let Role::Joiner(joiner) = &mut self.role else {
            return Err("only a join asks a question".into());
        };
        match answer {
            JoinAnswer::Cancel => {
                send(&mut self.transport, SERVER, &Msg::Bye);
                self.end("You cancelled the join.".to_string(), history);
                self.ended_here = true;
                Ok(Vec::new())
            }
            JoinAnswer::Pick(path) => {
                let head = joiner.head.clone().ok_or("the song has not arrived yet")?;
                joiner.question = Some(join::question_for(
                    &path,
                    &head,
                    &joiner.host_name,
                    self.options.install,
                    joiner.host_install,
                )?);
                Ok(Vec::new())
            }
            answer => {
                joiner.answer = Some(answer);
                match joiner.finish() {
                    Ok(effect) => Ok(effect.into_iter().collect()),
                    Err(why) => {
                        send(&mut self.transport, SERVER, &Msg::Bye);
                        self.end(why, history);
                        Ok(Vec::new())
                    }
                }
            }
        }
    }

    /// The join's copy is open as `doc`: this studio is in the session now.
    pub fn went_live(
        &mut self,
        doc: &mut Project,
        history: &mut History,
        place: &Place,
    ) -> Vec<Effect> {
        let Role::Joiner(joiner) = &mut self.role else {
            return Vec::new();
        };
        let mut effects = Vec::new();
        let mut turn = Turn {
            transport: &mut self.transport,
            options: &self.options,
            files: &mut self.files,
            place,
            notices: &mut self.notices,
            effects: &mut effects,
            presence: &mut self.presence,
            chat: &mut self.chat,
            unread: &mut self.unread,
        };
        if let Some(why) = joiner.went_live(&mut turn, doc, history) {
            self.end(why, history);
        }
        effects
    }

    /// Leaving: the host stops sharing, a joiner leaves and keeps its copy.
    /// Both write down what the two copies agree on as of now (§4.5).
    pub fn leave(&mut self, doc: &mut Project, history: &mut History) {
        if self.ended.is_some() {
            return;
        }
        // Whatever is still in the hand, and whatever is waiting to go, goes
        // before the goodbye — or the other side never hears of the last
        // thing done, and the two copies part disagreeing about the song.
        history.break_gesture();
        match &mut self.role {
            Role::Host(host) => {
                host.send_mine(&mut self.transport, doc, history);
                for (peer, who) in &host.peers {
                    if who.welcomed {
                        doc.meta.shared_revision = Some((who.install, doc.sync_hash()));
                    }
                    send(&mut self.transport, *peer, &Msg::Bye);
                }
                self.end("You stopped sharing.".to_string(), history);
                self.ended_here = true;
            }
            Role::Joiner(joiner) => {
                if joiner.stage == Stage::Live {
                    joiner.send_mine(&mut self.transport, history);
                    doc.meta.shared_revision = Some((joiner.host_install, doc.sync_hash()));
                }
                send(&mut self.transport, SERVER, &Msg::Bye);
                self.end("You left the session.".to_string(), history);
                self.ended_here = true;
            }
        }
    }

    fn end(&mut self, why: String, history: &mut History) {
        history.close_outbox();
        history.set_mint_space(None);
        self.ended = Some(why);
    }
}

/// What a joiner is told when the host changes what they may touch.
fn permissions_sentence(host: &str, p: fontelle_model::wire::Permissions) -> String {
    let mut kept = Vec::new();
    if !p.mixer {
        kept.push("the mixer");
    }
    if !p.sounds {
        kept.push("instruments' sounds");
    }
    if !p.plugins {
        kept.push("plugins they haven't got");
    }
    if kept.is_empty() {
        format!("{host} has let you change everything again.")
    } else {
        format!("{host} has kept you off {}.", kept.join(", "))
    }
}

/// How a refusal for want of a permission begins, before which one.
const PERMISSION: &str = "permission:";

/// Every file the song uses, by its contents.
fn doc_files(doc: &Project) -> Vec<u64> {
    doc.files().iter().map(|f| f.content_hash).collect()
}

/// Every plugin the song plays, instruments and inserts.
fn doc_plugins(doc: &Project) -> Vec<fontelle_types::PluginKey> {
    let mut keys = Vec::new();
    for channel in doc.channels.values() {
        if channel.instrument == Some(fontelle_types::InstrumentKind::Plugin)
            && let Some(state) = &channel.plugin
        {
            keys.push(state.key.clone());
        }
    }
    for track in doc.mixer.tracks.values() {
        keys.extend(
            track
                .inserts
                .iter()
                .filter_map(|i| i.plugin.as_ref().map(|s| s.key.clone())),
        );
    }
    keys
}

/// Keeps `line`, and no more than [`CHAT_KEPT`] of them.
fn keep_line(chat: &mut VecDeque<ChatLine>, line: ChatLine) {
    chat.push_back(line);
    while chat.len() > CHAT_KEPT {
        chat.pop_front();
    }
}

/// The link of a session that has ended and said its goodbye: nothing
/// goes, nothing comes.
struct Closed;

impl Transport for Closed {
    fn send(&mut self, _: PeerId, _: Channel, _: &[u8]) {}
    fn poll(&mut self) -> Vec<Incoming> {
        Vec::new()
    }
    fn stats(&self, _: PeerId) -> fontelle_net::LinkStats {
        fontelle_net::LinkStats::default()
    }
}

/// `Changed` once, however many edits arrived in the turn: each one is a
/// whole rebuild of the graph in the studio (every soundfont reloaded), and
/// twenty edits landing in one turn were twenty rebuilds back to back — the
/// "laggy" of Ty's report. Where the first one was is where it stays.
fn once_changed(effects: Vec<Effect>) -> Vec<Effect> {
    // One catch-up for the batch, as dear as the dearest edit in it, where
    // the first of them was.
    let dearest = effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Changed(reach) => Some(*reach),
            _ => None,
        })
        .max();
    let mut seen = false;
    effects
        .into_iter()
        .filter_map(|effect| match effect {
            Effect::Changed(_) if seen => None,
            Effect::Changed(_) => {
                seen = true;
                dearest.map(Effect::Changed)
            }
            other => Some(other),
        })
        .collect()
}

/// This studio's solos, carried from `from` onto `to` by id: a solo is each
/// studio's own and never crosses the wire, so a copy rebuilt from the host's
/// has none of its own (Ty, 2026-10-18).
fn keep_solos(from: &Project, to: &mut Project) {
    for (id, track) in to.mixer.tracks.iter_mut() {
        track.solo = from.mixer.tracks.get(id).is_some_and(|t| t.solo);
    }
    for (id, channel) in to.channels.iter_mut() {
        channel.soloed = from.channels.get(id).is_some_and(|c| c.soloed);
    }
    for (id, lane) in to.lanes.iter_mut() {
        lane.soloed = from.lanes.get(id).is_some_and(|l| l.soloed);
    }
}

/// Applies `command` the way the studio that made it did: minting in its
/// author's space (`fontelle_model::minting_in`, F55).
///
/// A command that makes things *inside* what it makes — a cut's two halves'
/// notes, an imported part's notes — mints those afresh on every apply, and
/// minted them wherever the machine applying it happened to: the joiner in
/// its own space, the host in the dense one, a joiner's copy made again in
/// the dense one too. The same note had two ids, the joiner's next move of
/// it was refused by the host ("taken back"), and the host's next edit of it
/// landed on nothing on the joiner's screen (F89). Applied in the author's
/// space everywhere, the same edit makes the same ids everywhere.
fn apply_as(
    author: u16,
    command: &mut dyn Command,
    doc: &mut Project,
) -> Result<(), fontelle_model::CommandError> {
    let space = (author != 0).then_some(author);
    fontelle_model::minting_in(space, || command.apply(doc))
}

/// Fits every take's trim to its block, as the studio's own healing does at
/// each republish — but at the same point of the stream on every copy, so
/// the host's song and a joiner's copy of it agree (F86).
fn healed(doc: &mut Project) {
    fontelle_model::trim::fit_windows(doc);
}

fn send(transport: &mut dyn Transport, to: PeerId, msg: &Msg) {
    transport.send(to, Channel::Reliable, &msg.to_bytes());
}

/// Everything a turn works with besides the document and the history.
struct Turn<'a> {
    transport: &'a mut sealed::Sealed,
    options: &'a CollabOptions,
    files: &'a mut Files,
    place: &'a Place,
    notices: &'a mut Vec<String>,
    effects: &'a mut Vec<Effect>,
    presence: &'a mut Presences,
    chat: &'a mut VecDeque<ChatLine>,
    unread: &'a mut usize,
}

impl Turn<'_> {
    /// What has arrived to be dealt with now. While a drag is in the hand
    /// that is only whereabouts — everything else waits, in order, until it
    /// is empty (F17) — so nobody's cursor freezes while anybody drags.
    fn incoming(&mut self, in_hand: bool) -> Vec<Incoming> {
        let fresh = self.transport.poll();
        if in_hand {
            let mut now = Vec::new();
            for incoming in fresh {
                // By what it says: a relay carries it on its reliable
                // stream (`fontelle_net`'s `Reliably`).
                let whereabouts = matches!(
                    &incoming,
                    Incoming::Message(_, _, bytes)
                        if matches!(Msg::from_bytes(bytes), Ok(Msg::Presence { .. }))
                );
                if whereabouts {
                    now.push(incoming);
                } else {
                    self.presence.held.push_back(incoming);
                }
            }
            return now;
        }
        let mut all: Vec<Incoming> = self.presence.held.drain(..).collect();
        all.extend(fresh);
        all
    }

    /// The file messages, which both sides speak alike (§7.2). `true` when
    /// `msg` was one of them.
    fn file_message(&mut self, from: PeerId, msg: &Msg, doc: &Project) -> bool {
        match msg {
            Msg::AssetRequest { hash } => {
                self.files
                    .asked(self.transport, from, *hash, &self.place.with(doc));
            }
            Msg::AssetChunk { hash, bytes, .. } => self.files.piece(from, *hash, bytes),
            Msg::AssetDone { hash } => match self.files.done(from, *hash, &self.place.with(doc)) {
                Ok(Some(hash)) => self.effects.push(Effect::FileArrived(hash)),
                Ok(None) => {}
                Err(why) => self.notices.push(why),
            },
            Msg::AssetMissing { hash } => {
                if let Some(why) = self.files.missing(self.transport, *hash) {
                    self.notices.push(why);
                }
            }
            _ => return false,
        }
        true
    }

    /// Asks `from` for whatever files `edit` names that are not here, and
    /// loads the ones that are.
    fn want_files_of(&mut self, edit: &Edit, from: PeerId, doc: &Project) {
        let named = files::named_by(edit);
        if !named.is_empty() {
            let here = self
                .files
                .want(named, from, &self.place.with(doc), self.options.ask_above);
            self.effects
                .extend(here.into_iter().map(Effect::FileArrived));
        }
    }
}

/// A drifted copy: a bug report in strict mode, a fresh snapshot otherwise.
fn drifted(options: &CollabOptions, seq: u64, ours: u64, theirs: u64) {
    let line = format!(
        "Fontelle: shared song drifted at edit {seq} (this copy {ours:016x}, host {theirs:016x}); \
         asking for a fresh copy"
    );
    assert!(!options.strict, "{line}");
    eprintln!("{line}");
}

// ================================================================== the host

#[derive(Default)]
struct Host {
    peers: BTreeMap<PeerId, HostPeer>,
    /// The number of the last edit in the one order there is.
    seq: u64,
    /// The last of each join's proposals this host has applied or refused,
    /// by the join's session **and the key that proved it**: what a joiner
    /// back after a drop is told (`Msg::Resumed`), and what a proposal sent
    /// twice is checked against. The session id crosses in the clear before
    /// the proof, so on its own it would let somebody else claim it.
    handled: BTreeMap<(Vec<u8>, PersistentId), u64>,
    /// The space each join was given, so one back after a drop gets its own
    /// — by the same pair, for the same reason.
    spaces: BTreeMap<(Vec<u8>, PersistentId), u16>,
    /// The singers the song uses (F93), as last said.
    singers: Vec<SingerNeed>,
    /// What each studio may touch, by the key it proved, so a return keeps
    /// it — and a modified build naming somebody else's install cannot take
    /// theirs.
    allowed: BTreeMap<Vec<u8>, fontelle_model::wire::Permissions>,
    /// Keys refused: this session's blocks and the settings' (who was
    /// blocked before).
    blocked: std::collections::BTreeSet<Vec<u8>>,
    /// Keys let in during this session and not removed since: one of these
    /// coming back — its link dropped — is not somebody new, to be asked
    /// about again or kept out by the lock.
    members: std::collections::BTreeSet<Vec<u8>>,
    /// The session's password, never sent; bumped `password_epoch` whenever
    /// it changes, so a challenge made under the old one is made again.
    password: Option<String>,
    password_epoch: u64,
    /// The password stretched under a code, kept: stretching is slow on
    /// purpose, and every challenge would pay it.
    stretched: Option<(String, String, std::sync::Arc<identity::PasswordKeys>)>,
    ask_first: bool,
    locked: bool,
    /// Proven newcomers waiting to be let in, first first, and the answers
    /// the host has given that the next turn carries out.
    knocks: VecDeque<PeerId>,
    decided: VecDeque<(PeerId, Knock)>,
    /// The plugins this computer has: one of these is anybody's to add.
    plugins_here: Vec<fontelle_types::PluginKey>,
}

struct HostPeer {
    space: u16,
    name: String,
    install: PersistentId,
    welcomed: bool,
    view_only: bool,
    /// Which join this is (`Msg::Session`), once it has said.
    session: Option<PersistentId>,
    /// The public key it proved; empty until it has.
    key: Vec<u8>,
    /// The challenge it was set and has not answered, and the host's secret
    /// half of the exchange in it — used once, when the answer comes.
    challenge: Option<Challenge>,
    exchange: Option<identity::Exchange>,
    /// Wrong passwords on this connection (see [`MOST_TRIES`]).
    failures: u8,
    /// When it arrived, or was last challenged: what the host's patience
    /// ([`CollabOptions::answer_within`]) is counted from.
    since: Instant,
    /// Proven, and waiting for the host to let it in.
    knocking: bool,
}

/// A challenge set to one connection: the nonce, the host's half of the
/// exchange, the code and password keys it was set under, and which password
/// that was.
#[derive(Clone)]
struct Challenge {
    nonce: Vec<u8>,
    exchange: Vec<u8>,
    code: String,
    keys: Option<std::sync::Arc<identity::PasswordKeys>>,
    epoch: u64,
}

impl HostPeer {
    fn arriving() -> Self {
        HostPeer {
            space: 0,
            name: String::new(),
            install: PersistentId::default(),
            welcomed: false,
            view_only: false,
            session: None,
            key: Vec::new(),
            challenge: None,
            exchange: None,
            failures: 0,
            since: Instant::now(),
            knocking: false,
        }
    }
}

impl Challenge {
    /// What the joiner is sent.
    fn msg(&self) -> Msg {
        Msg::Challenge {
            nonce: self.nonce.clone(),
            password: self.keys.is_some(),
            exchange: self.exchange.clone(),
        }
    }
}

impl HostPeer {
    fn public(&self) -> Peer {
        Peer {
            peer: self.space,
            name: self.name.clone(),
            colour: (self.space % 8) as u8,
            view_only: self.view_only,
        }
    }
}

impl Host {
    /// One turn. `Some` is why the share ended.
    fn pump(
        &mut self,
        turn: &mut Turn,
        doc: &mut Project,
        history: &mut History,
    ) -> Option<String> {
        self.send_mine(turn.transport, doc, history);

        // Nothing from outside while the host has a drag in the hand: an
        // edit applied under it, or a snapshot taken of its middle, would be
        // a document nobody else could reach (F17). Whereabouts are not the
        // document, and come in all the same.
        let in_hand = history.gesture_in_hand();
        // The host's answers to whoever was waiting — not under a drag
        // either, since letting somebody in sends them the song.
        if !in_hand {
            while let Some((peer, knock)) = self.decided.pop_front() {
                self.carry_out(turn, peer, knock, doc);
            }
        }
        self.let_go_of_the_silent(turn);
        for incoming in turn.incoming(in_hand) {
            match incoming {
                // Somebody this host already has, announced again: the
                // host's leg to the relay dropped and is back, and whatever
                // either side sent meanwhile is lost. They are welcomed
                // again and sent the song as it is now (F38).
                Incoming::Connected(peer) if self.peers.contains_key(&peer) => {
                    turn.files.relink(peer, Some(peer));
                    let who = &self.peers[&peer];
                    if who.welcomed {
                        self.welcome(turn, peer, doc);
                    } else if who.knocking {
                        let host = turn.options.name.clone();
                        send(turn.transport, peer, &Msg::Waiting { host });
                    } else if who.challenge.is_some() {
                        // The challenge, or the answer to it, may be what
                        // was lost: a fresh one, which a joiner that knows
                        // its password answers without asking again.
                        self.challenge(turn, peer);
                    }
                }
                Incoming::Connected(peer) => {
                    self.peers.insert(peer, HostPeer::arriving());
                }
                // The relay itself: it ended the lobby — nobody joined it for
                // half an hour, or it refused the host — and the code means
                // nothing now. Said, not hidden (F40).
                Incoming::Disconnected(peer, Some(why)) if peer == SERVER => {
                    return Some(format!(
                        "Your share ended \u{2014} {why}. Press Share for a new code."
                    ));
                }
                Incoming::Disconnected(peer, _) => self.gone(turn, peer, doc, false),
                Incoming::Message(peer, _, bytes) => {
                    let Ok(msg) = Msg::from_bytes(&bytes) else {
                        eprintln!("Fontelle: a message from peer {peer} did not read");
                        continue;
                    };
                    if !turn.file_message(peer, &msg, doc) {
                        self.receive(turn, peer, msg, doc);
                    }
                }
            }
        }
        None
    }

    /// The host's own edits, in the order they landed. The hash goes on the
    /// last: the edits before it are not on their own in the document any
    /// more to be hashed.
    ///
    /// Nor on that one while the next edit is still in the hand. An edit is
    /// sent when another lands on top of it, so the song already holds the
    /// one after it, which nobody else has: hashed then, every such edit
    /// was a drift and a fresh copy — 67 of them in Ty and Lore's session,
    /// the joiner each time one edit behind. The hash goes with the edit
    /// that leaves the hand empty.
    fn send_mine(
        &mut self,
        transport: &mut dyn Transport,
        doc: &mut Project,
        history: &mut History,
    ) {
        let mine = history.take_outgoing();
        let count = mine.len();
        let whole = !history.gesture_in_hand();
        for (i, outgoing) in mine.into_iter().enumerate() {
            self.seq += 1;
            let hash = (i + 1 == count && whole).then(|| {
                healed(doc);
                doc.sync_hash()
            });
            self.broadcast(
                transport,
                &Msg::Applied {
                    seq: self.seq,
                    author: 0,
                    author_seq: 0,
                    edit: outgoing.edit,
                    hash,
                },
            );
        }
    }

    fn receive(&mut self, turn: &mut Turn, peer: PeerId, msg: Msg, doc: &mut Project) {
        match msg {
            Msg::Session { id } => {
                self.peers
                    .entry(peer)
                    .or_insert_with(HostPeer::arriving)
                    .session = Some(id);
            }
            Msg::Hello {
                protocol,
                fontelle,
                name,
                install,
            } => {
                // Said again by a join already in, or waiting on the host,
                // which has had its answer by now.
                if self
                    .peers
                    .get(&peer)
                    .is_some_and(|p| p.welcomed || p.knocking)
                {
                    return;
                }
                let options = turn.options;
                if protocol != PROTOCOL || fontelle != options.fontelle {
                    let reason = version_sentence(&options.name, &options.fontelle, &fontelle);
                    send(turn.transport, peer, &Msg::Refuse { reason });
                    turn.transport.disconnect(peer);
                    self.peers.remove(&peer);
                    let what = if newer(&fontelle, &options.fontelle) {
                        "update from the start menu, then share again"
                    } else {
                        "they need to update"
                    };
                    turn.notices.push(format!(
                        "{name} tried to join with Fontelle {fontelle}, and this is {} \u{2014} \
                         {what}.",
                        options.fontelle
                    ));
                    return;
                }
                // Nobody is anybody yet: the hello only names them. What
                // makes them somebody is answering the challenge — the same
                // one again for a hello said twice, so an answer already on
                // its way still fits it.
                let entry = self.peers.entry(peer).or_insert_with(HostPeer::arriving);
                entry.name = name;
                entry.install = install;
                match entry.challenge.clone() {
                    Some(challenge) => send(turn.transport, peer, &challenge.msg()),
                    None => self.challenge(turn, peer),
                }
            }
            Msg::Answer {
                key,
                signature,
                proof,
                exchange,
            } => self.answered(turn, peer, key, signature, proof, exchange, doc),
            Msg::Propose { local_seq, edit } => {
                let Some(author) = self.peers.get(&peer).filter(|p| p.welcomed) else {
                    return;
                };
                // Sent again after a drop, and had the first time: once is
                // the edit, twice would be a second one.
                if let Some(session) = author.session {
                    let handled = self
                        .handled
                        .entry((author.key.clone(), session))
                        .or_default();
                    if local_seq <= *handled {
                        return;
                    }
                    *handled = local_seq;
                }
                // Refused whatever it is: the joiner has already been told,
                // and takes it back the way any refusal is (§10.1).
                if author.view_only {
                    send(
                        turn.transport,
                        peer,
                        &Msg::Refused {
                            local_seq,
                            reason: "view only".to_string(),
                        },
                    );
                    return;
                }
                let allowed = self.allowed.get(&author.key).copied().unwrap_or_default();
                let author = author.space;
                let mixer = edit.touches_mixer();
                let sound = edit.changes_sound();
                let files_before = (!allowed.sounds).then(|| doc_files(doc));
                let plugins_before = (!allowed.plugins).then(|| doc_plugins(doc));
                // The proposal as it came, ids and all: the joiner minted
                // them in its own space, so they can be nobody else's (F55).
                let mut command = edit.into_command();
                match apply_as(author, command.as_mut(), doc) {
                    Ok(()) => {
                        // What it needed, now it has shown what it does: a
                        // new file or plugin is seen in the song itself, so
                        // no edit can bring one in unnoticed.
                        let new_file = files_before.is_some_and(|before| {
                            doc_files(doc).iter().any(|f| !before.contains(f))
                        });
                        let plugins_here = &self.plugins_here;
                        let new_plugin = plugins_before.is_some_and(|before| {
                            doc_plugins(doc)
                                .iter()
                                .any(|p| !before.contains(p) && !plugins_here.contains(p))
                        });
                        let needed = if !allowed.mixer && mixer {
                            Some("mixer")
                        } else if !allowed.sounds && (sound || new_file) {
                            Some("sounds")
                        } else if !allowed.plugins && new_plugin {
                            Some("plugins")
                        } else {
                            None
                        };
                        if let Some(needed) = needed {
                            let mut back = command.invert();
                            let _ = apply_as(author, back.as_mut(), doc);
                            send(
                                turn.transport,
                                peer,
                                &Msg::Refused {
                                    local_seq,
                                    reason: format!("{PERMISSION}{needed}"),
                                },
                            );
                            return;
                        }
                        healed(doc);
                        self.seq += 1;
                        let applied = command.to_edit();
                        // A file the edit names that is not here yet is the
                        // author's to send (§7.2).
                        turn.want_files_of(&applied, peer, doc);
                        self.broadcast(
                            turn.transport,
                            &Msg::Applied {
                                seq: self.seq,
                                author,
                                author_seq: local_seq,
                                edit: applied.clone(),
                                hash: Some(doc.sync_hash()),
                            },
                        );
                        turn.effects.push(Effect::Changed(applied.reach()));
                    }
                    Err(e) => send(
                        turn.transport,
                        peer,
                        &Msg::Refused {
                            local_seq,
                            reason: e.to_string(),
                        },
                    ),
                }
            }
            Msg::ResyncRequest => self.send_snapshot(turn.transport, peer, doc),
            Msg::Chat { text, .. } => {
                let Some(who) = self.peers.get(&peer).filter(|p| p.welcomed) else {
                    return;
                };
                let text: String = text.trim().chars().take(CHAT_LONGEST).collect();
                if text.is_empty() {
                    return;
                }
                let (space, name) = (who.space, who.name.clone());
                self.broadcast_except(
                    turn.transport,
                    peer,
                    &Msg::Chat {
                        peer: space,
                        text: text.clone(),
                    },
                );
                keep_line(
                    turn.chat,
                    ChatLine {
                        peer: space,
                        name,
                        text,
                        mine: false,
                    },
                );
                *turn.unread += 1;
            }
            Msg::Presence { presence, .. } => {
                let Some(space) = self
                    .peers
                    .get(&peer)
                    .filter(|p| p.welcomed)
                    .map(|p| p.space)
                else {
                    return;
                };
                // Joiners talk only to the host: it passes each one's
                // whereabouts on to everybody else, under their number.
                let bytes = Msg::Presence {
                    peer: space,
                    presence: presence.clone(),
                }
                .to_bytes();
                for (id, who) in &self.peers {
                    if who.welcomed && *id != peer {
                        turn.transport.send(*id, Channel::Unreliable, &bytes);
                    }
                }
                turn.presence.others.insert(space, presence);
            }
            Msg::Bye => self.gone(turn, peer, doc, true),
            _ => {}
        }
    }

    /// Sets `peer` a fresh challenge and sends it: a new nonce and a new
    /// half of the key exchange, under the session's code as the relay has it
    /// now (it can change when the relay is lost and found again) and the
    /// password as it is now. The host's secret half is kept on the peer,
    /// for the answer.
    fn challenge(&mut self, turn: &mut Turn, peer: PeerId) {
        let code = identity::normal_code(
            &turn
                .transport
                .lobby_code()
                .unwrap_or_else(|| turn.options.code.clone()),
        );
        let keys = self.password.clone().map(|password| match &self.stretched {
            Some((p, c, keys)) if *p == password && *c == code => keys.clone(),
            _ => {
                let keys = std::sync::Arc::new(identity::PasswordKeys::derive(&password, &code));
                self.stretched = Some((password, code.clone(), keys.clone()));
                keys
            }
        });
        let exchange = identity::Exchange::new();
        let challenge = Challenge {
            nonce: identity::nonce(),
            exchange: exchange.public().to_vec(),
            code,
            keys,
            epoch: self.password_epoch,
        };
        let entry = self.peers.entry(peer).or_insert_with(HostPeer::arriving);
        entry.since = Instant::now();
        entry.challenge = Some(challenge.clone());
        entry.exchange = Some(exchange);
        send(turn.transport, peer, &challenge.msg());
    }

    /// Tells `peer` why, and lets go of the connection.
    fn turn_away(&mut self, turn: &mut Turn, peer: PeerId, reason: &str) {
        send(
            turn.transport,
            peer,
            &Msg::Refuse {
                reason: reason.to_string(),
            },
        );
        turn.transport.disconnect(peer);
        self.knocks.retain(|id| *id != peer);
        if self.peers.remove(&peer).is_some()
            && let Some(lost) = turn.files.relink(peer, None)
        {
            turn.notices.push(lost);
        }
    }

    /// A joiner's answer to its challenge. In order: it proves the key
    /// (or is let go of); it knows the password (or is asked again, up to
    /// [`MOST_TRIES`] times) — checked before anything else is said, so
    /// somebody blocked learns nothing a stranger without the password
    /// would not; the key is not blocked; the session is not locked to it;
    /// and then it is in, or waiting on the host.
    #[allow(clippy::too_many_arguments)]
    fn answered(
        &mut self,
        turn: &mut Turn,
        peer: PeerId,
        key: Vec<u8>,
        signature: Vec<u8>,
        proof: Option<Vec<u8>>,
        exchange: Vec<u8>,
        doc: &mut Project,
    ) {
        let Some(who) = self.peers.get(&peer).filter(|p| !p.welcomed && !p.knocking) else {
            return;
        };
        let Some(challenge) = who.challenge.clone() else {
            return;
        };
        let name = who.name.clone();
        let host = turn.options.name.clone();
        let not_proven = format!(
            "Your studio couldn't prove who it is to {host} \u{2014} check the code and join \
             again."
        );
        // Signed over both halves of the exchange: one swapped on the way,
        // either way, and this does not verify.
        let message = identity::join_message(
            &challenge.nonce,
            &challenge.code,
            &challenge.exchange,
            &exchange,
        );
        if !identity::verify(&key, &message, &signature) {
            self.turn_away(turn, peer, &not_proven);
            return;
        }
        // Set under a password that has changed since: set again, under
        // the one there is now. Not a wrong guess — nobody guessed.
        if challenge.epoch != self.password_epoch {
            self.challenge(turn, peer);
            return;
        }
        if let Some(keys) = &challenge.keys {
            let right = proof
                .as_deref()
                .is_some_and(|proof| keys.check(&challenge.nonce, &key, proof));
            if !right {
                let entry = self.peers.get_mut(&peer).expect("just found");
                entry.failures += 1;
                if entry.failures >= MOST_TRIES {
                    self.turn_away(
                        turn,
                        peer,
                        &format!(
                            "Too many wrong passwords \u{2014} ask {host} for it, then join \
                             again."
                        ),
                    );
                    turn.notices.push(format!(
                        "{name} gave a wrong password {MOST_TRIES} times and was let go of."
                    ));
                    return;
                }
                send(
                    turn.transport,
                    peer,
                    &Msg::Refuse {
                        reason: WRONG_PASSWORD.to_string(),
                    },
                );
                self.challenge(turn, peer);
                return;
            }
        }
        if self.blocked.contains(&key) {
            self.turn_away(turn, peer, CANT_JOIN);
            turn.notices.push(format!(
                "Somebody you blocked tried to join as \u{201c}{name}\u{201d}, and was kept out."
            ));
            return;
        }
        let member = self.members.contains(&key);
        if self.locked && !member {
            self.turn_away(turn, peer, LOCKED);
            turn.notices
                .push(format!("{name} tried to join; the session is locked."));
            return;
        }
        // Proven: from here on the connection is sealed, both ways (the
        // joiner sealed its side as it answered), under keys from this
        // connection's exchange — and the password's, when there is one.
        let entry = self.peers.get_mut(&peer).expect("just found");
        let Some(shared) = entry.exchange.take().and_then(|ours| ours.agree(&exchange)) else {
            self.turn_away(turn, peer, &not_proven);
            return;
        };
        turn.transport.arm(
            peer,
            &identity::link_keys(&shared, &challenge.nonce, challenge.keys.as_deref()),
        );
        let entry = self.peers.get_mut(&peer).expect("just found");
        entry.key = key;
        entry.challenge = None;
        if self.ask_first && !member {
            entry.knocking = true;
            self.knocks.push_back(peer);
            send(turn.transport, peer, &Msg::Waiting { host });
            turn.notices.push(format!("{name} wants to join"));
            return;
        }
        self.admit(turn, peer, doc);
    }

    /// What the host answered somebody waiting, carried out.
    fn carry_out(&mut self, turn: &mut Turn, peer: PeerId, knock: Knock, doc: &mut Project) {
        if !self.peers.get(&peer).is_some_and(|p| p.knocking) {
            return;
        }
        match knock {
            Knock::LetIn => {
                self.peers.get_mut(&peer).expect("just found").knocking = false;
                self.admit(turn, peer, doc);
            }
            Knock::TurnAway => {
                let reason = format!("{} didn't let you in this time.", turn.options.name);
                self.turn_away(turn, peer, &reason);
            }
            // Blocked when it was answered, so nobody slips in between.
            Knock::Block => self.turn_away(turn, peer, CANT_JOIN),
        }
    }

    /// Connections that never said hello, or never answered the challenge,
    /// once the host has waited [`CollabOptions::answer_within`] for them:
    /// let go of, rather than held open for whoever is holding them.
    /// Somebody waiting on the host is the host's to answer, and waits.
    fn let_go_of_the_silent(&mut self, turn: &mut Turn) {
        let patience = turn.options.answer_within;
        let silent: Vec<PeerId> = self
            .peers
            .iter()
            .filter(|(_, p)| !p.welcomed && !p.knocking && p.since.elapsed() >= patience)
            .map(|(&id, _)| id)
            .collect();
        for peer in silent {
            turn.transport.disconnect(peer);
            self.peers.remove(&peer);
            turn.files.relink(peer, None);
        }
    }

    /// Lets a proven joiner in: a place, the song, and everybody told —
    /// what a hello did before anybody had to prove anything.
    fn admit(&mut self, turn: &mut Turn, peer: PeerId, doc: &mut Project) {
        let Some(who) = self.peers.get(&peer) else {
            return;
        };
        let (key, name, session) = (who.key.clone(), who.name.clone(), who.session);
        // The same join on a new connection — its own link dropped and it
        // joined again before the old one was noticed gone — takes the old
        // one's place rather than being two people. Only if it proved the
        // same key: the session id came in the clear, and is a claim.
        let replaced = session.and_then(|session| {
            let old = self
                .peers
                .iter()
                .find(|(id, p)| {
                    **id != peer && p.welcomed && p.session == Some(session) && p.key == key
                })
                .map(|(&id, _)| id)?;
            turn.transport.disconnect(old);
            turn.files.relink(old, Some(peer));
            self.peers.remove(&old)
        });
        let free = |space: u16, peers: &BTreeMap<PeerId, HostPeer>| {
            peers.values().all(|p| !p.welcomed || p.space != space)
        };
        let space = replaced
            .as_ref()
            .map(|old| old.space)
            .or_else(|| {
                session
                    .and_then(|session| self.spaces.get(&(key.clone(), session)).copied())
                    .filter(|&space| free(space, &self.peers))
            })
            .or_else(|| (1..=u16::from(u8::MAX)).find(|&space| free(space, &self.peers)))
            .unwrap_or(u16::from(u8::MAX));
        let entry = self.peers.get_mut(&peer).expect("just found");
        entry.space = space;
        entry.welcomed = true;
        entry.knocking = false;
        entry.view_only = replaced.as_ref().is_some_and(|old| old.view_only);
        if let Some(session) = session {
            self.spaces.insert((key.clone(), session), space);
        }
        self.members.insert(key);
        self.welcome(turn, peer, doc);
        if replaced.is_some() {
            return;
        }
        let public = self.peers[&peer].public();
        self.broadcast_except(
            turn.transport,
            peer,
            &Msg::Joined {
                peer: public.peer,
                name: public.name,
                colour: public.colour,
            },
        );
        turn.notices.push(format!("{name} joined"));
        turn.effects.push(Effect::Say(format!("{name} joined")));
    }

    /// Somebody has left — said goodbye, or dropped.
    fn gone(&mut self, turn: &mut Turn, peer: PeerId, doc: &mut Project, said_goodbye: bool) {
        let Some(who) = self.peers.remove(&peer) else {
            return;
        };
        self.knocks.retain(|id| *id != peer);
        turn.transport.disarm(peer);
        if let Some(lost) = turn.files.relink(peer, None) {
            turn.notices.push(lost);
        }
        if !who.welcomed {
            return;
        }
        turn.presence.others.remove(&who.space);
        // What the two copies agree on as they part (§4.5).
        if said_goodbye {
            doc.meta.shared_revision = Some((who.install, doc.sync_hash()));
        }
        self.broadcast(turn.transport, &Msg::Left { peer: who.space });
        turn.notices.push(format!("{} left", who.name));
        turn.effects.push(Effect::Say(format!("{} left", who.name)));
    }

    /// Everything a joiner needs to be in the session as it is now: who it
    /// is, the song, which of its edits are in it, who else is here and
    /// whether it may edit. The first time, or again after a drop on either
    /// side, when anything sent meanwhile is lost.
    fn welcome(&self, turn: &mut Turn, peer: PeerId, doc: &Project) {
        let Some(who) = self.peers.get(&peer) else {
            return;
        };
        let options = turn.options;
        send(
            turn.transport,
            peer,
            &Msg::Welcome {
                peer: who.space,
                protocol: PROTOCOL,
                fontelle: options.fontelle.clone(),
                host: options.name.clone(),
                host_install: options.install,
                project: head_of(doc),
                manifest: manifest(doc),
            },
        );
        let handled = who
            .session
            .and_then(|session| self.handled.get(&(who.key.clone(), session)).copied())
            .unwrap_or(0);
        send(turn.transport, peer, &Msg::Resumed { handled });
        self.send_snapshot(turn.transport, peer, doc);
        for other in self
            .peers
            .iter()
            .filter(|(id, p)| **id != peer && p.welcomed)
            .map(|(_, p)| p.public())
        {
            send(
                turn.transport,
                peer,
                &Msg::Joined {
                    peer: other.peer,
                    name: other.name,
                    colour: other.colour,
                },
            );
        }
        // Where everybody already is: somebody holding still says nothing
        // more, and a newcomer would not see them until they moved.
        let here = turn.presence.sent.iter().map(|p| (0, p));
        let others = turn
            .presence
            .others
            .iter()
            .filter(|(space, _)| **space != who.space)
            .map(|(space, p)| (*space, p));
        for (space, presence) in here.chain(others) {
            let bytes = Msg::Presence {
                peer: space,
                presence: presence.clone(),
            }
            .to_bytes();
            turn.transport.send(peer, Channel::Unreliable, &bytes);
        }
        if who.view_only {
            send(turn.transport, peer, &Msg::ViewOnly { view_only: true });
        }
        if let Some(&permissions) = self.allowed.get(&who.key) {
            send(turn.transport, peer, &Msg::Permissions { permissions });
        }
        if !self.singers.is_empty() {
            send(
                turn.transport,
                peer,
                &Msg::Singers {
                    singers: self.singers.clone(),
                },
            );
        }
    }

    fn send_snapshot(&self, transport: &mut dyn Transport, peer: PeerId, doc: &Project) {
        let bytes = serde_json::to_vec(doc).expect("a project always writes");
        let pieces: Vec<&[u8]> = bytes.chunks(CHUNK).collect();
        let of = pieces.len() as u32;
        for (index, piece) in pieces.into_iter().enumerate() {
            send(
                transport,
                peer,
                &Msg::SnapshotChunk {
                    index: index as u32,
                    of,
                    bytes: piece.to_vec(),
                },
            );
        }
        send(
            transport,
            peer,
            &Msg::SnapshotDone {
                hash: doc.sync_hash(),
                seq: self.seq,
            },
        );
    }

    fn broadcast(&self, transport: &mut dyn Transport, msg: &Msg) {
        let bytes = msg.to_bytes();
        for (peer, who) in &self.peers {
            if who.welcomed {
                transport.send(*peer, Channel::Reliable, &bytes);
            }
        }
    }

    fn broadcast_except(&self, transport: &mut dyn Transport, except: PeerId, msg: &Msg) {
        let bytes = msg.to_bytes();
        for (peer, who) in &self.peers {
            if who.welcomed && *peer != except {
                transport.send(*peer, Channel::Reliable, &bytes);
            }
        }
    }
}

/// What a person is told when an edit they made was taken back because
/// somebody else got there first (§5.5). The reason the command gave is for
/// the log: it names ids, and a person needs to know *which* edit, not why
/// in the document's own words.
fn taken_back(label: &str, host: &str) -> String {
    format!("\u{201c}{label}\u{201d} was taken back \u{2014} {host} had changed it first.")
}

/// What a refused joiner is told (§8.3): who has which, and who updates —
/// whoever has the older one.
fn version_sentence(host: &str, hosts: &str, yours: &str) -> String {
    let what = if newer(yours, hosts) {
        format!("{host} needs to update from the start menu before you can join")
    } else {
        "update from the start menu, then join again".to_string()
    };
    format!("{host} has Fontelle {hosts} and you have {yours} \u{2014} {what}.")
}

/// Whether version `a` is newer than `b`, number by number (`0.10.0` is
/// newer than `0.9.0`).
fn newer(a: &str, b: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> {
        v.split(['.', '-'])
            .map_while(|part| part.parse().ok())
            .collect()
    };
    parts(a) > parts(b)
}

fn head_of(doc: &Project) -> ProjectHead {
    ProjectHead {
        id: doc.meta.id,
        name: doc.meta.name.clone(),
        saved_revision: doc.meta.saved_revision,
        saved_at: doc.meta.saved_at.clone(),
        saved_by: doc.meta.saved_by.clone(),
        shared_revision: doc.meta.shared_revision,
        hash: doc.sync_hash(),
    }
}

// ================================================================ the joiner

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// Hello sent; nothing back yet.
    Waiting,
    /// Welcomed: the song is arriving and the question is up.
    Asking,
    /// The copy is on disk and being opened.
    Opening,
    Live,
}

/// One of this studio's edits, shown here and not yet in the host's order.
struct Pending {
    local_seq: u64,
    /// The history entry it came from, so a refusal can take it back.
    entry: u64,
    /// The edit as applied here, ready to be applied again when the
    /// document is rebuilt under it.
    command: Box<dyn Command>,
    /// Whether it is on the screen. One that no longer applies to the
    /// host's song as this studio has it is left off — and kept, because it
    /// is still on its way: the host decides, and its answer (or refusal)
    /// is what takes it back, not a guess here. It used to be dropped,
    /// its undo entry and all, and said to be taken back; then the host,
    /// whose song had moved on again, applied it after all, and nothing
    /// here ever showed it — a clip deleted on the joiner's screen came
    /// back there and stayed.
    shown: bool,
}

struct Joiner {
    stage: Stage,
    projects_dir: PathBuf,
    /// Which join this is, for as long as the song is open (`Msg::Session`).
    session: PersistentId,
    /// When this join last said hello.
    hello_at: Instant,
    /// Where this studio mints, and who it is in the host's messages.
    space: u16,
    host_name: String,
    host_install: PersistentId,
    head: Option<ProjectHead>,
    question: Option<JoinQuestion>,
    answer: Option<JoinAnswer>,
    snapshot: Vec<u8>,
    snapshot_done: Option<(u64, u64)>,
    /// What the host sent after the snapshot and before the copy was open.
    backlog: Vec<Msg>,
    /// The files the song uses, as the host listed them.
    manifest: Vec<AssetEntry>,
    /// **The host's song, exactly**: every edit in the host's order and
    /// nothing of this studio's that the host has not ordered yet. The
    /// document on screen is this plus `pending`, and it is rebuilt from here
    /// rather than unwound — an edit's remembered "previous" goes stale the
    /// moment somebody else changes the same thing, and unwinding through a
    /// stale one puts back a state that never existed (§19).
    confirmed: Option<Project>,
    seq: u64,
    pending: VecDeque<Pending>,
    next_local: u64,
    /// The last of this studio's own edits the host has answered — applied
    /// or refused, one by one in its order, or as a number after a drop
    /// (`Msg::Resumed`). Nothing at or below it is pending, whatever else
    /// went wrong: the host's song has it, or has refused it (F87).
    answered: u64,
    /// A fresh copy has been asked for and has not arrived: the copy here
    /// is known not to be the host's, and asking again at every edit that
    /// lands on it meanwhile only sends the whole song again (F88).
    resyncing: bool,
    peers: BTreeMap<u16, Peer>,
    /// The host refuses this studio's edits (F49).
    view_only: bool,
    /// What the host lets this studio touch besides the notes.
    permissions: fontelle_model::wire::Permissions,
    /// The singers the host's song uses (F93).
    singers: Vec<SingerNeed>,
    /// Plugin states this studio kept of its own plugins, which a copy made
    /// again from the host's song must not take back — see
    /// [`Collab::plugin_kept_here`].
    kept_here: Vec<KeptHere>,
    /// The host's challenge — its nonce and its half of the exchange —
    /// while it waits on the person's password.
    challenge: Option<(Vec<u8>, Vec<u8>)>,
    /// The host has challenged this connection: hello is not said again.
    challenged: bool,
    /// How many times the password has been asked for, and whether the
    /// last one given was wrong.
    asks: u32,
    wrong: bool,
    /// The password given, kept for the life of the join: a link that drops
    /// and joins again proves it again without asking.
    password: Option<String>,
    stretched: Option<(String, String, identity::PasswordKeys)>,
    /// Who this join is waiting on to be let in.
    waiting_for: Option<String>,
    /// The last challenge seen, so the same one heard twice is one.
    last_nonce: Option<Vec<u8>>,
}

impl Joiner {
    fn new(projects_dir: PathBuf) -> Self {
        Self {
            stage: Stage::Waiting,
            projects_dir,
            session: PersistentId::new(),
            hello_at: Instant::now(),
            space: 0,
            host_name: String::new(),
            host_install: PersistentId::default(),
            head: None,
            question: None,
            answer: None,
            snapshot: Vec::new(),
            snapshot_done: None,
            backlog: Vec::new(),
            manifest: Vec::new(),
            confirmed: None,
            seq: 0,
            pending: VecDeque::new(),
            next_local: 0,
            answered: 0,
            resyncing: false,
            singers: Vec::new(),
            peers: BTreeMap::new(),
            view_only: false,
            permissions: Default::default(),
            kept_here: Vec::new(),
            challenge: None,
            challenged: false,
            asks: 0,
            wrong: false,
            password: None,
            stretched: None,
            waiting_for: None,
            last_nonce: None,
        }
    }

    /// Answers the host's challenge `nonce`, whose half of the exchange is
    /// `theirs`: this studio's key, this side's half of the exchange (fresh,
    /// used once), a signature over the nonce, the code and both halves,
    /// and — with a password — the proof of it. Then this end of the link
    /// is sealed, whatever the session: under the exchange's secret, and the
    /// password's too when there is one.
    fn answer(
        &mut self,
        transport: &mut sealed::Sealed,
        options: &CollabOptions,
        nonce: &[u8],
        theirs: &[u8],
    ) {
        let code = identity::normal_code(&options.code);
        let key = options.identity.public_key();
        let exchange = identity::Exchange::new();
        let ours = exchange.public().to_vec();
        let signature = options
            .identity
            .sign(&identity::join_message(nonce, &code, theirs, &ours));
        let keys = self.password.clone().map(|password| match &self.stretched {
            Some((p, c, keys)) if *p == password && *c == code => keys.clone(),
            _ => {
                let keys = identity::PasswordKeys::derive(&password, &code);
                self.stretched = Some((password, code.clone(), keys.clone()));
                keys
            }
        });
        let proof = keys.as_ref().map(|keys| keys.proof(nonce, &key));
        send(
            transport,
            SERVER,
            &Msg::Answer {
                key,
                signature,
                proof,
                exchange: ours,
            },
        );
        // A half that agrees on nothing is not a real one: nothing is sealed
        // here, the host cannot open what this side sends, and the join goes
        // no further.
        if let Some(shared) = exchange.agree(theirs) {
            transport.arm(SERVER, &identity::link_keys(&shared, nonce, keys.as_ref()));
        }
    }

    /// Says which join this is, and hello.
    fn say_hello(&mut self, transport: &mut dyn Transport, options: &CollabOptions) {
        send(transport, SERVER, &Msg::Session { id: self.session });
        send(
            transport,
            SERVER,
            &Msg::Hello {
                protocol: PROTOCOL,
                fontelle: options.fontelle.clone(),
                name: options.name.clone(),
                install: options.install,
            },
        );
        self.hello_at = Instant::now();
    }

    /// One turn. `Some` is why the session ended.
    fn pump(
        &mut self,
        turn: &mut Turn,
        doc: &mut Project,
        history: &mut History,
    ) -> Option<String> {
        // No welcome yet: the hello may have gone to a host that was away
        // from the relay, where nothing would ever answer it.
        // Once challenged it has been heard: the rest of the handshake waits
        // on a person (the password, the host's answer), not on the link.
        if self.stage == Stage::Waiting
            && !self.challenged
            && self.hello_at.elapsed() >= turn.options.hello_again
        {
            self.say_hello(turn.transport, turn.options);
        }
        if self.stage == Stage::Live {
            self.send_mine(turn.transport, history);
        }
        // Nothing is rebased under a drag in the hand (F17): it waits until
        // the button comes up. Whereabouts are not the document.
        let in_hand = self.stage == Stage::Live && history.gesture_in_hand();
        for incoming in turn.incoming(in_hand) {
            match incoming {
                // A connection again after the welcome: the link dropped and
                // `fontelle-net` joined again by itself. The host knows this
                // connection as nobody yet; hello says who it is (F39).
                // A new connection is a new handshake: whatever this one was
                // sealed under is not what the host will seal the next under,
                // so it starts in the clear and proves itself again.
                Incoming::Connected(_) if self.stage != Stage::Waiting || self.challenged => {
                    turn.transport.disarm(SERVER);
                    self.challenged = false;
                    self.challenge = None;
                    self.waiting_for = None;
                    self.say_hello(turn.transport, turn.options);
                }
                Incoming::Connected(_) => {}
                Incoming::Disconnected(_, why) => {
                    // What the two copies last agreed on is the host's song
                    // as this studio last had it — written down, so joining
                    // again finds this copy behind rather than diverged
                    // (F39).
                    if self.stage == Stage::Live
                        && let Some(confirmed) = &self.confirmed
                    {
                        doc.meta.shared_revision = Some((self.host_install, confirmed.sync_hash()));
                    }
                    // The relay's own words — it ended the lobby, or turned
                    // a join again away — said as the end of the session,
                    // with the copy a live joiner is holding.
                    if let Some(why) = &why
                        && self.stage == Stage::Live
                    {
                        return Some(format!(
                            "The session with {} ended \u{2014} {why}; your copy is still open.",
                            self.host_or_them()
                        ));
                    }
                    // Said by how far the join had got. Only a live session
                    // has a copy to keep; before that, "lost — your copy is
                    // still open" was untrue twice over.
                    return Some(why.unwrap_or_else(|| {
                        match self.stage {
                            Stage::Live => format!(
                                "The connection to {} was lost \u{2014} your copy is still open.",
                                self.host_or_them()
                            ),
                            Stage::Waiting => {
                                "Could not reach the song\u{2019}s host \u{2014} check \
                             the code, and that this network lets Fontelle reach its relay \
                             (UDP port 7788)."
                                    .to_string()
                            }
                            Stage::Asking | Stage::Opening => format!(
                                "The connection to {} was lost before the song arrived \u{2014} \
                             nothing was copied. Join again to try once more.",
                                self.host_or_them()
                            ),
                        }
                    }));
                }
                Incoming::Message(_, _, bytes) => {
                    let Ok(msg) = Msg::from_bytes(&bytes) else {
                        eprintln!("Fontelle: a message from the host did not read");
                        continue;
                    };
                    // Files only once there is a copy to put them in.
                    if self.stage == Stage::Live && turn.file_message(SERVER, &msg, doc) {
                        continue;
                    }
                    if let Some(why) = self.receive(turn, msg, doc, history) {
                        return Some(why);
                    }
                }
            }
        }
        if self.stage == Stage::Asking {
            match self.finish() {
                Ok(effect) => turn.effects.extend(effect),
                Err(why) => {
                    send(turn.transport, SERVER, &Msg::Bye);
                    return Some(why);
                }
            }
        }
        None
    }

    /// This studio's edits, proposed, and kept as pending until the host has
    /// put them in its order.
    fn send_mine(&mut self, transport: &mut dyn Transport, history: &mut History) {
        for outgoing in history.take_outgoing() {
            self.next_local += 1;
            send(
                transport,
                SERVER,
                &Msg::Propose {
                    local_seq: self.next_local,
                    edit: outgoing.edit.clone(),
                },
            );
            self.pending.push_back(Pending {
                local_seq: self.next_local,
                entry: outgoing.entry,
                command: outgoing.edit.into_command(),
                shown: true,
            });
        }
    }

    fn host_or_them(&self) -> &str {
        if self.host_name.is_empty() {
            "the host"
        } else {
            &self.host_name
        }
    }

    fn receive(
        &mut self,
        turn: &mut Turn,
        msg: Msg,
        doc: &mut Project,
        history: &mut History,
    ) -> Option<String> {
        match msg {
            // Welcomed again, after a drop on either side: who this studio
            // is may have changed, and nothing else — the song follows.
            Msg::Welcome {
                peer,
                host,
                host_install,
                ..
            } if self.stage == Stage::Opening
                || self.stage == Stage::Live
                || self.answer.is_some() =>
            {
                self.space = peer;
                self.host_name = host;
                self.host_install = host_install;
                if self.stage == Stage::Live {
                    history.set_mint_space(Some(peer));
                }
            }
            Msg::Resumed { handled } => {
                if self.stage == Stage::Live {
                    // Whatever was crossing the link when it dropped did not
                    // arrive whole, either way.
                    turn.files.relink(SERVER, Some(SERVER));
                    // Those the host has had are in the copy that follows
                    // (or were refused, and the copy is without them); the
                    // rest never reached it, and go again.
                    self.answered = self.answered.max(handled);
                    self.pending.retain(|pending| pending.local_seq > handled);
                    for pending in &self.pending {
                        send(
                            turn.transport,
                            SERVER,
                            &Msg::Propose {
                                local_seq: pending.local_seq,
                                edit: pending.command.to_edit(),
                            },
                        );
                    }
                }
            }
            Msg::Welcome {
                peer,
                host,
                host_install,
                project,
                manifest,
                ..
            } => {
                self.space = peer;
                self.host_name = host;
                self.host_install = host_install;
                self.waiting_for = None;
                self.question = Some(join::question(
                    &self.projects_dir,
                    &project,
                    &manifest,
                    &self.host_name,
                    turn.options.install,
                    host_install,
                ));
                self.head = Some(project);
                self.manifest = manifest;
                self.stage = Stage::Asking;
            }
            // A wrong password is the one refusal that is not the end: a
            // fresh challenge follows it, and the person tries again.
            Msg::Refuse { reason } if reason == WRONG_PASSWORD => {
                self.wrong = true;
                self.password = None;
            }
            Msg::Refuse { reason } => return Some(reason),
            // The same challenge again — the host answers every hello, and a
            // hello can be said more than once before the first is heard —
            // is the one already in hand.
            Msg::Challenge { nonce, .. } if self.last_nonce.as_ref() == Some(&nonce) => {}
            Msg::Challenge {
                nonce,
                password,
                exchange,
            } => {
                // Whatever this end sealed under answered an older
                // challenge; the next answer seals under this one's.
                turn.transport.disarm(SERVER);
                self.last_nonce = Some(nonce.clone());
                self.challenged = true;
                self.waiting_for = None;
                match &self.password {
                    // Proved before on this join (a link that dropped and
                    // joined again): proved again without asking.
                    _ if !password => {
                        self.password = None;
                        self.answer(turn.transport, turn.options, &nonce, &exchange);
                    }
                    Some(_) if !self.wrong => {
                        self.answer(turn.transport, turn.options, &nonce, &exchange);
                    }
                    _ => {
                        self.asks += 1;
                        self.challenge = Some((nonce, exchange));
                    }
                }
            }
            Msg::Waiting { host } => {
                self.host_name = host.clone();
                self.waiting_for = Some(host);
            }
            Msg::Singers { singers } => self.singers = singers,
            Msg::SnapshotChunk { index, bytes, .. } => {
                if index == 0 {
                    self.snapshot.clear();
                }
                self.snapshot.extend_from_slice(&bytes);
            }
            Msg::SnapshotDone { hash, seq } => {
                if self.stage == Stage::Live {
                    // A fresh copy asked for after a drift: it replaces the
                    // host's song here, and whatever is still pending is
                    // put back on top of it.
                    match serde_json::from_slice::<Project>(&self.snapshot) {
                        Ok(song) => {
                            self.confirmed = Some(song);
                            self.seq = seq;
                            self.resyncing = false;
                            self.rebuild(doc);
                            turn.effects
                                .push(Effect::Changed(fontelle_model::wire::Reach::Graph));
                        }
                        Err(e) => return Some(format!("The song did not arrive whole: {e}")),
                    }
                } else {
                    self.snapshot_done = Some((hash, seq));
                }
            }
            msg @ Msg::Applied { .. } if self.stage != Stage::Live => self.backlog.push(msg),
            Msg::Applied {
                seq,
                author,
                author_seq,
                edit,
                hash,
            } => {
                self.applied(turn, seq, author, author_seq, edit, hash, doc, history);
            }
            Msg::Refused { local_seq, reason } => {
                self.answered = self.answered.max(local_seq);
                if let Some(at) = self.pending.iter().position(|p| p.local_seq == local_seq) {
                    let refused = self.pending.remove(at).expect("just found");
                    history.forget(refused.entry);
                    eprintln!("Fontelle: the host refused edit {local_seq}: {reason}");
                    let label = refused.command.label();
                    turn.notices.push(if let Some(needed) = reason.strip_prefix(PERMISSION) {
                        let what = match needed {
                            "mixer" => "change the mixer",
                            "sounds" => "change instruments' sounds",
                            _ => "add plugins they haven't got",
                        };
                        format!(
                            "\u{201c}{label}\u{201d} was taken back \u{2014} {} hasn't let you {what}.",
                            self.host_or_them()
                        )
                    } else if self.view_only {
                        // Short enough for a toast (F64).
                        format!("\u{201c}{label}\u{201d} was taken back \u{2014} it is view only.")
                    } else {
                        taken_back(label, self.host_or_them())
                    });
                    self.rebuild(doc);
                    turn.effects
                        .push(Effect::Changed(fontelle_model::wire::Reach::Graph));
                }
            }
            Msg::Joined { peer, name, colour } => {
                turn.notices.push(format!("{name} joined"));
                self.peers.insert(
                    peer,
                    Peer {
                        peer,
                        name,
                        colour,
                        view_only: false,
                    },
                );
            }
            Msg::Presence { peer, presence } => {
                turn.presence.others.insert(peer, presence);
            }
            Msg::Permissions { permissions } => {
                if permissions != self.permissions {
                    self.permissions = permissions;
                    turn.notices
                        .push(permissions_sentence(self.host_or_them(), permissions));
                }
            }
            Msg::Chat { peer, text } => {
                let name = if peer == 0 {
                    self.host_or_them().to_string()
                } else {
                    self.peers
                        .get(&peer)
                        .map_or_else(|| "Somebody".to_string(), |p| p.name.clone())
                };
                let text: String = text.trim().chars().take(CHAT_LONGEST).collect();
                if !text.is_empty() {
                    keep_line(
                        turn.chat,
                        ChatLine {
                            peer,
                            name,
                            text,
                            mine: false,
                        },
                    );
                    *turn.unread += 1;
                }
            }
            Msg::Left { peer } => {
                turn.presence.others.remove(&peer);
                if let Some(who) = self.peers.remove(&peer) {
                    turn.notices.push(format!("{} left", who.name));
                }
            }
            Msg::Bye => {
                if self.stage == Stage::Live {
                    doc.meta.shared_revision = Some((self.host_install, doc.sync_hash()));
                }
                return Some(format!(
                    "{} stopped sharing \u{2014} your copy is still open, and yours now.",
                    self.host_or_them()
                ));
            }
            Msg::ViewOnly { view_only } => {
                if view_only != self.view_only {
                    self.view_only = view_only;
                    turn.notices.push(if view_only {
                        format!("{} made the song view only for you.", self.host_or_them())
                    } else {
                        format!("{} let you edit the song again.", self.host_or_them())
                    });
                }
            }
            Msg::Removed { by } => {
                if self.stage == Stage::Live {
                    doc.meta.shared_revision = Some((self.host_install, doc.sync_hash()));
                }
                return Some(format!(
                    "{by} removed you from the session \u{2014} your copy is still open, and \
                     yours now."
                ));
            }
            _ => {}
        }
        None
    }

    /// The next edit in the host's order.
    ///
    /// **One of this studio's own edits coming back is settled first**,
    /// whatever else is wrong with this message. It used to be settled last,
    /// after two early returns — an edit the copy here had already been
    /// given, and one that did not apply to the copy here — and an own edit
    /// that came back by either of those stayed pending for good although
    /// the host had it. Every later answer to an edit of this studio's then
    /// met it at the head of `pending`, was taken for somebody else's, went
    /// into the copy *and* stayed pending on top of it: an effect added once
    /// was there twice, a clip dragged four beats went eight, a clip the host
    /// deleted came back for good (Ty, 2026-10-11; F87).
    #[allow(clippy::too_many_arguments)]
    fn applied(
        &mut self,
        turn: &mut Turn,
        seq: u64,
        author: u16,
        author_seq: u64,
        edit: Edit,
        hash: Option<u64>,
        doc: &mut Project,
        history: &mut History,
    ) {
        let mine = author == self.space && author_seq > 0;
        // Whether the screen still shows the host's song with this studio's
        // waiting edits on it, once this edit is in.
        let mut screen_holds = true;
        if mine {
            self.answered = self.answered.max(author_seq);
            match self.pending.front() {
                Some(front) if front.local_seq == author_seq => {
                    let front = self.pending.pop_front().expect("just looked");
                    // The host made it from the state it had, which can be
                    // a different state from this screen's: then what it
                    // made differs, and the screen is made again from the
                    // host's song (§5.5 step 2, "apply the host's form").
                    // And one that was not on the screen is now.
                    if !front.shown || front.command.to_edit().to_bytes() != edit.to_bytes() {
                        screen_holds = false;
                    }
                }
                _ => {
                    // Not at the head: whatever was sent before it and is
                    // still waiting never reached the host (it answers in
                    // the order it was sent), and the screen is made again
                    // from the host's song, which has this one.
                    let host = self.host_or_them().to_string();
                    let answered = self.answered;
                    self.pending.retain(|pending| {
                        let keep = pending.local_seq > answered;
                        if !keep && pending.local_seq != author_seq {
                            history.forget(pending.entry);
                            turn.notices
                                .push(taken_back(pending.command.label(), &host));
                        }
                        keep
                    });
                    screen_holds = false;
                }
            }
        }
        if seq <= self.seq {
            // Already in the copy this studio was given.
            if !screen_holds {
                self.rebuild(doc);
                turn.effects
                    .push(Effect::Changed(fontelle_model::wire::Reach::Graph));
            }
            return;
        }
        self.seq = seq;
        let confirmed = self
            .confirmed
            .as_mut()
            .expect("live means a confirmed copy");
        if let Err(e) = apply_as(author, edit.clone().into_command().as_mut(), confirmed) {
            eprintln!("Fontelle: the host's edit {seq} did not apply to its own song here: {e}");
            self.resync(turn.transport);
            if !screen_holds {
                self.rebuild(doc);
                turn.effects
                    .push(Effect::Changed(fontelle_model::wire::Reach::Graph));
            }
            return;
        }
        healed(confirmed);
        if let Some(theirs) = hash
            && !self.resyncing
        {
            let ours = confirmed.sync_hash();
            if ours != theirs {
                drifted(turn.options, seq, ours, theirs);
                self.resync(turn.transport);
            }
        }

        if mine {
            if !screen_holds {
                self.rebuild(doc);
                turn.effects
                    .push(Effect::Changed(fontelle_model::wire::Reach::Graph));
            }
            return;
        }
        // What it costs here is what the edit reaches: putting this studio's
        // pending edits back on top changes nothing the graph was not already
        // built for. Only an edit that would not apply leaves a copy rebuilt
        // from scratch.
        let mut reach = edit.reach();
        if self.pending.is_empty() {
            // Nothing of ours in the way: the copy on screen is the host's.
            if apply_as(author, edit.clone().into_command().as_mut(), doc).is_err() {
                self.rebuild(doc);
                reach = fontelle_model::wire::Reach::Graph;
            } else {
                healed(doc);
            }
        } else {
            self.rebuild(doc);
        }
        // A file somebody else's edit names comes from the host (§7.2).
        turn.want_files_of(&edit, SERVER, doc);
        turn.effects.push(Effect::Changed(reach));
    }

    /// Asks the host for a fresh copy of its song, once until it comes.
    fn resync(&mut self, transport: &mut dyn Transport) {
        if !self.resyncing {
            self.resyncing = true;
            send(transport, SERVER, &Msg::ResyncRequest);
        }
    }

    /// The document on screen, made again: the host's song, and this
    /// studio's edits the host has not ordered yet on top of it — applied
    /// where this studio mints, as they were the first time (F89). One that
    /// does not apply any more — somebody changed what it was about — is
    /// left off the screen and kept until the host answers it (`Pending::
    /// shown`); one the host has already answered is never put on again
    /// (F87).
    fn rebuild(&mut self, doc: &mut Project) {
        let Some(confirmed) = &self.confirmed else {
            return;
        };
        let mut rebuilt = confirmed.clone();
        let answered = self.answered;
        let space = self.space;
        self.pending.retain_mut(|pending| {
            if pending.local_seq <= answered {
                return false;
            }
            pending.shown = match apply_as(space, pending.command.as_mut(), &mut rebuilt) {
                Ok(()) => true,
                Err(e) => {
                    eprintln!("Fontelle: a pending edit does not apply for now: {e}");
                    false
                }
            };
            true
        });
        healed(&mut rebuilt);
        // What this studio kept of its own plugins, where the host's song
        // has not changed them since: a plugin's state is each machine's
        // own (§5.2), and putting the song's older one back undid a knob
        // turned in the plugin's window here. Where the song did change
        // (a preset somebody chose), the song's is newer, and the keeping
        // is over.
        self.kept_here
            .retain(|kept| match kept.place.slot_mut(&mut rebuilt) {
                Some(slot) if *slot == kept.stream => {
                    *slot = Some(kept.kept.clone());
                    true
                }
                _ => false,
            });
        // The view is this studio's own, whatever the song did.
        rebuilt.view_state = doc.view_state.clone();
        rebuilt.meta = doc.meta.clone();
        keep_solos(doc, &mut rebuilt);
        *doc = rebuilt;
    }

    /// Writes the join's copy once there is both an answer and a whole song;
    /// a song that cannot be used ends the join, and says why.
    ///
    /// It used to be a line on the status bar, and the panel went on saying
    /// the song was on its way. Ty, joining a friend from a preview build:
    /// *"its getting stuck on this"*. Waiting cannot mend it — the host sends
    /// the same song again — so the join ends where the person can see it.
    fn finish(&mut self) -> Result<Option<Effect>, String> {
        let (Some(answer), Some((hash, _)), Some(head)) =
            (self.answer.clone(), self.snapshot_done, self.head.clone())
        else {
            return Ok(None);
        };
        if self.stage != Stage::Asking {
            return Ok(None);
        }
        // Two builds that write songs differently cannot agree on one: the
        // likeliest cause by far, and the one thing the person can act on.
        let mend = format!(
            "you and {} need the same version of Fontelle. Nothing was copied.",
            self.host_or_them()
        );
        let song: Project = serde_json::from_slice(&self.snapshot)
            .map_err(|e| format!("The song did not arrive whole ({e}) \u{2014} {mend}"))?;
        if song.sync_hash() != hash {
            return Err(format!("The song arrived damaged \u{2014} {mend}"));
        }
        let written = match answer {
            JoinAnswer::Copy => join::copy_in(&self.projects_dir, &song, &head, self.host_install),
            JoinAnswer::Update(path) => {
                join::update(&path, &song, &head, self.host_install).map(|_| path)
            }
            JoinAnswer::KeepBoth(path) => join::fork(&path)
                .and_then(|()| join::copy_in(&self.projects_dir, &song, &head, self.host_install)),
            JoinAnswer::Pick(_) | JoinAnswer::Cancel => return Ok(None),
        };
        Ok(Some(match written {
            Ok(path) => {
                self.stage = Stage::Opening;
                self.confirmed = Some(song);
                Effect::Open(path)
            }
            Err(e) => Effect::Say(format!("Could not write the copy: {e}")),
        }))
    }

    /// The copy is open as `doc`. `Some` is why the session ended instead.
    fn went_live(
        &mut self,
        turn: &mut Turn,
        doc: &mut Project,
        history: &mut History,
    ) -> Option<String> {
        let (_, seq) = self.snapshot_done?;
        let confirmed = self.confirmed.as_ref()?;
        if doc.sync_hash() != confirmed.sync_hash() {
            drifted(turn.options, seq, doc.sync_hash(), confirmed.sync_hash());
            self.resync(turn.transport);
        }
        self.seq = seq;
        self.stage = Stage::Live;
        history.set_mint_space(Some(self.space));
        history.open_outbox();
        // Every file the song uses that is not here yet, smallest first.
        let wanted = files::missing(doc, &turn.place.with(doc));
        let here = turn.files.want(
            wanted,
            SERVER,
            &turn.place.with(doc),
            turn.options.ask_above,
        );
        turn.effects
            .extend(here.into_iter().map(Effect::FileArrived));
        for msg in std::mem::take(&mut self.backlog) {
            if let Some(why) = self.receive(turn, msg, doc, history) {
                return Some(why);
            }
        }
        None
    }
}

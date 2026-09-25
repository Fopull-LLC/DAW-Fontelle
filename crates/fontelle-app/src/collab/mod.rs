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
pub(crate) mod join;

pub use files::FetchQuestion;

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use fontelle_model::wire::{AssetEntry, Edit, Msg, PROTOCOL, ProjectHead};
use fontelle_model::{Command, History, Project};
use fontelle_net::{Channel, Incoming, PeerId, SERVER, Transport};
use fontelle_types::PersistentId;

use files::{Files, Place};

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
#[derive(Clone, Debug)]
pub struct CollabOptions {
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
}

impl CollabOptions {
    pub fn new(name: impl Into<String>, install: PersistentId) -> Self {
        Self {
            name: name.into(),
            install,
            fontelle: env!("CARGO_PKG_VERSION").to_string(),
            strict: false,
            idle_break: Duration::from_millis(400),
            ask_above: 64 * 1024 * 1024,
        }
    }
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
    /// graph and the window have to catch up — what an undo does.
    Changed,
    /// The join's copy is on disk: open it, then hand it back to
    /// [`Collab::went_live`].
    Open(PathBuf),
    /// Something to put on the status line.
    Say(String),
    /// A file the song names has arrived whole: load it into what plays it.
    FileArrived(u64),
}

enum Role {
    Host(Host),
    Joiner(Box<Joiner>),
}

/// One shared session, from this studio's side.
pub(crate) struct Collab {
    transport: Box<dyn Transport>,
    options: CollabOptions,
    role: Role,
    /// Sentences for the person, taken by the window (toasts).
    notices: Vec<String>,
    /// Why the session is over, once it is.
    ended: Option<String>,
    /// The history's generation last seen with an edit in the hand, and
    /// since when — see [`CollabOptions::idle_break`].
    held: Option<(u64, Instant)>,
    /// The song's files on their way in and out (§7.2).
    files: Files,
}

impl Collab {
    pub fn host(transport: Box<dyn Transport>, options: CollabOptions) -> Self {
        Self {
            transport,
            options,
            role: Role::Host(Host::default()),
            notices: Vec::new(),
            ended: None,
            held: None,
            files: Files::default(),
        }
    }

    /// Starts a join: says hello, and waits for the song.
    pub fn join(
        mut transport: Box<dyn Transport>,
        options: CollabOptions,
        projects_dir: PathBuf,
    ) -> Self {
        send(
            transport.as_mut(),
            SERVER,
            &Msg::Hello {
                protocol: PROTOCOL,
                fontelle: options.fontelle.clone(),
                name: options.name.clone(),
                install: options.install,
            },
        );
        Self {
            transport,
            options,
            role: Role::Joiner(Box::new(Joiner::new(projects_dir))),
            notices: Vec::new(),
            ended: None,
            held: None,
            files: Files::default(),
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
            send(self.transport.as_mut(), id, &Msg::ViewOnly { view_only });
        }
        Ok(())
    }

    /// The host's *remove* on somebody's row (§10.1, F49): they are told who
    /// did it, and let go of. The code is unchanged — anybody who has it can
    /// come back (§14.6).
    pub fn remove(&mut self, peer: u16, doc: &mut Project) -> Result<(), String> {
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
        send(
            self.transport.as_mut(),
            id,
            &Msg::Removed {
                by: self.options.name.clone(),
            },
        );
        self.transport.disconnect(id);
        // They part holding the song as it is now (§4.5).
        doc.meta.shared_revision = Some((who.install, doc.sync_hash()));
        host.broadcast(self.transport.as_mut(), &Msg::Left { peer: who.space });
        self.notices.push(format!("You removed {}.", who.name));
        Ok(())
    }

    /// How far along the file `hash` is, while it is on its way here.
    pub fn fetching(&self, hash: u64) -> Option<f32> {
        self.files.progress(hash)
    }

    /// The files waiting on the person before they are fetched.
    pub fn fetch_question(&self) -> Option<FetchQuestion> {
        self.files.question()
    }

    pub fn answer_fetch(&mut self, fetch: bool) {
        self.files.answer(fetch);
    }

    /// The files that have arrived whole, by hash.
    pub fn files_received(&self) -> &[u64] {
        self.files.received()
    }

    /// One turn: what this studio did goes out, and what arrived comes in.
    pub fn pump(&mut self, doc: &mut Project, history: &mut History, place: &Place) -> Vec<Effect> {
        if self.ended.is_some() {
            return Vec::new();
        }
        self.let_go_of_idle_edits(history);
        let mut effects = Vec::new();
        let mut turn = Turn {
            transport: self.transport.as_mut(),
            options: &self.options,
            files: &mut self.files,
            place,
            notices: &mut self.notices,
            effects: &mut effects,
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
        self.files.pump(self.transport.as_mut(), &place.with(doc));
        effects
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
                send(self.transport.as_mut(), SERVER, &Msg::Bye);
                self.end("You cancelled the join.".to_string(), history);
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
                Ok(joiner.finish().into_iter().collect())
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
            transport: self.transport.as_mut(),
            options: &self.options,
            files: &mut self.files,
            place,
            notices: &mut self.notices,
            effects: &mut effects,
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
                host.send_mine(self.transport.as_mut(), doc, history);
                for (peer, who) in &host.peers {
                    if who.welcomed {
                        doc.meta.shared_revision = Some((who.install, doc.sync_hash()));
                    }
                    send(self.transport.as_mut(), *peer, &Msg::Bye);
                }
                self.end("You stopped sharing.".to_string(), history);
            }
            Role::Joiner(joiner) => {
                if joiner.stage == Stage::Live {
                    joiner.send_mine(self.transport.as_mut(), history);
                    doc.meta.shared_revision = Some((joiner.host_install, doc.sync_hash()));
                }
                send(self.transport.as_mut(), SERVER, &Msg::Bye);
                self.end("You left the session.".to_string(), history);
            }
        }
    }

    fn end(&mut self, why: String, history: &mut History) {
        history.close_outbox();
        history.set_mint_space(None);
        self.ended = Some(why);
    }
}

fn send(transport: &mut dyn Transport, to: PeerId, msg: &Msg) {
    transport.send(to, Channel::Reliable, &msg.to_bytes());
}

/// Everything a turn works with besides the document and the history.
struct Turn<'a> {
    transport: &'a mut dyn Transport,
    options: &'a CollabOptions,
    files: &'a mut Files,
    place: &'a Place,
    notices: &'a mut Vec<String>,
    effects: &'a mut Vec<Effect>,
}

impl Turn<'_> {
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
fn drifted(
    options: &CollabOptions,
    transport: &mut dyn Transport,
    seq: u64,
    ours: u64,
    theirs: u64,
) {
    let line = format!(
        "Fontelle: shared song drifted at edit {seq} (this copy {ours:016x}, host {theirs:016x}); \
         asking for a fresh copy"
    );
    assert!(!options.strict, "{line}");
    eprintln!("{line}");
    send(transport, SERVER, &Msg::ResyncRequest);
}

// ================================================================== the host

#[derive(Default)]
struct Host {
    peers: BTreeMap<PeerId, HostPeer>,
    /// The number of the last edit in the one order there is.
    seq: u64,
}

struct HostPeer {
    space: u16,
    name: String,
    install: PersistentId,
    welcomed: bool,
    view_only: bool,
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
        // a document nobody else could reach (F17).
        if history.gesture_in_hand() {
            return None;
        }
        for incoming in turn.transport.poll() {
            match incoming {
                Incoming::Connected(peer) => {
                    self.peers.insert(
                        peer,
                        HostPeer {
                            space: 0,
                            name: String::new(),
                            install: PersistentId::default(),
                            welcomed: false,
                            view_only: false,
                        },
                    );
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
    fn send_mine(&mut self, transport: &mut dyn Transport, doc: &Project, history: &mut History) {
        let mine = history.take_outgoing();
        let count = mine.len();
        for (i, outgoing) in mine.into_iter().enumerate() {
            self.seq += 1;
            let hash = (i + 1 == count).then(|| doc.sync_hash());
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
            Msg::Hello {
                protocol,
                fontelle,
                name,
                install,
            } => {
                let options = turn.options;
                if protocol != PROTOCOL || fontelle != options.fontelle {
                    let reason = version_sentence(&options.name, &options.fontelle, &fontelle);
                    send(turn.transport, peer, &Msg::Refuse { reason });
                    turn.transport.disconnect(peer);
                    self.peers.remove(&peer);
                    turn.notices.push(format!(
                        "{name} tried to join with Fontelle {fontelle}, and this is {}.",
                        options.fontelle
                    ));
                    return;
                }
                let space = (1..=u16::from(u8::MAX))
                    .find(|space| {
                        self.peers
                            .values()
                            .all(|p| !p.welcomed || p.space != *space)
                    })
                    .unwrap_or(u16::from(u8::MAX));
                send(
                    turn.transport,
                    peer,
                    &Msg::Welcome {
                        peer: space,
                        protocol: PROTOCOL,
                        fontelle: options.fontelle.clone(),
                        host: options.name.clone(),
                        host_install: options.install,
                        project: head_of(doc),
                        manifest: manifest(doc),
                    },
                );
                self.send_snapshot(turn.transport, peer, doc);
                // Who is here already, then everybody else hears of them.
                for other in self.peers.values().filter(|p| p.welcomed) {
                    let public = other.public();
                    send(
                        turn.transport,
                        peer,
                        &Msg::Joined {
                            peer: public.peer,
                            name: public.name,
                            colour: public.colour,
                        },
                    );
                }
                let entry = self.peers.entry(peer).or_insert(HostPeer {
                    space,
                    name: name.clone(),
                    install,
                    welcomed: false,
                    view_only: false,
                });
                entry.space = space;
                entry.name = name.clone();
                entry.install = install;
                entry.welcomed = true;
                let public = entry.public();
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
            Msg::Propose { local_seq, edit } => {
                let Some(author) = self.peers.get(&peer).filter(|p| p.welcomed) else {
                    return;
                };
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
                let author = author.space;
                // The proposal as it came, ids and all: the joiner minted
                // them in its own space, so they can be nobody else's (F55).
                let mut command = edit.into_command();
                match command.apply(doc) {
                    Ok(()) => {
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
                                edit: applied,
                                hash: Some(doc.sync_hash()),
                            },
                        );
                        turn.effects.push(Effect::Changed);
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
            Msg::Bye => self.gone(turn, peer, doc, true),
            _ => {}
        }
    }

    /// Somebody has left — said goodbye, or dropped.
    fn gone(&mut self, turn: &mut Turn, peer: PeerId, doc: &mut Project, said_goodbye: bool) {
        let Some(who) = self.peers.remove(&peer) else {
            return;
        };
        if !who.welcomed {
            return;
        }
        // What the two copies agree on as they part (§4.5).
        if said_goodbye {
            doc.meta.shared_revision = Some((who.install, doc.sync_hash()));
        }
        self.broadcast(turn.transport, &Msg::Left { peer: who.space });
        turn.notices.push(format!("{} left", who.name));
        turn.effects.push(Effect::Say(format!("{} left", who.name)));
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

/// What a refused joiner is told (§8.3): who has which, and what to do.
fn version_sentence(host: &str, hosts: &str, yours: &str) -> String {
    format!(
        "{host} has Fontelle {hosts} and you have {yours} \u{2014} the newer one should host, \
         or update from the start menu."
    )
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
}

struct Joiner {
    stage: Stage,
    projects_dir: PathBuf,
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
    peers: BTreeMap<u16, Peer>,
    /// The host refuses this studio's edits (F49).
    view_only: bool,
}

impl Joiner {
    fn new(projects_dir: PathBuf) -> Self {
        Self {
            stage: Stage::Waiting,
            projects_dir,
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
            peers: BTreeMap::new(),
            view_only: false,
        }
    }

    /// One turn. `Some` is why the session ended.
    fn pump(
        &mut self,
        turn: &mut Turn,
        doc: &mut Project,
        history: &mut History,
    ) -> Option<String> {
        if self.stage == Stage::Live {
            self.send_mine(turn.transport, history);
            // Nothing is rebased under a drag in the hand (F17): it waits in
            // the transport until the button comes up.
            if history.gesture_in_hand() {
                return None;
            }
        }
        for incoming in turn.transport.poll() {
            match incoming {
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
                    return Some(why.unwrap_or_else(|| {
                        format!(
                            "The connection to {} was lost \u{2014} your copy is still open.",
                            self.host_or_them()
                        )
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
            turn.effects.extend(self.finish());
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
            Msg::Refuse { reason } => return Some(reason),
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
                            self.rebuild(doc, history, turn.notices);
                            turn.effects.push(Effect::Changed);
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
                if let Some(at) = self.pending.iter().position(|p| p.local_seq == local_seq) {
                    let refused = self.pending.remove(at).expect("just found");
                    history.forget(refused.entry);
                    eprintln!("Fontelle: the host refused edit {local_seq}: {reason}");
                    let label = refused.command.label();
                    turn.notices.push(if self.view_only {
                        // Short enough for a toast (F64).
                        format!("\u{201c}{label}\u{201d} was taken back \u{2014} it is view only.")
                    } else {
                        taken_back(label, self.host_or_them())
                    });
                    self.rebuild(doc, history, turn.notices);
                    turn.effects.push(Effect::Changed);
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
            Msg::Left { peer } => {
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
        if seq <= self.seq {
            return; // already in the copy this studio was given
        }
        self.seq = seq;
        let confirmed = self
            .confirmed
            .as_mut()
            .expect("live means a confirmed copy");
        if let Err(e) = edit.clone().into_command().apply(confirmed) {
            eprintln!("Fontelle: the host's edit {seq} did not apply to its own song here: {e}");
            send(turn.transport, SERVER, &Msg::ResyncRequest);
            return;
        }
        if let Some(theirs) = hash {
            let ours = confirmed.sync_hash();
            if ours != theirs {
                drifted(turn.options, turn.transport, seq, ours, theirs);
            }
        }

        let mine = author == self.space
            && self
                .pending
                .front()
                .is_some_and(|pending| pending.local_seq == author_seq);
        if mine {
            // The host put it where this studio already had it.
            self.pending.pop_front();
            return;
        }
        if self.pending.is_empty() {
            // Nothing of ours in the way: the copy on screen is the host's.
            if edit.clone().into_command().apply(doc).is_err() {
                self.rebuild(doc, history, turn.notices);
            }
        } else {
            self.rebuild(doc, history, turn.notices);
        }
        // A file somebody else's edit names comes from the host (§7.2).
        turn.want_files_of(&edit, SERVER, doc);
        turn.effects.push(Effect::Changed);
    }

    /// The document on screen, made again: the host's song, and this
    /// studio's edits the host has not ordered yet on top of it. An edit that
    /// no longer applies — somebody deleted what it was about — is dropped,
    /// its undo entry with it, and said.
    fn rebuild(&mut self, doc: &mut Project, history: &mut History, notices: &mut Vec<String>) {
        let Some(confirmed) = &self.confirmed else {
            return;
        };
        let mut rebuilt = confirmed.clone();
        let host = self.host_or_them().to_string();
        self.pending
            .retain_mut(|pending| match pending.command.apply(&mut rebuilt) {
                Ok(()) => true,
                Err(e) => {
                    history.forget(pending.entry);
                    eprintln!("Fontelle: a pending edit no longer applies: {e}");
                    notices.push(taken_back(pending.command.label(), &host));
                    false
                }
            });
        // The view is this studio's own, whatever the song did.
        rebuilt.view_state = doc.view_state.clone();
        rebuilt.meta = doc.meta.clone();
        *doc = rebuilt;
    }

    /// Writes the join's copy once there is both an answer and a whole song.
    fn finish(&mut self) -> Option<Effect> {
        let answer = self.answer.clone()?;
        let (hash, _) = self.snapshot_done?;
        let head = self.head.clone()?;
        if self.stage != Stage::Asking {
            return None;
        }
        let song: Project = match serde_json::from_slice(&self.snapshot) {
            Ok(song) => song,
            Err(e) => {
                return Some(Effect::Say(format!("The song did not arrive whole: {e}")));
            }
        };
        if song.sync_hash() != hash {
            return Some(Effect::Say("The song arrived damaged; join again.".into()));
        }
        let written = match answer {
            JoinAnswer::Copy => join::copy_in(&self.projects_dir, &song, &head, self.host_install),
            JoinAnswer::Update(path) => {
                join::update(&path, &song, &head, self.host_install).map(|_| path)
            }
            JoinAnswer::KeepBoth(path) => join::fork(&path)
                .and_then(|()| join::copy_in(&self.projects_dir, &song, &head, self.host_install)),
            JoinAnswer::Pick(_) | JoinAnswer::Cancel => return None,
        };
        match written {
            Ok(path) => {
                self.stage = Stage::Opening;
                self.confirmed = Some(song);
                Some(Effect::Open(path))
            }
            Err(e) => Some(Effect::Say(format!("Could not write the copy: {e}"))),
        }
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
            drifted(
                turn.options,
                turn.transport,
                seq,
                doc.sync_hash(),
                confirmed.sync_hash(),
            );
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

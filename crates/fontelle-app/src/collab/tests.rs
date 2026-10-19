//! The host and a joiner as bare machines — a document and a history each,
//! no studio around them — so a test can reach inside the joiner's copy of
//! the host's song and make it disagree, the way any mutation that skips the
//! stream does (F82, F83, and the take's trim healed outside it: see
//! `tests/collab_latency.rs`).
//!
//! Ty, after a session with a friend (2026-10-11): *"my actions are
//! constantly taken back like im trying to delete this one clip but it keeps
//! immediately reappearing. my actions are also doubling often like adding
//! an effect made it add 2 versions of that effect."*

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fontelle_model::{
    AddClip, AddInsert, Arena, Clip, ClipSource, Command, History, MoveClip, NoteData, Project,
    RemoveClip,
};
use fontelle_net::{Channel, Incoming, MemoryHub, PeerId, Transport};
use fontelle_types::{EffectKind, PPQN, PersistentId};

use super::files::Place;
use super::*;

/// A joiner's link that counts what the joiner asks for.
struct Counting {
    inner: Box<dyn Transport>,
    resyncs: Arc<Mutex<usize>>,
}

impl Transport for Counting {
    fn send(&mut self, peer: PeerId, channel: Channel, bytes: &[u8]) {
        if matches!(Msg::from_bytes(bytes), Ok(Msg::ResyncRequest)) {
            *self.resyncs.lock().unwrap() += 1;
        }
        self.inner.send(peer, channel, bytes);
    }
    fn poll(&mut self) -> Vec<Incoming> {
        self.inner.poll()
    }
    fn stats(&self, peer: PeerId) -> fontelle_net::LinkStats {
        self.inner.stats(peer)
    }
}

struct Side {
    collab: Collab,
    doc: Project,
    history: History,
    place: Place,
}

impl Side {
    fn pump(&mut self) -> Vec<Effect> {
        self.collab
            .pump(&mut self.doc, &mut self.history, &self.place)
    }

    fn run(&mut self, command: impl Command + 'static) {
        self.history
            .apply(Box::new(command), &mut self.doc)
            .expect("applies here");
        self.history.break_gesture();
    }

    fn confirmed(&mut self) -> &mut Project {
        match &mut self.collab.role {
            Role::Joiner(joiner) => joiner.confirmed.as_mut().expect("live"),
            Role::Host(_) => panic!("the host's song is its document"),
        }
    }

    fn pending(&self) -> usize {
        match &self.collab.role {
            Role::Joiner(joiner) => joiner.pending.len(),
            Role::Host(_) => 0,
        }
    }
}

struct Two {
    hub: MemoryHub,
    now: u64,
    host: Side,
    joiner: Side,
    resyncs: Arc<Mutex<usize>>,
    dir: PathBuf,
}

impl Drop for Two {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

fn options(name: &str) -> CollabOptions {
    let mut options = CollabOptions::new(name, PersistentId::derived(name));
    options.idle_break = Duration::ZERO;
    options.hello_again = Duration::ZERO;
    options
}

/// How a [`Two`] is set up: the host's password, the password the joiner
/// gives when asked, and what either link goes through on its way to the hub.
#[derive(Default)]
struct Setup {
    host_password: Option<&'static str>,
    joiner_password: Option<&'static str>,
    host_link: Option<Wrap>,
    joiner_link: Option<Wrap>,
}

/// A link's end on the hub, made into whatever the test wants it to be.
type Wrap = Box<dyn FnOnce(&MemoryHub, fontelle_net::MemoryTransport) -> Box<dyn Transport>>;

impl Two {
    fn new(name: &str, latency: u64) -> Two {
        Two::with(name, latency, Setup::default())
    }

    fn with(name: &str, latency: u64, setup: Setup) -> Two {
        let dir = std::env::temp_dir().join(format!(
            "fontelle-collab-unit-{name}-{}",
            std::process::id()
        ));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        let hub = MemoryHub::new();
        hub.set_conditions(latency, 0.0);
        let mut song = crate::blank_project(8, 120.0, 48_000);
        let lane = song.lane_ids()[0];
        let channel = song.channels.keys().next().unwrap();
        for at in [0, 16] {
            AddClip::new(a_clip(lane, channel, PPQN * at))
                .apply(&mut song)
                .unwrap();
        }
        fontelle_model::AddMixerTrack::new("Bus")
            .apply(&mut song)
            .unwrap();
        let mut host_history = History::new();
        host_history.open_outbox();
        let mut host_options = options("Host");
        host_options.password = setup.host_password.map(str::to_string);
        let server: Box<dyn Transport> = match setup.host_link {
            Some(wrap) => wrap(&hub, hub.server_endpoint()),
            None => Box::new(hub.server_endpoint()),
        };
        let host = Side {
            collab: Collab::host(server, host_options),
            doc: song,
            history: host_history,
            place: Place {
                bundle: None,
                banks: Vec::new(),
            },
        };
        let resyncs = Arc::new(Mutex::new(0));
        let inner: Box<dyn Transport> = match setup.joiner_link {
            Some(wrap) => wrap(&hub, hub.connect()),
            None => Box::new(hub.connect()),
        };
        let link = Counting {
            inner,
            resyncs: resyncs.clone(),
        };
        let joiner = Side {
            collab: Collab::join(Box::new(link), options("Joiner"), dir.clone()),
            doc: Project::new("nothing yet"),
            history: History::new(),
            place: Place {
                bundle: None,
                banks: Vec::new(),
            },
        };
        let mut two = Two {
            hub,
            now: 0,
            host,
            joiner,
            resyncs,
            dir,
        };
        let mut joiner = two.take_joiner();
        let live = bring_in(&mut two, &mut joiner, setup.joiner_password);
        two.joiner = joiner;
        assert!(live, "the joiner has the song open");
        two.settle();
        two
    }

    /// The joiner, taken out to be pumped beside the pair (a placeholder in
    /// its place that nothing reaches).
    fn take_joiner(&mut self) -> Side {
        let nobody = Side {
            collab: Collab::join(Box::new(Closed), options("Nobody"), self.dir.join("nobody")),
            doc: Project::new("nobody"),
            history: History::new(),
            place: Place {
                bundle: None,
                banks: Vec::new(),
            },
        };
        std::mem::replace(&mut self.joiner, nobody)
    }

    /// Somebody else, joining over the same hub as `name` with `identity`.
    fn another(&self, name: &str, identity: Option<Identity>) -> Side {
        let mut options = options(name);
        if let Some(identity) = identity {
            options.identity = identity;
        }
        let dir = self.dir.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        Side {
            collab: Collab::join(Box::new(self.hub.connect()), options, dir),
            doc: Project::new("nothing yet"),
            history: History::new(),
            place: Place {
                bundle: None,
                banks: Vec::new(),
            },
        }
    }

    /// One turn each; what the joiner's turn asked of its studio.
    fn tick(&mut self) -> Vec<Effect> {
        self.now += 1;
        self.hub.set_now(self.now);
        self.host.pump();
        self.joiner.pump()
    }

    fn settle(&mut self) {
        for _ in 0..60 {
            self.tick();
        }
    }
}

fn a_clip(lane: fontelle_types::LaneId, channel: fontelle_types::ChannelId, start: i64) -> Clip {
    Clip {
        name: None,
        lane,
        start,
        length: PPQN * 4,
        source: ClipSource::Notes(NoteData {
            channel,
            notes: Arena::default(),
            vocal: None,
        }),
        prefab_link: None,
        color: None,
        muted: false,
        loop_length: None,
    }
}

fn a_track_that_is_not_the_master(doc: &Project) -> fontelle_types::MixerTrackId {
    doc.mixer
        .tracks
        .keys()
        .find(|id| Some(*id) != doc.mixer.master)
        .expect("a track besides the master")
}

/// **Measured before the fix**: one disagreement between the joiner's copy
/// of the host's song and the host's song — whatever caused it — and the
/// joiner's own edit whose answer landed on that copy stayed "pending" for
/// good, put again on top of the host's song every time the screen was made
/// again. An effect added once was there twice, and every later edit of the
/// joiner's was taken for somebody else's: its answer put into the copy and
/// the edit put again on top of it.
#[test]
fn an_effect_added_once_is_there_once_after_a_disagreement() {
    let mut two = Two::new("twice", 3);
    let track = a_track_that_is_not_the_master(&two.joiner.doc);
    let before = two.joiner.doc.mixer.tracks[track].inserts.len();
    two.joiner.run(AddInsert::new(track, EffectKind::Eq));
    // Before the answer is back, the joiner's copy of the host's song loses
    // that track (any mutation that skipped the stream).
    two.joiner.confirmed().mixer.tracks.remove(track);
    two.settle();
    assert_eq!(
        two.joiner.doc.mixer.tracks[track].inserts.len(),
        before + 1,
        "the effect added once is there once"
    );
    assert_eq!(two.joiner.pending(), 0, "nothing is left waiting");

    // And the next edit is made once too.
    two.joiner.run(AddInsert::new(track, EffectKind::Delay));
    two.settle();
    assert_eq!(two.joiner.doc.mixer.tracks[track].inserts.len(), before + 2);
    assert_eq!(two.joiner.doc.sync_hash(), two.host.doc.sync_hash());
}

/// **Measured before the fix**: the joiner makes a clip, the host deletes it
/// at once, and the joiner's copy had disagreed meanwhile. The clip came back
/// on the joiner's screen, for good; deleting it there was refused by the
/// host (it had no such clip) and "taken back", and the clip came back
/// again — Ty's *"im trying to delete this one clip but it keeps
/// immediately reappearing"*.
#[test]
fn a_clip_deleted_stays_deleted_after_a_disagreement() {
    let mut two = Two::new("reappears", 3);
    let lane = two.joiner.doc.lane_ids()[0];
    let channel = two.joiner.doc.channels.keys().next().unwrap();
    two.joiner
        .run(AddClip::new(a_clip(lane, channel, PPQN * 40)));
    let made = two
        .joiner
        .doc
        .clips
        .iter()
        .find(|(_, c)| c.start == PPQN * 40)
        .map(|(id, _)| id)
        .unwrap();
    // Before its answer is back, the joiner's copy of the host's song
    // disagrees about that clip.
    let phantom = two.joiner.doc.clips[made].clone();
    assert!(two.joiner.confirmed().clips.insert_at(made, phantom));
    // The host deletes it the moment it arrives.
    for _ in 0..40 {
        two.tick();
        if two.host.doc.clips.contains_key(made) {
            two.host.run(RemoveClip::new(made));
            break;
        }
    }
    two.settle();
    assert!(
        two.joiner.doc.clips.get(made).is_none(),
        "deleted by the host, gone on the joiner's screen"
    );
    let said = two.joiner.collab.take_notices().join(" ");
    assert!(!said.contains("taken back"), "{said}");
    assert_eq!(two.joiner.pending(), 0);
    assert_eq!(two.joiner.doc.sync_hash(), two.host.doc.sync_hash());
}

/// The same, for a move: once the copy disagreed, every drag of the
/// joiner's landed twice as far on his own screen.
#[test]
fn a_drag_after_a_disagreement_lands_where_it_was_dropped() {
    let mut two = Two::new("drag-twice", 3);
    let clip = two.joiner.doc.clips.keys().next().unwrap();
    let start = two.joiner.doc.clips[clip].start;
    let other = two.joiner.doc.clips.keys().nth(1).unwrap();
    two.joiner.run(MoveClip::new(other, PPQN, None));
    two.joiner.confirmed().clips.remove(other);
    two.settle();
    for _ in 0..3 {
        two.joiner.run(MoveClip::new(clip, PPQN * 4, None));
        two.settle();
    }
    assert_eq!(
        two.joiner.doc.clips[clip].start,
        start + PPQN * 12,
        "three drags of four beats are twelve beats"
    );
    assert_eq!(two.host.doc.clips[clip].start, start + PPQN * 12);
}

/// **Measured before the fix**: a copy that disagreed asked for a fresh
/// copy of the whole song at every edit that arrived while the first was on
/// its way — ten edits, ten songs down the wire.
#[test]
fn a_disagreement_asks_for_one_fresh_copy_not_one_an_edit() {
    let mut two = Two::new("one-resync", 6);
    let clip = two.host.doc.clips.keys().next().unwrap();
    // The joiner's copy of the host's song is off by one beat.
    two.joiner.confirmed().clips[clip].start += PPQN;
    for _ in 0..10 {
        two.host.run(MoveClip::new(clip, PPQN, None));
        two.tick();
    }
    two.settle();
    assert_eq!(*two.resyncs.lock().unwrap(), 1, "one fresh copy");
    assert_eq!(two.joiner.doc.sync_hash(), two.host.doc.sync_hash());
}

/// A joiner's link on which every song arrives with a fingerprint that is
/// not its own — what a build that writes songs differently looks like.
struct Damaging {
    inner: fontelle_net::MemoryTransport,
}

impl Transport for Damaging {
    fn send(&mut self, peer: PeerId, channel: Channel, bytes: &[u8]) {
        self.inner.send(peer, channel, bytes);
    }
    fn poll(&mut self) -> Vec<Incoming> {
        self.inner
            .poll()
            .into_iter()
            .map(|incoming| match incoming {
                Incoming::Message(peer, channel, bytes) => match Msg::from_bytes(&bytes) {
                    Ok(Msg::SnapshotDone { hash, seq }) => Incoming::Message(
                        peer,
                        channel,
                        Msg::SnapshotDone {
                            hash: hash ^ 1,
                            seq,
                        }
                        .to_bytes(),
                    ),
                    _ => Incoming::Message(peer, channel, bytes),
                },
                other => other,
            })
            .collect()
    }
    fn stats(&self, peer: PeerId) -> fontelle_net::LinkStats {
        self.inner.stats(peer)
    }
}

/// Ty, joining a friend from the phase 1 preview (2026-10-18): *"its
/// getting stuck on this"* — "YapEmporium is sending the song…" for good.
/// The preview wrote songs without their solos and the friend's 0.28.0 with
/// them, so the fingerprints never agreed; the join said so on the status
/// line and went on waiting.
#[test]
fn a_song_that_arrives_damaged_ends_the_join_and_says_why() {
    let dir = std::env::temp_dir().join(format!("fontelle-collab-damaged-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    let hub = MemoryHub::new();
    let mut host = Side {
        collab: Collab::host(Box::new(hub.server_endpoint()), options("Host")),
        doc: crate::blank_project(8, 120.0, 48_000),
        history: History::new(),
        place: Place {
            bundle: None,
            banks: Vec::new(),
        },
    };
    host.history.open_outbox();
    let mut joiner = Side {
        collab: Collab::join(
            Box::new(Damaging {
                inner: hub.connect(),
            }),
            options("Joiner"),
            dir.clone(),
        ),
        doc: Project::new("nothing yet"),
        history: History::new(),
        place: Place {
            bundle: None,
            banks: Vec::new(),
        },
    };
    let mut now = 0;
    let mut tick = |host: &mut Side, joiner: &mut Side| {
        now += 1;
        hub.set_now(now);
        host.pump();
        joiner.pump()
    };
    for _ in 0..60 {
        tick(&mut host, &mut joiner);
    }
    let mut effects = joiner
        .collab
        .answer(JoinAnswer::Copy, &mut joiner.history)
        .unwrap();
    for _ in 0..60 {
        effects.extend(tick(&mut host, &mut joiner));
    }
    assert!(
        !effects.iter().any(|e| matches!(e, Effect::Open(_))),
        "a damaged song was opened"
    );
    let why = joiner.collab.ended().expect("the join ended");
    assert!(why.contains("damaged"), "{why}");
    assert!(why.contains("same version"), "{why}");
    std::fs::remove_dir_all(&dir).ok();
}

/// A link that says when it has been let go of.
struct Watched {
    inner: fontelle_net::MemoryTransport,
    dropped: Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for Watched {
    fn drop(&mut self) {
        self.dropped
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Transport for Watched {
    fn send(&mut self, peer: PeerId, channel: Channel, bytes: &[u8]) {
        self.inner.send(peer, channel, bytes);
    }
    fn poll(&mut self) -> Vec<Incoming> {
        self.inner.poll()
    }
    fn stats(&self, peer: PeerId) -> fontelle_net::LinkStats {
        self.inner.stats(peer)
    }
}

/// A session that has ended lets go of its link once the goodbye is out,
/// so the connection closes then — and the relay, and everyone still on it,
/// hear the host has gone — rather than when the studio is next shared or
/// closed. It used to be held, open and idle, until then.
#[test]
fn an_ended_session_lets_go_of_its_link() {
    let hub = MemoryHub::new();
    let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut host = Side {
        collab: Collab::host(
            Box::new(Watched {
                inner: hub.server_endpoint(),
                dropped: dropped.clone(),
            }),
            options("Host"),
        ),
        doc: crate::blank_project(8, 120.0, 48_000),
        history: History::new(),
        place: Place {
            bundle: None,
            banks: Vec::new(),
        },
    };
    host.history.open_outbox();
    host.pump();
    host.collab.leave(&mut host.doc, &mut host.history);
    for _ in 0..3 {
        host.pump();
    }
    assert!(
        dropped.load(std::sync::atomic::Ordering::SeqCst),
        "the link is still held"
    );
}

/// A link that holds back what it is given until it has been looked at a
/// few times — a relay's pace.
struct Slow {
    backlog: usize,
}

impl Transport for Slow {
    fn send(&mut self, _: PeerId, _: Channel, _: &[u8]) {
        self.backlog += 1;
    }
    fn poll(&mut self) -> Vec<Incoming> {
        self.backlog = self.backlog.saturating_sub(1);
        Vec::new()
    }
    fn stats(&self, _: PeerId) -> fontelle_net::LinkStats {
        fontelle_net::LinkStats::default()
    }
    fn backlog(&self) -> usize {
        self.backlog
    }
}

/// Closing Fontelle straight after Stop sharing used to take the goodbye
/// with it: it was still waiting on the relay's pace when the process went,
/// and everyone else waited out the relay's grace instead of hearing at once.
#[test]
fn closing_waits_a_moment_for_the_goodbye() {
    let mut collab = Collab::host(Box::new(Slow { backlog: 5 }), options("Host"));
    let mut doc = crate::blank_project(8, 120.0, 48_000);
    let mut history = History::new();
    history.open_outbox();
    collab.leave(&mut doc, &mut history);
    assert!(collab.flushing());
    collab.flush(Duration::from_secs(1));
    assert!(!collab.flushing(), "the goodbye is still waiting");
}

// --- presence (next update, phase 2.2) ---

fn at(tick: f64) -> fontelle_model::wire::Presence {
    fontelle_model::wire::Presence {
        playhead: tick,
        ..Default::default()
    }
}

fn whereabouts(side: &Side) -> Vec<(u16, f64)> {
    side.collab
        .presences()
        .into_iter()
        .map(|(peer, presence)| (peer, presence.playhead))
        .collect()
}

#[test]
fn whereabouts_reach_the_other_side() {
    let mut two = Two::new("whereabouts", 2);
    two.joiner.collab.set_presence(at(480.0));
    two.host.collab.set_presence(at(960.0));
    two.settle();
    let joiner = match &two.joiner.collab.role {
        Role::Joiner(joiner) => joiner.space,
        Role::Host(_) => unreachable!(),
    };
    assert_eq!(whereabouts(&two.host), vec![(joiner, 480.0)]);
    assert_eq!(whereabouts(&two.joiner), vec![(0, 960.0)]);
}

/// The session holds everything from outside while a drag is in the hand
/// (F17) — and used to hold where everybody was with it, so every cursor
/// froze for as long as anybody dragged anything.
#[test]
fn whereabouts_keep_coming_during_a_drag() {
    let mut two = Two::new("whereabouts-drag", 2);
    two.host.collab.options.idle_break = Duration::from_secs(600);
    let track = a_track_that_is_not_the_master(&two.host.doc);
    // A step of a drag, applied and left in the hand.
    two.host
        .history
        .apply(
            Box::new(AddInsert::new(track, EffectKind::Eq)),
            &mut two.host.doc,
        )
        .unwrap();
    assert!(two.host.history.gesture_in_hand());
    two.joiner.collab.set_presence(at(1920.0));
    two.settle();
    assert_eq!(
        whereabouts(&two.host).first().map(|w| w.1),
        Some(1920.0),
        "the cursor froze under the drag"
    );
    assert!(two.host.history.gesture_in_hand(), "the drag is still held");
}

/// Fifteen times a second at most, and only when something moved: a pointer
/// held still sends nothing.
#[test]
fn whereabouts_go_when_they_change_and_no_faster_than_fifteen_a_second() {
    let mut two = Two::new("whereabouts-rate", 2);
    two.joiner.collab.set_presence(at(1.0));
    let first = two.joiner.collab.presence_sent();
    for _ in 0..20 {
        two.joiner.collab.set_presence(at(1.0));
        two.joiner.pump();
    }
    assert_eq!(two.joiner.collab.presence_sent(), first + 1, "still sends");
    two.joiner.collab.set_presence(at(2.0));
    two.joiner.pump();
    two.joiner.collab.set_presence(at(3.0));
    two.joiner.pump();
    assert_eq!(
        two.joiner.collab.presence_sent(),
        first + 1,
        "sent again within a fifteenth of a second"
    );
    std::thread::sleep(Duration::from_millis(70));
    two.joiner.pump();
    assert_eq!(two.joiner.collab.presence_sent(), first + 2);
    two.settle();
    assert_eq!(whereabouts(&two.host).first().map(|w| w.1), Some(3.0));
}

/// Three studios: whereabouts go through the host, since joiners only talk
/// to it, and somebody who leaves takes theirs with them.
#[test]
fn whereabouts_go_round_through_the_host_and_leave_with_their_owner() {
    let mut two = Two::new("whereabouts-three", 2);
    let dir = two.dir.join("carol");
    std::fs::create_dir_all(&dir).unwrap();
    let mut carol = Side {
        collab: Collab::join(Box::new(two.hub.connect()), options("Carol"), dir),
        doc: Project::new("nothing yet"),
        history: History::new(),
        place: Place {
            bundle: None,
            banks: Vec::new(),
        },
    };
    let tick = |two: &mut Two, carol: &mut Side| {
        two.tick();
        carol.pump()
    };
    for _ in 0..60 {
        tick(&mut two, &mut carol);
    }
    let mut effects = carol
        .collab
        .answer(JoinAnswer::Copy, &mut carol.history)
        .unwrap();
    for _ in 0..200 {
        effects.extend(tick(&mut two, &mut carol));
        if let Some(path) = std::mem::take(&mut effects)
            .into_iter()
            .find_map(|e| match e {
                Effect::Open(path) => Some(path),
                _ => None,
            })
        {
            carol.doc = fontelle_model::load_project(&path).unwrap();
            let Side {
                collab,
                doc,
                history,
                place,
            } = &mut carol;
            collab.went_live(doc, history, place);
        }
    }
    assert!(carol.collab.is_live());
    two.joiner.collab.set_presence(at(7.0));
    for _ in 0..60 {
        tick(&mut two, &mut carol);
    }
    let bob = match &two.joiner.collab.role {
        Role::Joiner(joiner) => joiner.space,
        Role::Host(_) => unreachable!(),
    };
    assert_eq!(whereabouts(&carol), vec![(bob, 7.0)]);
    two.joiner
        .collab
        .leave(&mut two.joiner.doc, &mut two.joiner.history);
    for _ in 0..60 {
        tick(&mut two, &mut carol);
    }
    assert!(whereabouts(&carol).is_empty(), "{:?}", whereabouts(&carol));
    assert!(whereabouts(&two.host).is_empty());
}

// --- chat (phase 2.3) ---

fn lines(side: &Side) -> Vec<(String, String)> {
    side.collab
        .chat()
        .into_iter()
        .map(|line| (line.name, line.text))
        .collect()
}

/// What one person says reaches everybody, under their name, in the order
/// it was said — and their own line is in their own list at once.
#[test]
fn what_is_said_reaches_everybody_in_order() {
    let mut two = Two::new("chat", 2);
    two.joiner.collab.say("hi Alice");
    assert_eq!(
        lines(&two.joiner),
        vec![("Joiner".to_string(), "hi Alice".to_string())],
        "your own line shows at once"
    );
    two.settle();
    two.host.collab.say("  hey  ");
    two.settle();
    let both = vec![
        ("Joiner".to_string(), "hi Alice".to_string()),
        ("Host".to_string(), "hey".to_string()),
    ];
    assert_eq!(lines(&two.host), both);
    assert_eq!(lines(&two.joiner), both);
    assert!(two.joiner.collab.chat()[0].mine);
    assert!(!two.host.collab.chat()[0].mine);
}

/// Nothing is said by a blank line, a novel is cut short, and only the
/// last two hundred lines are kept.
#[test]
fn chat_keeps_a_sensible_amount() {
    let mut two = Two::new("chat-limits", 2);
    two.joiner.collab.say("   ");
    assert!(two.joiner.collab.chat().is_empty());
    two.joiner.collab.say(&"a".repeat(2000));
    assert_eq!(
        two.joiner.collab.chat()[0].text.chars().count(),
        CHAT_LONGEST
    );
    for n in 0..250 {
        two.host.collab.say(&format!("line {n}"));
    }
    two.settle();
    let kept = two.joiner.collab.chat();
    assert_eq!(kept.len(), CHAT_KEPT);
    assert_eq!(kept.last().unwrap().text, "line 249");
}

/// A line from somebody else is unread until the person looks.
#[test]
fn a_line_from_somebody_else_is_unread_until_looked_at() {
    let mut two = Two::new("chat-unread", 2);
    two.joiner.collab.say("psst");
    two.settle();
    assert_eq!(two.host.collab.chat_unread(), 1);
    assert_eq!(two.joiner.collab.chat_unread(), 0, "your own line is read");
    two.host.collab.read_chat();
    assert_eq!(two.host.collab.chat_unread(), 0);
}

// --- permissions (phase 2.4) ---

fn joiner_space(two: &Two) -> u16 {
    match &two.joiner.collab.role {
        Role::Joiner(joiner) => joiner.space,
        Role::Host(_) => unreachable!(),
    }
}

fn a_plugin() -> fontelle_types::PluginState {
    fontelle_types::PluginState::new(
        fontelle_types::PluginKey::new(fontelle_types::PluginFormat::Clap, "com.example.diva"),
        "Diva",
    )
}

/// The host decides, person by person, what each may touch beyond the
/// notes; what is not allowed is refused and taken back, with a sentence
/// that says which permission it needed.
#[test]
fn a_person_kept_off_the_mixer_cannot_change_it_but_can_still_write() {
    let mut two = Two::new("perm-mixer", 2);
    let bob = joiner_space(&two);
    two.host
        .collab
        .set_permissions(
            bob,
            fontelle_model::wire::Permissions {
                mixer: false,
                ..Default::default()
            },
        )
        .unwrap();
    two.settle();
    assert!(!two.joiner.collab.my_permissions().mixer, "Bob is told");
    let track = a_track_that_is_not_the_master(&two.joiner.doc);
    let inserts = two.host.doc.mixer.tracks[track].inserts.len();
    two.joiner.run(AddInsert::new(track, EffectKind::Eq));
    two.settle();
    assert_eq!(two.host.doc.mixer.tracks[track].inserts.len(), inserts);
    assert_eq!(two.joiner.doc.mixer.tracks[track].inserts.len(), inserts);
    let said = two.joiner.collab.take_notices();
    assert!(said.iter().any(|n| n.contains("mixer")), "{said:?}");
    // The notes are still Bob's to write.
    let lane = two.joiner.doc.lane_ids()[0];
    let channel = two.joiner.doc.channels.keys().next().unwrap();
    let clips = two.host.doc.clips.len();
    two.joiner
        .run(AddClip::new(a_clip(lane, channel, PPQN * 64)));
    two.settle();
    assert_eq!(two.host.doc.clips.len(), clips + 1);
}

#[test]
fn a_person_kept_off_the_sounds_cannot_change_an_instruments_sound() {
    let mut two = Two::new("perm-sounds", 2);
    let bob = joiner_space(&two);
    two.host
        .collab
        .set_permissions(
            bob,
            fontelle_model::wire::Permissions {
                sounds: false,
                ..Default::default()
            },
        )
        .unwrap();
    two.settle();
    let channel = two.joiner.doc.channels.keys().next().unwrap();
    let before = two.host.doc.channels[channel].patch_data.clone();
    two.joiner
        .run(fontelle_model::SetChannelPatch::new(channel, None));
    two.settle();
    assert_eq!(two.host.doc.channels[channel].patch_data, before);
    let said = two.joiner.collab.take_notices();
    assert!(said.iter().any(|n| n.contains("sound")), "{said:?}");
}

/// A plugin the host has is anybody's to add; one it has not got needs the
/// permission, since nobody else will hear it either.
#[test]
fn a_plugin_the_host_has_not_got_needs_permission() {
    let mut two = Two::new("perm-plugins", 2);
    let bob = joiner_space(&two);
    two.host
        .collab
        .set_permissions(
            bob,
            fontelle_model::wire::Permissions {
                plugins: false,
                ..Default::default()
            },
        )
        .unwrap();
    two.settle();
    let track = a_track_that_is_not_the_master(&two.joiner.doc);
    let inserts = two.host.doc.mixer.tracks[track].inserts.len();
    two.joiner
        .run(fontelle_model::AddPluginInsert::new(track, a_plugin()));
    two.settle();
    assert_eq!(
        two.host.doc.mixer.tracks[track].inserts.len(),
        inserts,
        "a plugin the host lacks went in"
    );
    two.host.collab.set_plugins_here(vec![a_plugin().key]);
    two.joiner
        .run(fontelle_model::AddPluginInsert::new(track, a_plugin()));
    two.settle();
    assert_eq!(two.host.doc.mixer.tracks[track].inserts.len(), inserts + 1);
}

// --- who may come in (Ty, 2026-10-19) ---
//
// *"removing someone shouldnt block them from the session, that should be
// something totally different, and we should try our best to implement
// security so that users can be blocked from sessions and stuff like if
// there are bad actors and stuff, maybe making password only sessions,
// etc. just ensure odd gaps like this dont make it in"*

/// Pumps everybody until `side` has the song open or its join has ended:
/// it gives `password` whenever it is asked for one (once an asking), and
/// takes a copy. `true` when it is in.
fn bring_in(two: &mut Two, side: &mut Side, password: Option<&str>) -> bool {
    let mut effects = Vec::new();
    let mut answered = false;
    let mut given = None;
    for _ in 0..400 {
        two.tick();
        effects.extend(side.pump());
        if let Some(ask) = side.collab.password_wanted()
            && given != Some(ask)
            && let Some(password) = password
        {
            given = Some(ask);
            side.collab.give_password(password);
        }
        if !answered && side.collab.question().is_some() {
            effects.extend(
                side.collab
                    .answer(JoinAnswer::Copy, &mut side.history)
                    .unwrap(),
            );
            answered = true;
        }
        if let Some(path) = std::mem::take(&mut effects)
            .into_iter()
            .find_map(|e| match e {
                Effect::Open(path) => Some(path),
                _ => None,
            })
        {
            side.doc = fontelle_model::load_project(&path).unwrap();
            let Side {
                collab,
                doc,
                history,
                place,
            } = side;
            collab.went_live(doc, history, place);
        }
        if side.collab.is_live() || side.collab.ended().is_some() {
            break;
        }
    }
    for _ in 0..10 {
        two.tick();
        side.pump();
    }
    side.collab.is_live()
}

/// Pumps the pair and `side` together for a while.
fn run_with(two: &mut Two, side: &mut Side, ticks: usize) {
    for _ in 0..ticks {
        two.tick();
        side.pump();
    }
}

fn joiner_identity(two: &Two) -> Identity {
    two.joiner.collab.options.identity.clone()
}

/// Remove is a kick, and only that: the person is told who did it and let
/// go of, and the code still lets them in — the same studio, joining again,
/// is back in the song.
#[test]
fn somebody_removed_can_come_back_with_the_code() {
    let mut two = Two::new("removed-back", 2);
    let bob = joiner_space(&two);
    two.host.collab.remove(bob, &mut two.host.doc).unwrap();
    two.settle();
    let why = two.joiner.collab.ended().expect("told").to_string();
    assert!(why.contains("Host removed you"), "{why}");
    assert!(two.host.collab.peers().is_empty());

    let mut again = two.another("Joiner", Some(joiner_identity(&two)));
    assert!(
        bring_in(&mut two, &mut again, None),
        "a removed person can come back: {:?}",
        again.collab.ended()
    );
    assert_eq!(two.host.collab.peers().len(), 1);
}

/// Block is the other thing: they are let go of, and the studio that proved
/// who it was can't come back — told only that it can't join, never why.
/// Somebody else can still join.
#[test]
fn somebody_blocked_cannot_come_back_and_is_not_told_why() {
    let mut two = Two::new("blocked", 2);
    let bob = joiner_space(&two);
    let identity = joiner_identity(&two);
    let blocked = two.host.collab.block(bob, &mut two.host.doc).unwrap();
    assert_eq!(blocked.key, identity.public_key());
    assert_eq!(blocked.name, "Joiner");
    two.settle();
    assert!(two.joiner.collab.ended().is_some(), "let go of");
    assert!(two.host.collab.peers().is_empty());

    let mut again = two.another("Joiner", Some(identity.clone()));
    assert!(!bring_in(&mut two, &mut again, None));
    assert_eq!(again.collab.ended(), Some(CANT_JOIN));
    // Under another name, the same key is the same person.
    let mut renamed = two.another("Totally Not Bob", Some(identity));
    assert!(!bring_in(&mut two, &mut renamed, None));
    assert_eq!(renamed.collab.ended(), Some(CANT_JOIN));

    let mut carol = two.another("Carol", None);
    assert!(
        bring_in(&mut two, &mut carol, None),
        "somebody else still can"
    );
}

/// Somebody blocked in an earlier session is refused in every later one:
/// the host's settings carry the keys into the next share.
#[test]
fn a_key_blocked_before_is_refused_in_a_new_session() {
    let identity = Identity::generate();
    let blocked = identity.public_key();
    let mut two = Two::new("blocked-before", 2);
    // A host that starts with the list, as a share from the settings does.
    let mut options = options("Host");
    options.blocked = vec![blocked.clone()];
    two.host.collab = Collab::host(Box::new(two.hub.server_endpoint()), options);
    let mut bob = two.another("Bob", Some(identity.clone()));
    assert!(!bring_in(&mut two, &mut bob, None));
    assert_eq!(bob.collab.ended(), Some(CANT_JOIN));

    // Unblocked, they are let in.
    two.host.collab.unblock(&blocked);
    let mut back = two.another("Bob", Some(identity));
    assert!(
        bring_in(&mut two, &mut back, None),
        "{:?}",
        back.collab.ended()
    );
}

/// With a password, the right one lets you in and a wrong one asks again —
/// saying it was wrong — rather than ending the join.
#[test]
fn a_password_session_lets_in_only_the_right_password() {
    let mut two = Two::with(
        "password",
        2,
        Setup {
            host_password: Some("hunter2"),
            joiner_password: Some("hunter2"),
            ..Setup::default()
        },
    );
    assert!(two.host.collab.has_password());
    let mut carol = two.another("Carol", None);
    for _ in 0..40 {
        two.tick();
        carol.pump();
        if carol.collab.password_wanted().is_some() {
            break;
        }
    }
    let ask = carol.collab.password_wanted().expect("asked for one");
    assert!(!ask.wrong);
    carol.collab.give_password("hunter3");
    assert_eq!(carol.collab.password_wanted(), None, "answered");
    run_with(&mut two, &mut carol, 40);
    let again = carol.collab.password_wanted().expect("asked again");
    assert!(again.wrong, "told it was wrong");
    assert!(again.attempt > ask.attempt);
    assert!(carol.collab.ended().is_none(), "and may try again");
    assert!(bring_in(&mut two, &mut carol, Some("hunter2")));
    assert_eq!(two.host.collab.peers().len(), 2);
}

/// Five wrong passwords on one connection, and it is let go of.
#[test]
fn five_wrong_passwords_end_the_join() {
    let mut two = Two::with(
        "password-five",
        2,
        Setup {
            host_password: Some("hunter2"),
            joiner_password: Some("hunter2"),
            ..Setup::default()
        },
    );
    let mut carol = two.another("Carol", None);
    let mut tries = 0;
    for _ in 0..600 {
        two.tick();
        carol.pump();
        if carol.collab.password_wanted().is_some() {
            tries += 1;
            carol.collab.give_password("guess");
        }
        if carol.collab.ended().is_some() {
            break;
        }
    }
    assert_eq!(tries, 5);
    let why = carol.collab.ended().expect("ended");
    assert!(why.contains("Too many wrong passwords"), "{why}");
    assert_eq!(two.host.collab.peers().len(), 1, "only the first joiner");
}

/// Escape at the password: the join ends cleanly, said as the person's own.
#[test]
fn a_join_abandoned_at_the_password_ends_cleanly() {
    let mut two = Two::with(
        "password-cancel",
        2,
        Setup {
            host_password: Some("hunter2"),
            joiner_password: Some("hunter2"),
            ..Setup::default()
        },
    );
    let mut carol = two.another("Carol", None);
    for _ in 0..40 {
        two.tick();
        carol.pump();
    }
    assert!(carol.collab.password_wanted().is_some());
    carol
        .collab
        .answer(JoinAnswer::Cancel, &mut carol.history)
        .unwrap();
    assert_eq!(carol.collab.ended(), Some("You cancelled the join."));
    assert_eq!(carol.collab.password_wanted(), None);
    run_with(&mut two, &mut carol, 20);
    assert_eq!(two.host.collab.peers().len(), 1);
}

/// Every frame a link carried, with the channel it went on.
type Frames = Arc<Mutex<Vec<(Channel, Vec<u8>)>>>;

/// Every frame a link carries, both ways, with the channel it went on.
struct Recording {
    inner: Box<dyn Transport>,
    frames: Frames,
}

impl Transport for Recording {
    fn send(&mut self, peer: PeerId, channel: Channel, bytes: &[u8]) {
        self.frames.lock().unwrap().push((channel, bytes.to_vec()));
        self.inner.send(peer, channel, bytes);
    }
    fn poll(&mut self) -> Vec<Incoming> {
        let got = self.inner.poll();
        for incoming in &got {
            if let Incoming::Message(_, channel, bytes) = incoming {
                self.frames.lock().unwrap().push((*channel, bytes.clone()));
            }
        }
        got
    }
    fn stats(&self, peer: PeerId) -> fontelle_net::LinkStats {
        self.inner.stats(peer)
    }
    fn backlog(&self) -> usize {
        self.inner.backlog()
    }
}

fn recorded(frames: &Frames) -> Wrap {
    let frames = frames.clone();
    Box::new(move |_, link| {
        Box::new(Recording {
            inner: Box::new(link),
            frames,
        })
    })
}

/// A password session seals everything after the handshake, both ways and
/// on every channel — the song, the edits, whereabouts (fire-and-forget)
/// and a file's pieces: under the choke point nothing but the handshake is
/// in the clear, and the password itself never crosses at all.
#[test]
fn everything_after_the_handshake_is_sealed_whereabouts_and_files_too() {
    let host_frames = Arc::new(Mutex::new(Vec::new()));
    let joiner_frames = Arc::new(Mutex::new(Vec::new()));
    let mut two = Two::with(
        "sealed",
        2,
        Setup {
            host_password: Some("correct horse"),
            joiner_password: Some("correct horse"),
            host_link: Some(recorded(&host_frames)),
            joiner_link: Some(recorded(&joiner_frames)),
        },
    );
    // Whereabouts, both ways.
    two.joiner.collab.set_presence(at(480.0));
    two.host.collab.set_presence(at(960.0));
    // An edit, both ways.
    let clip = two.host.doc.clips.keys().next().unwrap();
    two.host.run(MoveClip::new(clip, PPQN, None));
    two.settle();
    two.joiner.run(MoveClip::new(clip, PPQN, None));
    two.settle();
    // A file, in pieces.
    let pack = two.dir.join("voice.pack");
    let bytes: Vec<u8> = (0..(CHUNK * 2 + 777))
        .map(|i| (i * 31 % 251) as u8)
        .collect();
    std::fs::write(&pack, &bytes).unwrap();
    let hash = fontelle_assets::content_hash::hash_file(&pack).unwrap().low;
    two.host
        .collab
        .offer_singers(Vec::new(), vec![(hash, pack.clone())]);
    two.joiner.collab.fetch_singer(&SingerNeed {
        engine: "fontelle".into(),
        singer: "voice".into(),
        name: "Voice".into(),
        offer: SingerOffer::Send {
            hash,
            size: bytes.len() as u64,
            file_name: "voice.pack".into(),
            terms: None,
        },
    });
    for _ in 0..200 {
        two.tick();
    }
    // It all arrived.
    assert_eq!(whereabouts(&two.host).first().map(|w| w.1), Some(480.0));
    assert_eq!(whereabouts(&two.joiner), vec![(0, 960.0)]);
    assert_eq!(two.joiner.doc.sync_hash(), two.host.doc.sync_hash());
    let landed = two.joiner.collab.singer_pack(hash).expect("the file came");
    assert_eq!(std::fs::read(landed).unwrap(), bytes);

    for (whose, frames) in [("host", &host_frames), ("joiner", &joiner_frames)] {
        let frames = frames.lock().unwrap();
        let mut sealed_unreliable = 0;
        let mut sealed = 0;
        for (channel, frame) in frames.iter() {
            if frame.starts_with(&fontelle_model::wire::SEALED) {
                sealed += 1;
                if *channel == Channel::Unreliable {
                    sealed_unreliable += 1;
                }
                continue;
            }
            let msg = Msg::from_bytes(frame).expect("a clear frame is a message");
            assert!(
                matches!(
                    msg,
                    Msg::Session { .. }
                        | Msg::Hello { .. }
                        | Msg::Challenge { .. }
                        | Msg::Answer { .. }
                        | Msg::Refuse { .. }
                        | Msg::Waiting { .. }
                ),
                "{whose}'s link carried {msg:?} in the clear"
            );
            let text = String::from_utf8_lossy(frame);
            assert!(!text.contains("correct horse"), "the password crossed");
        }
        assert!(sealed > 10, "{whose}: {sealed} sealed frames");
        assert!(sealed_unreliable > 0, "{whose}: whereabouts went unsealed");
    }
    // And through the one choke point: what it sealed is what went.
    assert!(two.host.collab.transport.sealed_out() > 0);
    assert!(two.joiner.collab.transport.opened_in() > 0);
}

/// A link that spoils or repeats what reaches it, on request.
struct Spoiling {
    inner: Box<dyn Transport>,
    spoil: Arc<std::sync::atomic::AtomicBool>,
    replay: Arc<std::sync::atomic::AtomicBool>,
    last: Option<Incoming>,
}

impl Transport for Spoiling {
    fn send(&mut self, peer: PeerId, channel: Channel, bytes: &[u8]) {
        self.inner.send(peer, channel, bytes);
    }
    fn poll(&mut self) -> Vec<Incoming> {
        use std::sync::atomic::Ordering;
        let mut got = self.inner.poll();
        for incoming in &mut got {
            if let Incoming::Message(_, _, bytes) = incoming {
                if self.spoil.swap(false, Ordering::SeqCst) {
                    let last = bytes.len() - 1;
                    bytes[last] ^= 0x40;
                }
                self.last = Some(incoming.clone());
            }
        }
        if self.replay.swap(false, Ordering::SeqCst)
            && let Some(last) = self.last.clone()
        {
            got.push(last);
        }
        got
    }
    fn stats(&self, peer: PeerId) -> fontelle_net::LinkStats {
        self.inner.stats(peer)
    }
}

/// The choke point on its own: a sealed frame opens on the other side; one
/// spoiled on the way, or the same one again, is dropped — never a panic;
/// and a frame in the clear on a sealed link is dropped unless it is the
/// handshake's.
#[test]
fn a_frame_that_does_not_open_is_dropped_and_so_is_a_replay() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let hub = MemoryHub::new();
    let mut host = sealed::Sealed::new(Box::new(hub.server_endpoint()), sealed::End::Host);
    let spoil = Arc::new(AtomicBool::new(false));
    let replay = Arc::new(AtomicBool::new(false));
    let mut joiner = sealed::Sealed::new(
        Box::new(Spoiling {
            inner: Box::new(hub.connect()),
            spoil: spoil.clone(),
            replay: replay.clone(),
            last: None,
        }),
        sealed::End::Joiner,
    );
    let peer = match host.poll().as_slice() {
        [Incoming::Connected(peer)] => *peer,
        other => panic!("{other:?}"),
    };
    let keys = identity::PasswordKeys::derive("pw", "ABCDEF").link(&[5; 32]);
    host.arm(peer, &keys);
    joiner.arm(fontelle_net::SERVER, &keys);
    let said = |text: &str| Msg::Chat {
        peer: 0,
        text: text.into(),
    };
    let heard = |joiner: &mut sealed::Sealed| -> Vec<String> {
        joiner
            .poll()
            .into_iter()
            .filter_map(|incoming| match incoming {
                Incoming::Message(_, _, bytes) => match Msg::from_bytes(&bytes) {
                    Ok(Msg::Chat { text, .. }) => Some(text),
                    Ok(Msg::Challenge { .. }) => Some("challenge".into()),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    };

    host.send(peer, Channel::Reliable, &said("one").to_bytes());
    assert_eq!(heard(&mut joiner), vec!["one"]);

    spoil.store(true, Ordering::SeqCst);
    host.send(peer, Channel::Reliable, &said("two").to_bytes());
    assert!(heard(&mut joiner).is_empty(), "a spoiled frame opened");
    assert_eq!(joiner.dropped_in(), 1);

    host.send(peer, Channel::Unreliable, &said("three").to_bytes());
    assert_eq!(heard(&mut joiner), vec!["three"]);
    replay.store(true, Ordering::SeqCst);
    assert!(heard(&mut joiner).is_empty(), "a frame heard twice");

    // In the clear, under the seal: only the handshake gets through.
    let mut raw = hub.server_endpoint();
    raw.send(peer, Channel::Reliable, &said("forged").to_bytes());
    raw.send(
        peer,
        Channel::Reliable,
        &Msg::Challenge {
            nonce: vec![1; 32],
            password: true,
        }
        .to_bytes(),
    );
    assert_eq!(heard(&mut joiner), vec!["challenge"]);
    // Garbage is dropped too.
    raw.send(peer, Channel::Reliable, &[0xfe, 0x5e, 1, 2, 3]);
    raw.send(peer, Channel::Reliable, &[]);
    assert!(heard(&mut joiner).is_empty());
}

/// The host changes the password while people are in: those already in keep
/// the keys they came in with, and the session goes on for them exactly as
/// before; the next person needs the new one.
#[test]
fn changing_the_password_keeps_those_already_in() {
    let mut two = Two::with(
        "password-change",
        2,
        Setup {
            host_password: Some("first"),
            joiner_password: Some("first"),
            ..Setup::default()
        },
    );
    two.host.collab.set_password(Some("second".into()));
    let clip = two.host.doc.clips.keys().next().unwrap();
    two.joiner.run(MoveClip::new(clip, PPQN, None));
    two.settle();
    two.host.run(MoveClip::new(clip, PPQN * 2, None));
    two.joiner.collab.say("still here");
    two.settle();
    assert_eq!(two.joiner.doc.sync_hash(), two.host.doc.sync_hash());
    assert_eq!(lines(&two.host).last().unwrap().1, "still here");

    let mut carol = two.another("Carol", None);
    let mut asked = 0;
    for _ in 0..200 {
        two.tick();
        carol.pump();
        if let Some(ask) = carol.collab.password_wanted() {
            asked += 1;
            if ask.wrong {
                break;
            }
            carol.collab.give_password("first");
        }
    }
    assert!(asked >= 2, "the old password let Carol in");
    assert!(bring_in(&mut two, &mut carol, Some("second")));

    // Cleared: nobody new is asked, and those in are still sealed and well.
    two.host.collab.set_password(None);
    assert!(!two.host.collab.has_password());
    let mut dave = two.another("Dave", None);
    assert!(bring_in(&mut two, &mut dave, None));
    two.host.run(MoveClip::new(clip, PPQN, None));
    two.settle();
    assert_eq!(two.joiner.doc.sync_hash(), two.host.doc.sync_hash());
}

/// *Ask before letting people in*: a newcomer who proved who they are waits,
/// told so, until the host lets them in — and several wait in turn.
#[test]
fn ask_first_holds_newcomers_until_the_host_answers() {
    let mut two = Two::new("ask-first", 2);
    two.host.collab.set_ask_first(true);
    let mut carol = two.another("Carol", None);
    let mut dave = two.another("Dave", None);
    for _ in 0..60 {
        two.tick();
        carol.pump();
        dave.pump();
    }
    assert_eq!(two.host.collab.knocks(), vec!["Carol", "Dave"]);
    assert_eq!(carol.collab.waiting_for_host(), Some("Host"));
    assert!(carol.collab.question().is_none(), "nothing sent yet");
    assert_eq!(two.host.collab.peers().len(), 1);

    assert!(
        two.host
            .collab
            .answer_knock(Knock::LetIn, &mut two.host.doc)
            .is_none()
    );
    assert_eq!(two.host.collab.knocks(), vec!["Dave"]);
    assert!(bring_in(&mut two, &mut carol, None));

    two.host
        .collab
        .answer_knock(Knock::TurnAway, &mut two.host.doc);
    for _ in 0..40 {
        two.tick();
        dave.pump();
    }
    let why = dave.collab.ended().expect("turned away");
    assert!(why.contains("Host didn't let you in"), "{why}");
    assert!(two.host.collab.knocks().is_empty());

    // Turned away is not blocked: Dave may knock again.
    let mut dave = two.another("Dave", Some(dave.collab.options.identity.clone()));
    for _ in 0..40 {
        two.tick();
        dave.pump();
    }
    assert_eq!(two.host.collab.knocks(), vec!["Dave"]);
    let blocked = two
        .host
        .collab
        .answer_knock(Knock::Block, &mut two.host.doc)
        .expect("blocked");
    assert_eq!(blocked.name, "Dave");
    for _ in 0..40 {
        two.tick();
        dave.pump();
    }
    assert_eq!(dave.collab.ended(), Some(CANT_JOIN));
}

/// Somebody who leaves while waiting is no longer waiting.
#[test]
fn a_knock_that_leaves_is_forgotten() {
    let mut two = Two::new("ask-first-leaves", 2);
    two.host.collab.set_ask_first(true);
    let mut carol = two.another("Carol", None);
    run_with(&mut two, &mut carol, 40);
    assert_eq!(two.host.collab.knocks(), vec!["Carol"]);
    carol
        .collab
        .answer(JoinAnswer::Cancel, &mut carol.history)
        .unwrap();
    run_with(&mut two, &mut carol, 40);
    assert!(two.host.collab.knocks().is_empty());
}

/// *Lock*: nobody new at all, in a sentence that says so.
#[test]
fn a_locked_session_takes_nobody_new() {
    let mut two = Two::new("locked", 2);
    two.host.collab.set_locked(true);
    let mut carol = two.another("Carol", None);
    assert!(!bring_in(&mut two, &mut carol, None));
    assert_eq!(carol.collab.ended(), Some(LOCKED));
    two.host.collab.set_locked(false);
    let mut carol = two.another("Carol", None);
    assert!(bring_in(&mut two, &mut carol, None));
}

/// The proof names the session it was made for: one made with another code
/// does not get in.
#[test]
fn a_proof_made_for_another_session_is_refused() {
    let mut two = Two::new("other-code", 2);
    let mut options = options("Host");
    options.code = "ABC123".into();
    two.host.collab = Collab::host(Box::new(two.hub.server_endpoint()), options);
    let mut mallory = two.another("Mallory", None);
    mallory.collab.options.code = "ZZZ999".into();
    assert!(!bring_in(&mut two, &mut mallory, None));
    let why = mallory.collab.ended().unwrap();
    assert!(why.contains("couldn't prove"), "{why}");

    let mut carol = two.another("Carol", None);
    carol.collab.options.code = "abc-123".into();
    assert!(
        bring_in(&mut two, &mut carol, None),
        "however the code was typed: {:?}",
        carol.collab.ended()
    );
}

/// A connection that says hello and never answers the challenge is let go
/// of once the host has waited long enough.
#[test]
fn a_connection_that_never_answers_is_dropped() {
    let mut two = Two::new("silent", 2);
    let mut options = options("Host");
    options.answer_within = Duration::from_millis(30);
    two.host.collab = Collab::host(Box::new(two.hub.server_endpoint()), options);
    let mut silent = two.hub.connect();
    silent.send(
        fontelle_net::SERVER,
        Channel::Reliable,
        &Msg::Hello {
            protocol: PROTOCOL,
            fontelle: env!("CARGO_PKG_VERSION").into(),
            name: "Silent".into(),
            install: PersistentId::new(),
        }
        .to_bytes(),
    );
    for _ in 0..5 {
        two.tick();
    }
    let challenged = silent
        .poll()
        .into_iter()
        .any(|i| matches!(i, Incoming::Message(_, _, b) if matches!(Msg::from_bytes(&b), Ok(Msg::Challenge { .. }))));
    assert!(challenged, "challenged first");
    assert_eq!(host_connections(&two), 1);
    std::thread::sleep(Duration::from_millis(40));
    two.tick();
    assert_eq!(host_connections(&two), 0, "still held");
}

fn host_connections(two: &Two) -> usize {
    match &two.host.collab.role {
        Role::Host(host) => host.peers.len(),
        Role::Joiner(_) => unreachable!(),
    }
}

/// What somebody may touch follows the key they proved, so being removed
/// and coming back does not hand back what the host took away.
#[test]
fn permissions_follow_the_proven_key_across_a_return() {
    let mut two = Two::new("perm-key", 2);
    let bob = joiner_space(&two);
    two.host
        .collab
        .set_permissions(
            bob,
            fontelle_model::wire::Permissions {
                mixer: false,
                ..Default::default()
            },
        )
        .unwrap();
    two.settle();
    two.host.collab.remove(bob, &mut two.host.doc).unwrap();
    two.settle();
    let mut again = two.another("Joiner", Some(joiner_identity(&two)));
    assert!(bring_in(&mut two, &mut again, None));
    assert!(!again.collab.my_permissions().mixer, "kept off the mixer");
    let mut carol = two.another("Carol", None);
    assert!(bring_in(&mut two, &mut carol, None));
    assert!(carol.collab.my_permissions().mixer, "somebody else is not");
}

/// The join's session id goes before the proof, in the clear: somebody
/// else naming it must not take that person's place.
#[test]
fn a_session_id_alone_cannot_take_somebodys_place() {
    let mut two = Two::new("hijack", 2);
    let bobs = match &two.joiner.collab.role {
        Role::Joiner(joiner) => joiner.session,
        Role::Host(_) => unreachable!(),
    };
    let mut mallory = two.another("Mallory", None);
    if let Role::Joiner(joiner) = &mut mallory.collab.role {
        joiner.session = bobs;
        joiner.say_hello(&mut mallory.collab.transport, &mallory.collab.options);
    }
    assert!(bring_in(&mut two, &mut mallory, None));
    two.settle();
    assert!(two.joiner.collab.is_live(), "Bob is still in");
    let names: Vec<String> = two
        .host
        .collab
        .peers()
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert_eq!(names.len(), 2, "{names:?}");
    assert_ne!(
        two.host.collab.peers()[0].peer,
        two.host.collab.peers()[1].peer,
        "two places, not one taken"
    );
}

/// The joiner's own link, cut and joined again the way `fontelle-net` does
/// it: a new connection, which says `Connected`.
struct Rejoins {
    hub: MemoryHub,
    inner: fontelle_net::MemoryTransport,
    back: Arc<std::sync::atomic::AtomicBool>,
}

impl Transport for Rejoins {
    fn send(&mut self, peer: PeerId, channel: Channel, bytes: &[u8]) {
        self.inner.send(peer, channel, bytes);
    }
    fn poll(&mut self) -> Vec<Incoming> {
        if self.back.swap(false, std::sync::atomic::Ordering::SeqCst) {
            self.inner = self.hub.connect();
            return vec![Incoming::Connected(fontelle_net::SERVER)];
        }
        self.inner.poll()
    }
    fn stats(&self, peer: PeerId) -> fontelle_net::LinkStats {
        self.inner.stats(peer)
    }
}

/// In a password session a joiner whose link dropped proves itself again
/// on the new one — with the password it gave, not asked twice — and is
/// back in its place, sealed.
#[test]
fn a_password_joiner_whose_link_drops_is_back_without_being_asked() {
    let back = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let theirs = back.clone();
    let mut two = Two::with(
        "password-rejoin",
        2,
        Setup {
            host_password: Some("pw"),
            joiner_password: Some("pw"),
            joiner_link: Some(Box::new(move |hub, link| {
                Box::new(Rejoins {
                    hub: hub.clone(),
                    inner: link,
                    back: theirs,
                })
            })),
            ..Setup::default()
        },
    );
    let bob = joiner_space(&two);
    back.store(true, std::sync::atomic::Ordering::SeqCst);
    let mut asked = false;
    for _ in 0..80 {
        two.tick();
        asked |= two.joiner.collab.password_wanted().is_some();
    }
    assert!(!asked, "asked for the password again");
    assert!(two.joiner.collab.is_live());
    assert_eq!(joiner_space(&two), bob, "in its own place");
    let clip = two.host.doc.clips.keys().next().unwrap();
    two.joiner.run(MoveClip::new(clip, PPQN, None));
    two.settle();
    assert_eq!(two.joiner.doc.sync_hash(), two.host.doc.sync_hash());
}

/// An install's key is kept in a file of its own, made the first time and
/// read back after, readable by its owner alone.
#[test]
fn an_identity_is_made_once_and_kept() {
    let dir = std::env::temp_dir().join(format!("fontelle-identity-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    let path = dir.join("identity.key");
    let made = Identity::load_or_create(&path).unwrap();
    let again = Identity::load_or_create(&path).unwrap();
    assert_eq!(made.public_key(), again.public_key());
    assert_eq!(made.public_key().len(), 32);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    assert_ne!(Identity::generate().public_key(), made.public_key());
    std::fs::remove_dir_all(&dir).ok();
}

/// Somebody who joins sees where everybody already is, not only those who
/// move after they arrive: the host's pointer held still said nothing to a
/// newcomer, seen with two studios on two displays.
#[test]
fn a_newcomer_is_told_where_everybody_already_is() {
    let hub = MemoryHub::new();
    let dir = std::env::temp_dir().join(format!("fontelle-collab-newcomer-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    let mut host = Side {
        collab: Collab::host(Box::new(hub.server_endpoint()), options("Host")),
        doc: crate::blank_project(8, 120.0, 48_000),
        history: History::new(),
        place: Place {
            bundle: None,
            banks: Vec::new(),
        },
    };
    host.history.open_outbox();
    host.collab.set_presence(at(960.0));
    host.pump();
    let mut joiner = Side {
        collab: Collab::join(Box::new(hub.connect()), options("Joiner"), dir.clone()),
        doc: Project::new("nothing yet"),
        history: History::new(),
        place: Place {
            bundle: None,
            banks: Vec::new(),
        },
    };
    let mut now = 0;
    let mut effects = Vec::new();
    let mut answered = false;
    for _ in 0..200 {
        now += 1;
        hub.set_now(now);
        host.pump();
        effects.extend(joiner.pump());
        if !answered && joiner.collab.question().is_some() {
            effects.extend(
                joiner
                    .collab
                    .answer(JoinAnswer::Copy, &mut joiner.history)
                    .unwrap(),
            );
            answered = true;
        }
        if let Some(path) = std::mem::take(&mut effects)
            .into_iter()
            .find_map(|e| match e {
                Effect::Open(path) => Some(path),
                _ => None,
            })
        {
            joiner.doc = fontelle_model::load_project(&path).unwrap();
            let Side {
                collab,
                doc,
                history,
                place,
            } = &mut joiner;
            collab.went_live(doc, history, place);
        }
    }
    assert!(joiner.collab.is_live());
    assert_eq!(whereabouts(&joiner), vec![(0, 960.0)]);
    std::fs::remove_dir_all(&dir).ok();
}

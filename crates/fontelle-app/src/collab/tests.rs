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
    inner: fontelle_net::MemoryTransport,
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

impl Two {
    fn new(name: &str, latency: u64) -> Two {
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
        let host = Side {
            collab: Collab::host(Box::new(hub.server_endpoint()), options("Host")),
            doc: song,
            history: host_history,
            place: Place {
                bundle: None,
                banks: Vec::new(),
            },
        };
        let resyncs = Arc::new(Mutex::new(0));
        let link = Counting {
            inner: hub.connect(),
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
        two.settle();
        let mut effects = two
            .joiner
            .collab
            .answer(JoinAnswer::Copy, &mut two.joiner.history)
            .unwrap();
        // The studio's part of a join: open the copy, hand it back.
        for _ in 0..200 {
            effects.extend(two.tick());
            if let Some(path) = std::mem::take(&mut effects)
                .into_iter()
                .find_map(|e| match e {
                    Effect::Open(path) => Some(path),
                    _ => None,
                })
            {
                two.joiner.doc = fontelle_model::load_project(&path).unwrap();
                let Side {
                    collab,
                    doc,
                    history,
                    place,
                } = &mut two.joiner;
                collab.went_live(doc, history, place);
                break;
            }
        }
        assert!(two.joiner.collab.is_live(), "the joiner has the song open");
        two.settle();
        two
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

/// Removing somebody keeps them out for the rest of the session: the same
/// studio joining again with the code is refused (L6).
#[test]
fn somebody_removed_cannot_come_back_with_the_code() {
    let mut two = Two::new("perm-removed", 2);
    let bob = joiner_space(&two);
    two.host.collab.remove(bob, &mut two.host.doc).unwrap();
    two.settle();
    let mut again = Side {
        collab: Collab::join(
            Box::new(two.hub.connect()),
            options("Joiner"),
            two.dir.clone(),
        ),
        doc: Project::new("nothing yet"),
        history: History::new(),
        place: Place {
            bundle: None,
            banks: Vec::new(),
        },
    };
    for _ in 0..60 {
        two.tick();
        again.pump();
    }
    let why = again.collab.ended().expect("refused");
    assert!(why.contains("removed"), "{why}");
    assert!(two.host.collab.peers().is_empty());
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

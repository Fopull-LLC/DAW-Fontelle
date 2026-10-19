//! Three studios on one song over a link with latency and jitter: the host
//! and two joiners, each doing what a person does — dragging with pauses,
//! deleting, adding effects, undoing — while the others do the same.
//!
//! Written from Ty's report of a session with a friend (2026-10-11):
//!
//! > *"its like glitchy and laggy feeling like my moving around things often
//! > over or undershoots where im trying to drag it and my actions are
//! > constantly taken back like im trying to delete this one clip but it
//! > keeps immediately reappearing. my actions are also doubling often like
//! > adding an effect made it add 2 versions of that effect."*
//!
//! Every edit here is one nobody else touches, so each has exactly one right
//! answer: the clip a person dragged ends where they dropped it, the clip
//! they deleted stays deleted, an effect added once is there once — on every
//! screen, at every moment after it was made, not only at the end.

mod common;

use std::path::PathBuf;
use std::time::Duration;

use fontelle_app::Session;
use fontelle_app::collab::{CollabOptions, JoinAnswer};
use fontelle_net::MemoryHub;
use fontelle_types::{ClipId, EffectKind, PPQN, PersistentId};
use fontelle_ui::canvas::ArrangeEdit;
use fontelle_ui::document::{DocumentHost, StudioHost};

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-collab-latency-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("creatable");
    path
}

struct Cleanup(PathBuf);

impl Drop for Cleanup {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

/// How long an edit left in the hand waits before it is let go of. The
/// studio's is 400 ms; here it is short, and a "pause" in a drag sleeps past
/// it, so a drag held still splits the way it does in the window.
const IDLE: Duration = Duration::from_millis(4);

fn options(name: &str, strict: bool) -> CollabOptions {
    let mut options = CollabOptions::new(name, PersistentId::derived(name));
    options.strict = strict;
    options.idle_break = IDLE;
    options.hello_again = Duration::ZERO;
    options
}

/// The host and the joiners, in one process.
struct Room {
    hub: MemoryHub,
    now: u64,
    latency: u64,
    jitter: u64,
    /// `studios[0]` is the host.
    studios: Vec<Session>,
    _cleanup: Cleanup,
}

impl Room {
    fn new(name: &str, joiners: usize, latency: u64, jitter: u64, strict: bool) -> Room {
        let dir = scratch(name);
        let hub = MemoryHub::new();
        hub.set_conditions(latency, 0.0);
        hub.set_jitter(jitter, 0x5eed ^ latency ^ (jitter << 8));
        let mut host = common::a_session_for(common::a_clip_project(8));
        std::fs::create_dir_all(dir.join("host")).unwrap();
        host.set_projects_dir(Some(dir.join("host")));
        host.save_as("Song").unwrap();
        host.share(Box::new(hub.server_endpoint()), options("Host", strict))
            .expect("shares");
        let mut room = Room {
            hub,
            now: 0,
            latency,
            jitter,
            studios: vec![host],
            _cleanup: Cleanup(dir.clone()),
        };
        for j in 0..joiners {
            let who = format!("Joiner{j}");
            let mut joiner = common::a_session_for(common::a_clip_project(8));
            std::fs::create_dir_all(dir.join(&who)).unwrap();
            joiner.set_projects_dir(Some(dir.join(&who)));
            joiner
                .join(Box::new(room.hub.connect()), options(&who, strict))
                .expect("joins");
            room.studios.push(joiner);
            room.settle();
            let last = room.studios.last_mut().unwrap();
            last.answer_join(JoinAnswer::Copy).expect("answers");
            room.settle();
            assert!(room.studios.last().unwrap().collab_live());
        }
        room.same();
        room
    }

    fn tick(&mut self, ticks: u64) {
        for _ in 0..ticks {
            self.now += 1;
            self.hub.set_now(self.now);
            for studio in &mut self.studios {
                studio.pump_collab();
            }
        }
    }

    /// Long enough for anything sent to have gone there and back, twice,
    /// with an idle break on the way.
    fn settle(&mut self) {
        for _ in 0..4 {
            std::thread::sleep(IDLE + Duration::from_millis(1));
            self.tick((self.latency + self.jitter) * 3 + 4);
        }
    }

    fn same(&self) {
        let hashes: Vec<u64> = self
            .studios
            .iter()
            .map(|s| s.project().sync_hash())
            .collect();
        assert!(
            hashes.windows(2).all(|w| w[0] == w[1]),
            "every studio holds the same song: {hashes:x?}"
        );
    }
}

fn start_of(studio: &Session, clip: ClipId) -> Option<i64> {
    studio.project().clips.get(clip).map(|c| c.start)
}

fn inserts(studio: &Session) -> Vec<PersistentId> {
    let master = studio
        .project()
        .mixer
        .tracks
        .keys()
        .next()
        .expect("a track");
    studio.project().mixer.tracks[master]
        .inserts
        .iter()
        .map(|i| i.id)
        .collect()
}

/// A tiny xorshift so a run is the same run every time.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// What one person meant, to be checked on every screen.
#[derive(Default)]
struct Meant {
    /// Their own clip, and where they dropped it.
    clip: Option<(ClipId, i64)>,
    /// Clips they made and then deleted.
    deleted: Vec<ClipId>,
    /// How many effects they added (and did not take back).
    effects: usize,
}

/// A person drags their clip `steps` bars along, a step a frame, pausing
/// mid-drag the way a hand does while it lines something up.
fn drag(room: &mut Room, who: usize, clip: ClipId, steps: i64, pause_at: Option<i64>) {
    let step = if steps < 0 { -PPQN } else { PPQN };
    for i in 0..steps.abs() {
        room.studios[who].arrange(ArrangeEdit::Move {
            ids: vec![clip],
            tick_delta: step,
            lane_delta: 0,
        });
        room.tick(1);
        if pause_at == Some(i) {
            std::thread::sleep(IDLE + Duration::from_millis(1));
            room.tick(2);
        }
    }
    room.studios[who].end_gesture();
}

fn run(room: &mut Room, seed: u64, rounds: usize) -> Vec<Meant> {
    let mut rng = Rng(seed);
    let n = room.studios.len();
    // Each person gets a clip of their own, far from everyone else's.
    let mut meant: Vec<Meant> = (0..n).map(|_| Meant::default()).collect();
    for (who, m) in meant.iter_mut().enumerate() {
        let start = PPQN * 4 * (8 + 16 * who as i64);
        let made = room.studios[who].arrange(ArrangeEdit::Add { lane: 0, start });
        let clip = made.clips[0];
        m.clip = Some((clip, start));
        room.settle();
    }
    room.same();

    for _ in 0..rounds {
        let who = rng.below(n as u64) as usize;
        match rng.below(4) {
            0 | 1 => {
                let (clip, at) = meant[who].clip.unwrap();
                let mut steps = rng.below(6) as i64 + 1;
                if rng.below(2) == 0
                    && at - steps * PPQN >= PPQN * 4 * (8 + 16 * who as i64) - 8 * PPQN
                {
                    steps = -steps;
                }
                let pause = (rng.below(2) == 0).then(|| rng.below(steps.unsigned_abs()) as i64);
                drag(room, who, clip, steps, pause);
                meant[who].clip = Some((clip, at + steps * PPQN));
            }
            2 => {
                let start = PPQN * 4 * (12 + 16 * who as i64);
                let made = room.studios[who].arrange(ArrangeEdit::Add { lane: 0, start });
                let clip = made.clips[0];
                room.tick(rng.below(3) + 1);
                room.studios[who].arrange(ArrangeEdit::Remove(vec![clip]));
                room.studios[who].end_gesture();
                meant[who].deleted.push(clip);
            }
            _ => {
                room.studios[who].add_insert(0, EffectKind::Eq);
                meant[who].effects += 1;
            }
        }
        room.tick(rng.below(room.latency + room.jitter + 1));
    }
    room.settle();
    meant
}

/// What every screen must show of what everybody meant.
fn check(room: &Room, meant: &[Meant]) -> Vec<String> {
    let mut wrong = Vec::new();
    let effects: usize = meant.iter().map(|m| m.effects).sum();
    for (s, studio) in room.studios.iter().enumerate() {
        for (who, m) in meant.iter().enumerate() {
            let (clip, at) = m.clip.unwrap();
            let start = start_of(studio, clip);
            if start != Some(at) {
                wrong.push(format!(
                    "studio {s}: studio {who}'s clip at {start:?}, dropped at {at} (off by {:?} beats)",
                    start.map(|x| (x - at) / PPQN)
                ));
            }
            for gone in &m.deleted {
                if start_of(studio, *gone).is_some() {
                    wrong.push(format!("studio {s}: studio {who}'s deleted clip is back"));
                }
            }
        }
        let ids = inserts(studio);
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        if ids.len() != effects || unique.len() != ids.len() {
            wrong.push(format!(
                "studio {s}: {} effects ({} distinct), {effects} added",
                ids.len(),
                unique.len()
            ));
        }
    }
    wrong
}

#[test]
fn three_studios_explore() {
    let mut failures = Vec::new();
    for seed in 1..=12u64 {
        for (latency, jitter) in [(3, 0), (6, 4), (12, 8)] {
            let mut room = Room::new(
                &format!("explore-{seed}-{latency}"),
                2,
                latency,
                jitter,
                false,
            );
            let meant = run(&mut room, seed * 7919 + latency, 30);
            let wrong = check(&room, &meant);
            let hashes: Vec<u64> = room
                .studios
                .iter()
                .map(|s| s.project().sync_hash())
                .collect();
            let same = hashes.windows(2).all(|w| w[0] == w[1]);
            if !wrong.is_empty() || !same {
                failures.push(format!(
                    "seed {seed} latency {latency} jitter {jitter}: same={same} {wrong:#?}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ------------------------------------------------- edits made the same way

use fontelle_types::NoteId;
use fontelle_ui::canvas::RollEdit;

fn a_note(start: i64, key: u8) -> fontelle_model::Note {
    fontelle_model::Note {
        start,
        length: PPQN,
        key,
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
    }
}

fn notes_in(studio: &Session, clip: ClipId) -> Vec<(NoteId, i64, u8)> {
    match studio.project().clip_source(clip).as_deref() {
        Some(fontelle_model::ClipSource::Notes(data)) => data
            .notes
            .iter()
            .map(|(id, n)| (id, n.start, n.key))
            .collect(),
        _ => Vec::new(),
    }
}

fn the_clip(studio: &Session) -> ClipId {
    Session::first_clip(studio.project()).expect("a clip")
}

/// Every kind of edit a joiner makes lands as the same song on every screen
/// — the same ids included, or the next edit anybody makes to what it made
/// is refused somewhere ("taken back") or lands on nothing.
/// A song with four notes in its clip, drawn by a joiner, shared by three.
fn a_room_with_notes(name: &str) -> Room {
    let mut room = Room::new(name, 2, 5, 3, true);
    for (i, key) in [60u8, 62, 64, 65].into_iter().enumerate() {
        room.studios[1].edit(RollEdit::Add {
            note: a_note(i as i64 * PPQN * 2, key),
        });
        room.studios[1].end_gesture();
    }
    room.settle();
    room.same();
    room
}

type Op = fn(&mut Session, ClipId);

fn joiner_ops() -> Vec<(&'static str, Op)> {
    vec![
        ("cut a clip in two", |s, clip| {
            s.arrange(ArrangeEdit::Split {
                cuts: vec![(clip, PPQN * 3)],
            });
        }),
        ("slice notes", |s, clip| {
            let ids: Vec<NoteId> = notes_in(s, clip).iter().map(|n| n.0).collect();
            s.edit(RollEdit::Slice {
                cuts: vec![(ids[0], PPQN / 2)],
            });
        }),
        ("duplicate a clip", |s, clip| {
            s.arrange(ArrangeEdit::Duplicate {
                ids: vec![clip],
                tick_offset: PPQN * 64,
            });
        }),
        ("copy and paste a clip", |s, clip| {
            s.arrange(ArrangeEdit::Copy(vec![clip]));
            s.arrange(ArrangeEdit::Paste { at: PPQN * 96 });
        }),
        ("add a lane", |s, _| s.add_lane()),
        ("add a mixer track", |s, _| s.add_mixer_track()),
        ("duplicate a mixer track", |s, _| {
            s.add_mixer_track();
            s.add_insert(1, EffectKind::Eq);
            s.duplicate_mixer_track(1);
        }),
        ("add a send", |s, _| {
            s.add_mixer_track();
            s.add_mixer_track();
            s.add_send(1, 2);
        }),
        ("duplicate a channel", |s, _| s.duplicate_channel(0)),
        ("make a prefab of a clip", |s, clip| {
            let _ = s.make_prefab_from(clip);
        }),
    ]
}

/// Every kind of edit a joiner makes lands as the same song on every screen
/// — the same ids included, or the next edit anybody makes to what it made
/// is refused somewhere ("taken back") or lands on nothing.
#[test]
fn whatever_a_joiner_does_lands_as_the_same_song_everywhere() {
    let mut wrong = Vec::new();
    for (n, (what, op)) in joiner_ops().into_iter().enumerate() {
        let mut room = a_room_with_notes(&format!("same-{n}"));
        let clip = the_clip(&room.studios[1]);
        op(&mut room.studios[1], clip);
        room.studios[1].end_gesture();
        room.settle();
        let hashes: Vec<u64> = room
            .studios
            .iter()
            .map(|s| s.project().sync_hash())
            .collect();
        if !hashes.windows(2).all(|w| w[0] == w[1]) {
            wrong.push(what);
        }
        for (s, studio) in room.studios.iter_mut().enumerate() {
            let said = studio.take_collab_notices().join(" ");
            assert!(
                !said.contains("taken back"),
                "{what}: studio {s} said {said}"
            );
        }
    }
    assert!(
        wrong.is_empty(),
        "these land differently on the joiner: {wrong:?}"
    );
}

/// Undo, redo, effects taken off and moved, clips cut — by everybody at
/// once. Everybody must end on one song with no effect twice.
#[test]
fn three_studios_with_undo_explore() {
    let mut failures = Vec::new();
    for seed in 1..=10u64 {
        for (latency, jitter) in [(4, 3), (10, 8)] {
            let mut room = Room::new(&format!("undo-{seed}-{latency}"), 2, latency, jitter, false);
            let mut rng = Rng(seed * 104_729 + latency);
            let n = room.studios.len();
            for _ in 0..40 {
                let who = rng.below(n as u64) as usize;
                let s = &mut room.studios[who];
                match rng.below(9) {
                    0 => s.add_insert(0, EffectKind::Eq),
                    1 => s.add_insert(0, EffectKind::Delay),
                    2 => {
                        let count = inserts(s).len();
                        if count > 0 {
                            s.remove_insert(0, rng.below(count as u64) as usize);
                        }
                    }
                    3 => {
                        let count = inserts(s).len();
                        if count > 1 {
                            s.move_insert(0, 0, count - 1);
                        }
                    }
                    4 => s.undo(),
                    5 => s.redo(),
                    6 => {
                        let made = s.arrange(ArrangeEdit::Add {
                            lane: 0,
                            start: PPQN * 4 * (20 + rng.below(20) as i64),
                        });
                        let _ = made;
                    }
                    7 => {
                        let clips: Vec<ClipId> = s.project().clips.keys().collect();
                        if let Some(c) = clips.get(rng.below(clips.len().max(1) as u64) as usize) {
                            s.arrange(ArrangeEdit::Remove(vec![*c]));
                        }
                    }
                    _ => {
                        let clips: Vec<ClipId> = s.project().clips.keys().collect();
                        if let Some(c) = clips.get(rng.below(clips.len().max(1) as u64) as usize) {
                            for _ in 0..3 {
                                room.studios[who].arrange(ArrangeEdit::Move {
                                    ids: vec![*c],
                                    tick_delta: PPQN,
                                    lane_delta: 0,
                                });
                                room.tick(1);
                            }
                        }
                    }
                }
                room.studios[who].end_gesture();
                room.tick(rng.below(latency + jitter + 1));
            }
            room.settle();
            let hashes: Vec<u64> = room
                .studios
                .iter()
                .map(|s| s.project().sync_hash())
                .collect();
            let same = hashes.windows(2).all(|w| w[0] == w[1]);
            let twice: Vec<usize> = room
                .studios
                .iter()
                .map(|s| {
                    let ids = inserts(s);
                    let mut unique = ids.clone();
                    unique.sort();
                    unique.dedup();
                    ids.len() - unique.len()
                })
                .collect();
            if !same || twice.iter().any(|t| *t > 0) {
                failures.push(format!(
                    "seed {seed} latency {latency}: same={same} twice={twice:?}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn a_tone(dir: &std::path::Path, name: &str, seconds: f32, hz: f32) -> PathBuf {
    let frames = (48_000.0 * seconds) as usize;
    let samples: Vec<f32> = (0..frames)
        .map(|i| (i as f32 * hz * std::f32::consts::TAU / 48_000.0).sin() * 0.5)
        .collect();
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    std::fs::write(
        &path,
        fontelle_assets::fixtures::build_wav(48_000, 1, &samples),
    )
    .unwrap();
    path
}

/// A song with a take in it: the edits a take gets — its length, its front,
/// its fades, a cut, the tempo under it — land as one song everywhere.
#[test]
fn a_take_edited_by_everybody_stays_one_song() {
    let mut room = Room::new("take", 2, 5, 3, true);
    let dir = room._cleanup.0.clone();
    room.studios[0]
        .drop_file_at(&a_tone(&dir.join("desk"), "tone.wav", 2.0, 440.0), 0)
        .unwrap();
    room.studios[0].end_gesture();
    room.settle();
    room.same();
    let take = room.studios[0]
        .project()
        .clips
        .iter()
        .find(|(_, c)| matches!(c.source, fontelle_model::ClipSource::Audio(_)))
        .map(|(id, _)| id)
        .expect("the take");
    type Step = fn(&mut Session, ClipId);
    let steps: Vec<(&str, Step)> = vec![
        ("tempo", |s, _| s.set_tempo(97.0)),
        ("resize", |s, c| {
            s.arrange(ArrangeEdit::Resize {
                ids: vec![c],
                tick_delta: -PPQN,
            });
        }),
        ("trim front", |s, c| {
            s.arrange(ArrangeEdit::TrimStart {
                ids: vec![c],
                tick_delta: PPQN / 2,
            });
        }),
        ("fade", |s, c| {
            s.arrange(ArrangeEdit::SetFade {
                clip: c,
                end: fontelle_ui::canvas::FadeEnd::In,
                fraction: 0.25,
            });
        }),
        ("move", |s, c| {
            s.arrange(ArrangeEdit::Move {
                ids: vec![c],
                tick_delta: PPQN * 4,
                lane_delta: 0,
            });
        }),
        ("tempo again", |s, _| s.set_tempo(133.0)),
        ("cut", |s, c| {
            let start = s.project().clips[c].start;
            s.arrange(ArrangeEdit::Split {
                cuts: vec![(c, start + PPQN)],
            });
        }),
    ];
    for (i, (what, step)) in steps.into_iter().enumerate() {
        let who = 1 + i % 2;
        step(&mut room.studios[who], take);
        room.studios[who].end_gesture();
        room.settle();
        let hashes: Vec<u64> = room
            .studios
            .iter()
            .map(|s| s.project().sync_hash())
            .collect();
        assert!(
            hashes.windows(2).all(|w| w[0] == w[1]),
            "after {what}: {hashes:x?}"
        );
    }
}

/// The cut, as Ty's friend would make it: a joiner cuts a clip with notes
/// in it, and then both of them go on editing those notes. Before: the cut's
/// notes had other ids on the host than on the joiner (the host made them
/// again where it mints, the joiner where it does), so the joiner's next
/// move of one of them was refused by the host and taken back off his
/// screen, and the host's next edit of one landed on nothing on his.
#[test]
fn notes_in_a_clip_a_joiner_cut_are_the_same_notes_everywhere() {
    let mut room = a_room_with_notes("cut-notes");
    let clip = the_clip(&room.studios[1]);
    room.studios[1].arrange(ArrangeEdit::Split {
        cuts: vec![(clip, PPQN * 3)],
    });
    // While the cut is still on its way, somebody else's edit arrives under
    // it and the joiner's screen is made again.
    room.studios[0].add_insert(0, EffectKind::Eq);
    room.tick(1);
    let tail = room.studios[1]
        .project()
        .clips
        .iter()
        .find(|(id, c)| *id != clip && c.start == PPQN * 3)
        .map(|(id, _)| id)
        .expect("the cut made a second clip");
    let tail_notes = notes_in(&room.studios[1], tail);
    assert!(!tail_notes.is_empty());
    room.tick(2);
    room.studios[1].open_clip(tail);
    let note = notes_in(&room.studios[1], tail)[0].0;
    room.studios[1].edit(RollEdit::Move {
        ids: vec![note],
        tick_delta: PPQN,
        key_delta: 0,
    });
    room.studios[1].end_gesture();
    room.settle();
    room.same();
    room.studios[0].open_clip(tail);
    let note = notes_in(&room.studios[0], tail)[0].0;
    room.studios[0].edit(RollEdit::Move {
        ids: vec![note],
        tick_delta: 0,
        key_delta: 1,
    });
    room.studios[0].end_gesture();
    room.settle();
    room.same();
    for (s, studio) in room.studios.iter_mut().enumerate() {
        let said = studio.take_collab_notices().join(" ");
        assert!(!said.contains("taken back"), "studio {s} said {said}");
    }
}

fn bypassed(studio: &Session) -> Vec<(PersistentId, bool)> {
    let master = studio
        .project()
        .mixer
        .tracks
        .keys()
        .next()
        .expect("a track");
    studio.project().mixer.tracks[master]
        .inserts
        .iter()
        .map(|i| (i.id, i.bypassed))
        .collect()
}

/// Ty's friend switches off his EQ while Ty drags the delay above it. The
/// switch named the EQ by its place in the chain, and by the time it reached
/// the host the delay was in that place: **before**, refused and "taken
/// back"; **after**, it follows the EQ to where it went — the same insert,
/// by its own id, never its neighbour.
#[test]
fn an_effect_switched_off_while_somebody_reorders_the_chain_is_still_switched_off() {
    let mut room = Room::new("follow-insert", 1, 6, 0, true);
    room.studios[0].add_insert(0, EffectKind::Eq);
    room.studios[0].add_insert(0, EffectKind::Delay);
    room.settle();
    let eq = bypassed(&room.studios[0])[0].0;
    room.studios[0].move_insert(0, 1, 0);
    room.studios[1].toggle_insert_bypass(0, 0);
    room.settle();
    room.same();
    let chain = bypassed(&room.studios[0]);
    assert_eq!(
        chain.iter().find(|(id, _)| *id == eq).map(|(_, b)| *b),
        Some(true),
        "the EQ is off: {chain:?}"
    );
    assert_eq!(
        chain.iter().filter(|(_, b)| *b).count(),
        1,
        "and only the EQ"
    );
    let said = room.studios[1].take_collab_notices().join(" ");
    assert!(!said.contains("taken back"), "{said}");
}

/// *"glitchy and laggy"*: twenty edits of the host's landing on a joiner in
/// one turn were twenty rebuilds of the joiner's whole graph, back to back
/// (every soundfont reloaded each time). **Before**: 20. **Then**: one.
/// **Now** (2026-10-18, `Edit::reach`): none — notes reach the timeline, and
/// a rebuild would cut whatever was sounding.
#[test]
fn a_burst_of_edits_is_one_rebuild_of_the_graph() {
    let mut room = Room::new("burst", 1, 8, 0, true);
    let clip = the_clip(&room.studios[0]);
    for i in 0..20 {
        room.studios[0].edit(RollEdit::Add {
            note: a_note(i * PPQN / 4, 60),
        });
        room.studios[0].end_gesture();
    }
    // The host sends them in one turn; they cross together.
    room.studios[0].pump_collab();
    let before = room.studios[1].graph_builds();
    room.tick(room.latency + 2);
    assert_eq!(notes_in(&room.studios[1], clip).len(), 20);
    assert_eq!(room.studios[1].graph_builds() - before, 0);
}

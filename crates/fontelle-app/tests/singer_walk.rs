//! The Fontelle Singer walked end to end with **real** voicebanks, the way a
//! producer would use it: a vocal track, a singer, a melody with words, the
//! preview then the final take, an edit and how soon it is heard again, a
//! render to a clip, and how loud each bank comes out. What it measures is
//! printed (`--nocapture`); what must hold is asserted.
//!
//! Ignored by default: third-party voicebanks are never in the repository.
//!
//! `FONTELLE_TEST_VOICEBANKS=/path/to/voicebanks cargo test -p fontelle-app
//! --test singer_walk -- --ignored --nocapture --test-threads 1`
//!
//! The banks are linked into a scratch folder, so nothing is written beside
//! them (agreed terms, downloads), and the song is saved in scratch too.

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fontelle_app::Session;
use fontelle_model::VocalVoice;
use fontelle_types::{InstrumentKind, NoteId, PPQN};
use fontelle_ui::canvas::RollEdit;
use fontelle_ui::document::{ClipKind, DocumentHost, StudioHost, VocalRenderState};

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "fontelle-singer-walk-{name}-{}",
        std::process::id()
    ));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).unwrap();
    path
}

/// The real banks, each linked into a voicebanks folder of the test's own.
fn linked_banks(into: &Path) -> Option<PathBuf> {
    let source = PathBuf::from(std::env::var_os("FONTELLE_TEST_VOICEBANKS")?);
    let banks = into.join("voicebanks");
    std::fs::create_dir_all(&banks).unwrap();
    for entry in std::fs::read_dir(&source).ok()?.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with(['_', '.']) || !entry.path().is_dir() {
            continue;
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(entry.path(), banks.join(&name)).unwrap();
    }
    Some(banks)
}

fn walk_session(name: &str, bars: i64) -> Option<(Session, PathBuf)> {
    let dir = scratch(name);
    let banks = linked_banks(&dir)?;
    let bundle = dir.join("Song.fontelle");
    let project = common::a_clip_project(bars);
    fontelle_app::save_project(&project, &bundle).unwrap();
    let mut session = common::a_session_in(project, Some(bundle.clone()));
    session.set_singer_folders(vec![dir.join("no-engines")]);
    session.set_voicebanks(banks, fontelle_vocal::builtin_backends());
    Some((session, bundle))
}

fn vocal_index(session: &Session) -> usize {
    (0..session.channels().len())
        .rev()
        .find(|i| session.channel_kind(*i) == Some(InstrumentKind::Vocal))
        .expect("a vocal channel")
}

fn draw(session: &mut Session, beat: f64, beats: f64, key: u8) -> NoteId {
    let ticks = |b: f64| (b * PPQN as f64).round() as i64;
    session.edit(RollEdit::Add {
        note: fontelle_model::Note {
            start: ticks(beat),
            length: ticks(beats),
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
        },
    })[0]
}

fn states(session: &Session) -> Vec<VocalRenderState> {
    session
        .vocal_view()
        .map(|v| v.phrases.iter().map(|p| p.state).collect())
        .unwrap_or_default()
}

/// Pumps as the window does until `done`, answering how long it took.
fn pump_until(session: &mut Session, what: &str, done: impl Fn(&Session) -> bool) -> Duration {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(300);
    while !done(session) {
        assert!(
            Instant::now() < deadline,
            "{what}: never settled ({:?}, {:?})",
            states(session),
            session.vocal_view().and_then(|v| v.message)
        );
        session.pump();
        std::thread::sleep(Duration::from_millis(2));
    }
    started.elapsed()
}

fn heard(state: VocalRenderState) -> bool {
    matches!(state, VocalRenderState::Preview | VocalRenderState::Final)
}

/// The frames of a mono-summed 16-bit stereo WAV, and its rate.
fn read_wav(path: &Path) -> (Vec<f32>, u32) {
    let bytes = std::fs::read(path).unwrap();
    let rate = u32::from_le_bytes(bytes[24..28].try_into().unwrap());
    let data = bytes
        .windows(4)
        .position(|w| w == b"data")
        .expect("a data chunk")
        + 8;
    let samples: Vec<f32> = bytes[data..]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|f| {
            let l = i16::from_le_bytes([f[0], f[1]]) as f32 / 32768.0;
            let r = i16::from_le_bytes([f[2], f[3]]) as f32 / 32768.0;
            (l + r) / 2.0
        })
        .collect();
    (samples, rate)
}

/// The newest render in the song's `renders/` folder.
fn newest_render(bundle: &Path) -> PathBuf {
    std::fs::read_dir(bundle.join("renders"))
        .unwrap()
        .flatten()
        .max_by_key(|e| e.metadata().unwrap().modified().unwrap())
        .unwrap()
        .path()
}

/// Loudness in dBFS: the peak, and the RMS of the loudest 400 ms.
fn loudness(samples: &[f32], sr: u32) -> (f32, f32) {
    let peak = samples.iter().fold(0f32, |m, s| m.max(s.abs()));
    let window = (sr as usize * 2) / 5;
    let mut best = 0f32;
    for chunk in samples.chunks(window / 2) {
        let rms = (chunk.iter().map(|s| s * s).sum::<f32>() / chunk.len().max(1) as f32).sqrt();
        best = best.max(rms);
    }
    let db = |x: f32| 20.0 * x.max(1e-9).log10();
    (db(peak), db(best))
}

/// The largest jump between neighbouring samples, against the signal's own
/// largest slope over the middle of notes: a click stands out of it.
fn largest_step(samples: &[f32]) -> f32 {
    samples
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0, f32::max)
}

struct Walk {
    folder: &'static str,
    language: &'static str,
    line: &'static str,
}

const WALKS: &[Walk] = &[
    Walk {
        folder: "KasaneTeto_tandoku_100619",
        language: "ja",
        line: "さ く ら さ く ら や よ い",
    },
    Walk {
        folder: "KasaneTeto_tougou_110401",
        language: "ja",
        line: "さ く ら さ く ら や よ い",
    },
    Walk {
        folder: "KasaneTeto_OU_JA_240323",
        language: "ja",
        line: "さ く ら さ く ら や よ い",
    },
    Walk {
        folder: "KasaneTeto_English_150401",
        language: "en",
        line: "twin-kle twin-kle lit-tle star how I",
    },
    Walk {
        folder: "Canary_ARPAsing_1.0",
        language: "en",
        line: "twin-kle twin-kle lit-tle star how I",
    },
    Walk {
        folder: "LIEE_English_ARPAsing",
        language: "en",
        line: "twin-kle twin-kle lit-tle star how I",
    },
    Walk {
        folder: "Hitsuboku_Kumi_CHN",
        language: "zh",
        line: "一闪一闪亮晶晶满",
    },
    Walk {
        folder: "Canary_DiffSinger_v106",
        language: "en",
        line: "twin-kle twin-kle lit-tle star how I",
    },
    Walk {
        folder: "TIGER_DiffSinger_v106",
        language: "en",
        line: "twin-kle twin-kle lit-tle star how I",
    },
    Walk {
        folder: "LIEE_DiffSinger_MM2.8",
        language: "en",
        line: "twin-kle twin-kle lit-tle star how I",
    },
    Walk {
        folder: "Qixuan_v2.7.0_DiffSinger_OpenUtau",
        language: "zh",
        line: "一闪一闪亮晶晶满",
    },
    Walk {
        folder: "Nishiren_DiffSinger_v2.0",
        language: "ja",
        line: "さ く ら さ く ら や よ い",
    },
    Walk {
        folder: "Yoko_DiffSinger_v1.0.0",
        language: "ja",
        line: "さ く ら さ く ら や よ い",
    },
    Walk {
        folder: "Kiritan_DiffSinger",
        language: "ja",
        line: "さ く ら さ く ら や よ い",
    },
];

/// Each bank: written, heard, edited, heard again, rendered, measured.
#[test]
#[ignore = "needs FONTELLE_TEST_VOICEBANKS"]
fn every_bank_walked_from_a_new_track_to_a_rendered_clip() {
    let only = std::env::var("FONTELLE_WALK_ONLY").ok();
    let mut table = Vec::new();
    for walk in WALKS {
        if only
            .as_deref()
            .is_some_and(|o| !o.split(',').any(|o| walk.folder.contains(o)))
        {
            continue;
        }
        let Some((mut session, bundle)) = walk_session(walk.folder, 8) else {
            eprintln!("FONTELLE_TEST_VOICEBANKS not set");
            return;
        };
        session.add_singer_channel(None).unwrap();
        let index = vocal_index(&session);
        session.select_channel(index);
        let singer = session
            .singers()
            .into_iter()
            .find(|s| s.folder.as_deref() == Some(walk.folder))
            .unwrap_or_else(|| panic!("{} is a singer", walk.folder));
        session.set_singer(
            index,
            Some(VocalVoice::new(&singer.engine, &singer.id, walk.language)),
        );
        // The producer reads the panel a moment before writing.
        let think: f64 = std::env::var("FONTELLE_WALK_THINK")
            .ok()
            .and_then(|t| t.parse().ok())
            .unwrap_or(0.0);
        let thinking = Instant::now();
        while thinking.elapsed().as_secs_f64() < think {
            session.pump();
            std::thread::sleep(Duration::from_millis(5));
        }
        // "Twinkle twinkle": two phrases of four, a beat's rest between.
        let melody = [
            (0.0, 60),
            (1.0, 60),
            (2.0, 67),
            (3.0, 67),
            (5.0, 69),
            (6.0, 69),
            (7.0, 67),
            (8.0, 65),
        ];
        let ids: Vec<NoteId> = melody
            .iter()
            .map(|(beat, key)| draw(&mut session, *beat, 1.0, *key))
            .collect();
        session.set_lyric_line(&ids, walk.line).unwrap();
        let lyrics: Vec<String> = ids
            .iter()
            .map(|id| session.notes()[*id].lyric.clone())
            .collect();

        let first = pump_until(&mut session, "first preview", |s| {
            let st = states(s);
            !st.is_empty() && st.iter().all(|x| heard(*x))
        });
        let fin = pump_until(&mut session, "first final", |s| {
            states(s).iter().all(|x| *x == VocalRenderState::Final)
        }) + first;
        let view = session.vocal_view().unwrap();
        assert!(
            view.message.is_none(),
            "{}: {:?}",
            walk.folder,
            view.message
        );
        let phonemes: Vec<String> = ids
            .iter()
            .map(|id| {
                view.phonemes
                    .iter()
                    .find(|(n, _)| n == id)
                    .map_or("?".into(), |(_, p)| p.clone())
            })
            .collect();

        // An edit to the second phrase: how soon is it heard?
        session.edit(RollEdit::Move {
            ids: vec![ids[6]],
            tick_delta: 0,
            key_delta: 2,
        });
        session.end_gesture();
        let edited_preview = pump_until(&mut session, "edit preview", |s| {
            states(s).iter().all(|x| heard(*x))
        });
        let edited_final = pump_until(&mut session, "edit final", |s| {
            states(s).iter().all(|x| *x == VocalRenderState::Final)
        }) + edited_preview;
        // Undo is a cache hit: heard at once.
        session.undo();
        let undone = pump_until(&mut session, "undo", |s| {
            states(s).iter().all(|x| *x == VocalRenderState::Final)
        });

        // Rendered to a clip: the whole part (nothing selected).
        let said = session.render_vocal_selection_now().unwrap();
        assert!(
            session.clips().iter().any(|c| c.kind == ClipKind::Audio),
            "{said}: no clip"
        );
        let path = newest_render(&bundle);
        let (samples, sr) = read_wav(&path);
        let (peak, rms) = loudness(&samples, sr);
        let step = largest_step(&samples);
        assert!(samples.iter().all(|s| s.is_finite()));
        table.push(format!(
            "{:36} {:2} first heard {:>6.0} ms, final {:>6.0} ms | edit heard {:>5.0} ms, final {:>6.0} ms | undo {:>4.0} ms | peak {:>6.1} dB, loud {:>6.1} dB, step {step:.3}\n    {lyrics:?}\n    {phonemes:?}",
            walk.folder,
            walk.language,
            first.as_secs_f64() * 1000.0,
            fin.as_secs_f64() * 1000.0,
            edited_preview.as_secs_f64() * 1000.0,
            edited_final.as_secs_f64() * 1000.0,
            undone.as_secs_f64() * 1000.0,
            peak,
            rms,
        ));
        eprintln!("{}", table.last().unwrap());
    }
    println!("\n{}", table.join("\n"));
}

/// A long song on one singer (`FONTELLE_WALK_LONG`, a folder; Canary DS by
/// default): how soon an edit is heard while the song's finals are still
/// being sung, a storm of edits and undos settling, and the song reopened
/// playing from the takes kept on disk.
#[test]
#[ignore = "needs FONTELLE_TEST_VOICEBANKS"]
fn a_long_song_stays_responsive_and_reopens_sung() {
    let folder =
        std::env::var("FONTELLE_WALK_LONG").unwrap_or_else(|_| "Canary_DiffSinger_v106".into());
    let Some((mut session, bundle)) = walk_session("long", 64) else {
        eprintln!("FONTELLE_TEST_VOICEBANKS not set");
        return;
    };
    session.add_singer_channel(None).unwrap();
    let index = vocal_index(&session);
    session.select_channel(index);
    let singer = session
        .singers()
        .into_iter()
        .find(|s| s.folder.as_deref() == Some(folder.as_str()))
        .unwrap();
    let language = singer.languages[0].clone();
    session.set_singer(
        index,
        Some(VocalVoice::new(&singer.engine, &singer.id, &language)),
    );
    // 40 phrases of four notes, a beat's rest after each: 200 beats.
    let mut ids = Vec::new();
    for phrase in 0..40 {
        for n in 0..4 {
            let beat = f64::from(phrase * 5 + n);
            ids.push(draw(
                &mut session,
                beat,
                1.0,
                60 + (n as u8 * 2) + (phrase % 3) as u8,
            ));
        }
    }
    let started = Instant::now();
    let previews = pump_until(&mut session, "all previews", |s| {
        let st = states(s);
        st.len() == 40 && st.iter().all(|x| heard(*x))
    });
    // Finals are being sung now. An edit to phrase 20.
    let finals_left = states(&session)
        .iter()
        .filter(|x| **x != VocalRenderState::Final)
        .count();
    session.edit(RollEdit::Move {
        ids: vec![ids[20 * 4 + 1]],
        tick_delta: 0,
        key_delta: 3,
    });
    session.end_gesture();
    let edit_heard = pump_until(&mut session, "edit heard while finals render", |s| {
        heard(states(s)[20])
    });
    // A storm: twenty edits and undos in a row, then everything settles.
    for k in 0..20 {
        session.edit(RollEdit::Move {
            ids: vec![ids[(k * 7) % ids.len()]],
            tick_delta: 0,
            key_delta: 1,
        });
        session.end_gesture();
        session.pump();
        if k % 2 == 1 {
            session.undo();
            session.undo();
        }
    }
    for _ in 0..10 {
        session.redo();
    }
    let settled = pump_until(&mut session, "storm settles", |s| {
        states(s).iter().all(|x| *x == VocalRenderState::Final)
    });
    let total = started.elapsed();
    DocumentHost::save(&mut session).unwrap();

    // Reopened: a new studio on the saved song plays from the disk.
    let project = fontelle_app::open_project(&bundle).unwrap().project;
    let mut reopened = common::a_session_in(project, Some(bundle.clone()));
    reopened.set_singer_folders(vec![bundle.with_file_name("no-engines")]);
    reopened.set_voicebanks(
        bundle.with_file_name("voicebanks"),
        fontelle_vocal::builtin_backends(),
    );
    reopened.select_channel(vocal_index(&reopened));
    let reopen = pump_until(&mut reopened, "reopened", |s| {
        let st = states(s);
        !st.is_empty() && st.iter().all(|x| *x == VocalRenderState::Final)
    });
    println!(
        "{folder}: 40 phrases previewed in {:.1} s ({finals_left} finals left); \
         an edit heard in {:.0} ms while finals render; a storm of 20 edits settled \
         in {:.1} s; all final after {:.1} s; reopened all final in {:.0} ms",
        previews.as_secs_f64(),
        edit_heard.as_secs_f64() * 1000.0,
        settled.as_secs_f64(),
        total.as_secs_f64(),
        reopen.as_secs_f64() * 1000.0,
    );
}

/// The demo song that ships with Joule's bank, as a producer would bring
/// it from OpenUtau: dropped in, sung by the bank it names, written back
/// out as MIDI and USTX, frozen and unfrozen, saved and reopened.
#[test]
#[ignore = "needs FONTELLE_TEST_VOICEBANKS"]
fn an_openutau_song_comes_in_sung_and_goes_back_out() {
    let Some(source) = std::env::var_os("FONTELLE_TEST_VOICEBANKS").map(PathBuf::from) else {
        return;
    };
    let demo = source.join("Joule_EN_CVVC_beta/02 extra/demo_daydream.ustx");
    if !demo.is_file() {
        eprintln!("no demo song");
        return;
    }
    let (mut session, bundle) = walk_session("ustx", 8).unwrap();
    let said = session.drop_file(&demo).unwrap();
    println!("{said}");
    // The import opened its first track's clip and selected that track.
    let index = (0..session.channels().len())
        .find(|i| session.channel_kind(*i) == Some(InstrumentKind::Vocal))
        .unwrap();
    let voice = session.vocal_voice(index).unwrap();
    assert!(voice.singer.contains("Joule_EN_CVVC"), "{voice:?}");
    let heard = pump_until(&mut session, "the demo sung", |s| {
        let st = states(s);
        !st.is_empty() && st.iter().all(|x| *x == VocalRenderState::Final)
    });
    let view = session.vocal_view().unwrap();
    assert!(view.message.is_none(), "{:?}", view.message);
    println!(
        "{} phrases, {} notes, all final in {:.1} s; phonemes {:?}",
        view.phrases.len(),
        session.notes().len(),
        heard.as_secs_f64(),
        view.phonemes
            .iter()
            .take(8)
            .map(|(_, p)| p)
            .collect::<Vec<_>>()
    );
    for format in [
        fontelle_ui::document::VocalFileFormat::Midi,
        fontelle_ui::document::VocalFileFormat::Ustx,
    ] {
        let ext = match format {
            fontelle_ui::document::VocalFileFormat::Midi => "mid",
            fontelle_ui::document::VocalFileFormat::Ustx => "ustx",
        };
        let out = bundle.with_file_name(format!("out.{ext}"));
        let said = session.export_vocal_to(index, format, &out).unwrap();
        println!("{said}");
        let back = fontelle_vocal_files::read_file(&out).unwrap();
        assert_eq!(
            back.parts.iter().map(|p| p.notes.len()).sum::<usize>(),
            session.notes().len()
        );
    }
    let froze = Instant::now();
    println!("{}", session.freeze_vocal_now(index).unwrap());
    println!("frozen in {:.1} s", froze.elapsed().as_secs_f64());
    println!("{}", session.unfreeze_vocal_now(index).unwrap());
    DocumentHost::save(&mut session).unwrap();
    let project = fontelle_app::open_project(&bundle).unwrap().project;
    let mut reopened = common::a_session_in(project, Some(bundle.clone()));
    reopened.set_singer_folders(vec![bundle.with_file_name("no-engines")]);
    reopened.set_voicebanks(
        bundle.with_file_name("voicebanks"),
        fontelle_vocal::builtin_backends(),
    );
    let first = (0..reopened.channels().len())
        .find(|i| reopened.channel_kind(*i) == Some(InstrumentKind::Vocal))
        .unwrap();
    reopened.select_channel(first);
    // Open a clip it sings in, as the producer would.
    let vocal = reopened
        .project()
        .channels
        .keys()
        .find(|id| reopened.project().channels[*id].instrument == Some(InstrumentKind::Vocal));
    let clip = reopened
        .project()
        .clips
        .iter()
        .find(|(_, c)| {
            matches!(&c.source, fontelle_model::ClipSource::Notes(d)
                if Some(d.channel) == vocal && !d.notes.is_empty())
        })
        .map(|(id, _)| id)
        .unwrap();
    reopened.open_clip(clip);
    let reopen = pump_until(&mut reopened, "reopened", |s| {
        let st = states(s);
        !st.is_empty() && st.iter().all(|x| *x == VocalRenderState::Final)
    });
    println!(
        "reopened all final in {:.0} ms",
        reopen.as_secs_f64() * 1000.0
    );
}

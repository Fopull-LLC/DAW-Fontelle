//! A song with a vocal part, sung by the test engine loaded as a library,
//! played back by the audio node at the right place.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{BEAT, note, song};
use fontelle_engine::{
    AudioNode, PrepareContext, ProcessContext, TransportSnapshot, TransportState, VocalNode,
    VocalPlayer,
};
use fontelle_host::SingerEngines;
use fontelle_model::VocalVoice;
use fontelle_sing::Backend;
use fontelle_vocal::{PhraseState, VocalRenderer, Wanted, phrases_of, table_for};

fn library_name(stem: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("{stem}.dll")
    } else if cfg!(target_os = "macos") {
        format!("lib{stem}.dylib")
    } else {
        format!("lib{stem}.so")
    }
}

fn engines() -> SingerEngines {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    let built = path.join(library_name("fontelle_testsinger"));
    assert!(built.exists(), "run `cargo build -p fontelle-testsinger`");
    let folder = path.join(format!("fontelle-vocal-e2e-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&folder);
    std::fs::copy(&built, folder.join(library_name("fontelle_testsinger"))).unwrap();
    SingerEngines::load(&[folder])
}

/// The dominant period by autocorrelation, between 80 Hz and 1 kHz.
fn fundamental(samples: &[f32], rate: u32) -> f32 {
    let min_lag = (rate as f32 / 1000.0) as usize;
    let max_lag = (rate as f32 / 80.0) as usize;
    let mut best = (0usize, f32::MIN);
    for lag in min_lag..max_lag {
        let s: f32 = samples
            .iter()
            .zip(&samples[lag..])
            .map(|(a, b)| a * b)
            .sum();
        if s > best.1 {
            best = (lag, s);
        }
    }
    rate as f32 / best.0 as f32
}

#[test]
fn a_part_is_sung_by_an_installed_engine_and_heard_in_time() {
    let engines = engines();
    let engine = engines
        .engine(fontelle_testsinger::ENGINE_NAME)
        .expect("the test engine loaded")
        .clone();
    // Two phrases: beats 2-3 ("la la") and beats 8-9.
    let mut s = song(
        "en",
        vec![
            note(BEAT * 2, BEAT, 69, "la"),
            note(BEAT * 3, BEAT, 71, "-"),
            note(BEAT * 8, BEAT, 72, ""),
            note(BEAT * 9, BEAT, 74, "lo"),
        ],
    );
    s.project.channels[s.voice].vocal = Some(VocalVoice::new(
        fontelle_testsinger::ENGINE_NAME,
        fontelle_testsinger::ALTO_ID,
        "en",
    ));
    let phrases = phrases_of(&s.project, s.voice, &s.tempo());
    assert_eq!(phrases.len(), 2);

    let backend: Arc<dyn Backend> = engine;
    let wanted: Vec<Wanted> = phrases
        .iter()
        .map(|p| Wanted {
            backend: Arc::clone(&backend),
            phrase: p.sing.clone(),
            start: p.start,
            end: p.end,
        })
        .collect();
    let mut r = VocalRenderer::new(2);
    r.want(wanted.clone());
    r.wait_for_final(&wanted, Duration::from_secs(20), &mut |_, _| {})
        .expect("sung");
    assert!(wanted.iter().all(|w| r.state(w) == PhraseState::Final));

    // The node plays it: silent before the first consonant, singing A4 at
    // beat 2.5 (1.25 s).
    let player = VocalPlayer::new();
    player.publish(table_for(&r, &wanted));
    let mut node = VocalNode::new(Arc::clone(&player));
    node.prepare(&PrepareContext {
        sample_rate: 48_000.0,
        max_block_size: 1024,
    });
    let mut song_audio = Vec::new();
    let mut at = 0i64;
    while at < 48_000 * 6 {
        let mut left = vec![0.0f32; 1024];
        let mut right = vec![0.0f32; 1024];
        {
            let mut channels: [&mut [f32]; 2] = [&mut left, &mut right];
            let mut ctx = ProcessContext {
                inputs: &[],
                outputs: &mut channels,
                all_events: &[],
                cut_events: &[],
                live_events: &[],
                audio: &[],
                node: fontelle_types::NodeId::default(),
                transport: TransportSnapshot {
                    state: TransportState::Playing,
                    position_sample: at,
                    ..Default::default()
                },
                sample_range: at..at + 1024,
            };
            node.process(&mut ctx);
        }
        song_audio.extend_from_slice(&left);
        at += 1024;
    }
    let lead = fontelle_testsinger::LEAD;
    let quiet_until = ((1.0 - lead - 0.01) * 48_000.0) as usize;
    assert!(
        song_audio[..quiet_until].iter().all(|s| *s == 0.0),
        "nothing before the first phrase's consonant"
    );
    let window = &song_audio[(1.1 * 48_000.0) as usize..(1.4 * 48_000.0) as usize];
    let hz = fundamental(window, 48_000);
    assert!((hz - 440.0).abs() < 8.0, "A4 heard as {hz} Hz");
    // The second phrase is heard at 4 s.
    let energy: f32 = song_audio[(4.1 * 48_000.0) as usize..(4.4 * 48_000.0) as usize]
        .iter()
        .map(|s| s.abs())
        .sum();
    assert!(energy > 100.0);
}

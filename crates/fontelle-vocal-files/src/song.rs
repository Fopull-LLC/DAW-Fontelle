//! What every reader makes and every writer takes.

use fontelle_model::{Note, TempoMap, TempoSegment, VocalPart};
use fontelle_types::Tick;

/// A song as a vocal file has it, on this project's grid.
#[derive(Debug, Clone, PartialEq)]
pub struct VocalSong {
    /// The file's own name for it, else the file's.
    pub name: String,
    /// Tempo changes from tick 0, in order; never empty.
    pub tempo: Vec<TempoSegment>,
    /// The (first) time signature, as written: `6/8` is six over eight.
    pub beats_per_bar: u32,
    pub beat_unit: u32,
    pub parts: Vec<SongPart>,
    /// What the file carried that has no place in a part — one line per
    /// kind, for the app to say.
    pub unmapped: Vec<String>,
}

/// One sung line: a track in the file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SongPart {
    pub name: String,
    /// The singer the file named, if any — only for saying so; a voice is
    /// chosen in Fontelle.
    pub singer: Option<String>,
    /// From the song's tick 0, in time order, one voice (no overlaps).
    pub notes: Vec<Note>,
    /// Lanes in ticks from the song's tick 0.
    pub vocal: VocalPart,
}

impl SongPart {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    /// Where its last note ends.
    pub fn end(&self) -> Tick {
        self.notes
            .iter()
            .map(|n| n.start + n.length)
            .max()
            .unwrap_or(0)
    }
}

impl VocalSong {
    /// An empty song at 120 bpm in 4/4.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            tempo: vec![TempoSegment {
                start_tick: 0,
                bpm: 120.0,
            }],
            beats_per_bar: 4,
            beat_unit: 4,
            parts: Vec::new(),
            unmapped: Vec::new(),
        }
    }

    /// The song's tempo as a map (48 kHz; only seconds are asked of it).
    pub fn tempo_map(&self) -> TempoMap {
        TempoMap::from_segments(self.tempo.clone(), 48_000.0)
    }

    /// Notes `what` as not carried over, once.
    pub fn unmapped(&mut self, what: impl Into<String>) {
        let what = what.into();
        if !self.unmapped.contains(&what) {
            self.unmapped.push(what);
        }
    }

    /// Every part's notes sorted and made one voice: of two notes starting
    /// together the higher is kept, and a note is cut where the next
    /// starts.
    pub(crate) fn tidy(&mut self) {
        for part in &mut self.parts {
            part.notes
                .sort_by(|a, b| a.start.cmp(&b.start).then(b.key.cmp(&a.key)));
            part.notes
                .dedup_by(|later, earlier| later.start == earlier.start);
            for i in 1..part.notes.len() {
                let next = part.notes[i].start;
                let prev = &mut part.notes[i - 1];
                if prev.start + prev.length > next {
                    prev.length = next - prev.start;
                }
            }
            part.notes.retain(|n| n.length > 0);
        }
        self.parts.retain(|p| !p.notes.is_empty());
        if self.tempo.is_empty() {
            self.tempo.push(TempoSegment {
                start_tick: 0,
                bpm: 120.0,
            });
        }
        self.tempo.sort_by_key(|s| s.start_tick);
        self.beats_per_bar = self.beats_per_bar.clamp(1, fontelle_types::MAX_TOP);
        self.beat_unit = self.beat_unit.clamp(1, fontelle_types::MAX_BOTTOM);
    }
}

/// A note as a vocal track has it: velocity 100 (the voice's own consonant
/// speed), nothing else set.
pub fn vocal_note(start: Tick, length: Tick, key: u8, lyric: impl Into<String>) -> Note {
    Note {
        start,
        length,
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
        lyric: lyric.into(),
        voice: None,
        muted: false,
    }
}

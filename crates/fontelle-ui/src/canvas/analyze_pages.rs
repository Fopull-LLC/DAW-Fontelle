//! What the Analyze Musically window's Clean, Slice and Record pages are
//! about (`docs/analyze-musically-plan.md` §3.1, §3.8, §6.1, P3–P5): the
//! plain data the host hands over and takes back. Pure, like the rest of
//! `canvas`; the study itself (`fontelle_types::Study`) is the document's.

use fontelle_types::{ArmMode, StudyId};

/// Where the study's audio came from, which decides what its results do.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AnalyzeSource {
    /// An arrangement clip: Render to clip replaces its audio.
    #[default]
    Clip,
    /// An Analyze Musically insert on a mixer track (Ty, plan §6.1): takes
    /// come from what plays through it.
    Insert {
        /// The track's name, for the Source chooser.
        track: String,
    },
    /// A file or a recording with no clip: Send to arrangement makes one.
    Standalone,
}

impl AnalyzeSource {
    /// Whether it records takes (an insert, or a standalone study).
    pub fn records(&self) -> bool {
        !matches!(self, Self::Clip)
    }
}

/// The Record page's state, from the host: the arm, the meter, what it is
/// hearing.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnalyzeRecordView {
    /// The input device it records from; `None` is the insert's own track.
    pub input: Option<String>,
    /// The input devices there are, for the Source chooser.
    pub inputs: Vec<String>,
    pub arm: ArmMode,
    /// On input's threshold, dBFS, and its release, milliseconds.
    pub threshold_db: f32,
    pub release_ms: f32,
    pub post_fader: bool,
    pub armed: bool,
    /// A take is being written now.
    pub recording: bool,
    /// The level heard, 0..1 (peak, linear), for the meter.
    pub level: f32,
    /// Frames lost since the capture began: not zero is a warning.
    pub dropped_frames: u64,
    /// Seconds in the take being recorded.
    pub take_seconds: f64,
    /// Why it cannot record, when it cannot (no input open, …).
    pub problem: Option<String>,
}

/// A change on the Record page.
#[derive(Debug, Clone, PartialEq)]
pub enum AnalyzeRecordOp {
    Arm(bool),
    Mode(ArmMode),
    Threshold(f32),
    Release(f32),
    PostFader(bool),
    /// `None`: the insert's own track.
    Source(Option<String>),
}

/// Something done to a take in the takes list.
#[derive(Debug, Clone, PartialEq)]
pub enum AnalyzeTakeOp {
    /// Into the lane: the study studies it.
    Load(u32),
    Star(u32),
    Rename(u32, String),
    /// Out of the study; its file goes once nothing could bring it back.
    Discard(u32),
    /// The comp, made into a take of its own and loaded.
    UseComp,
}

/// How auto-slice finds its cuts (plan §3.8).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum AutoSlice {
    /// Hand-placed markers only.
    #[default]
    Off,
    /// Where the energy jumps; `sensitivity` 0..1, more finds quieter hits.
    Transients { sensitivity: f32 },
    /// At each detected note.
    Notes,
    /// Every beat of the song's tempo.
    Beats,
    /// So many equal pieces.
    Equal { pieces: u32 },
}

impl AutoSlice {
    pub const CHOICES: [&'static str; 5] = ["Markers", "Transients", "Notes", "Beat grid", "Equal"];

    pub fn index(self) -> usize {
        match self {
            Self::Off => 0,
            Self::Transients { .. } => 1,
            Self::Notes => 2,
            Self::Beats => 3,
            Self::Equal { .. } => 4,
        }
    }

    pub fn label(self) -> &'static str {
        Self::CHOICES[self.index()]
    }
}

/// How slices land on the keyboard (`fontelle_analysis::slice::SliceLayout`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AnalyzeSliceLayout {
    /// Slice i on C3 + i, each at its own pitch (Slicex).
    #[default]
    Chop,
    /// Rooted on their own pitches: a playable multisample.
    ByPitch,
    /// Kick, snare, hats on their General MIDI keys.
    DrumMap,
}

impl AnalyzeSliceLayout {
    pub const ALL: [Self; 3] = [Self::Chop, Self::ByPitch, Self::DrumMap];

    pub fn label(self) -> &'static str {
        match self {
            Self::Chop => "Chop",
            Self::ByPitch => "By pitch",
            Self::DrumMap => "Drum map",
        }
    }
}

/// Where one slice lands, for the keyboard preview: the keys it plays on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalyzeSliceKey {
    pub slice: usize,
    pub low: u8,
    pub high: u8,
    pub root: u8,
}

/// One study in the browser's list and the window's title menu (Ty, plan §6
/// answer 4: never lost after its window closes).
#[derive(Debug, Clone, PartialEq)]
pub struct AnalyzeStudyRow {
    pub id: StudyId,
    pub name: String,
    /// Where it is from, in words: "clip", "insert on Vox", "recording".
    pub place: String,
    /// The window is open on it.
    pub open: bool,
}

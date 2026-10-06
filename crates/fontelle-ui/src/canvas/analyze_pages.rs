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

// ================================================================ knobs ===

/// The knobs on the cards: Flopsynth's (drag, Shift fine, Ctrl finer,
/// Alt-click back to default, double-click to type).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnalyzeKnob {
    // The Pitch card, for the selected notes.
    Centre,
    Drift,
    Vibrato,
    GlideIn,
    GlideOut,
    Formant,
    NoteGain,
    // The Denoise card.
    Reduce,
    Amount,
    Sensitivity,
    // Trim & fades.
    FadeIn,
    FadeOut,
    Gain,
    // Slice points.
    SliceSensitivity,
    Pieces,
    // Record (On input).
    Threshold,
    Release,
}

/// What a knob's number is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnobUnit {
    Cents,
    Percent,
    Ms,
    Db,
    Count,
}

impl AnalyzeKnob {
    pub const PITCH: [Self; 7] = [
        Self::Centre,
        Self::Drift,
        Self::Vibrato,
        Self::GlideIn,
        Self::GlideOut,
        Self::Formant,
        Self::NoteGain,
    ];

    /// The caption over it: capitals, the one place they are used.
    pub fn caption(self) -> &'static str {
        match self {
            Self::Centre => "CENTRE",
            Self::Drift => "FLATTEN",
            Self::Vibrato => "VIBRATO",
            Self::GlideIn => "GLIDE IN",
            Self::GlideOut => "GLIDE OUT",
            Self::Formant => "FORMANT",
            Self::NoteGain => "GAIN",
            Self::Reduce => "REDUCE",
            Self::Amount => "AMOUNT",
            Self::Sensitivity => "SENSITIVITY",
            Self::FadeIn => "FADE IN",
            Self::FadeOut => "FADE OUT",
            Self::Gain => "GAIN",
            Self::SliceSensitivity => "SENSITIVITY",
            Self::Pieces => "PIECES",
            Self::Threshold => "THRESHOLD",
            Self::Release => "RELEASE",
        }
    }

    pub fn unit(self) -> KnobUnit {
        match self {
            Self::Centre | Self::Formant => KnobUnit::Cents,
            Self::Drift | Self::Vibrato | Self::Amount | Self::Sensitivity => KnobUnit::Percent,
            Self::SliceSensitivity => KnobUnit::Percent,
            Self::GlideIn | Self::GlideOut | Self::FadeIn | Self::FadeOut | Self::Release => {
                KnobUnit::Ms
            }
            Self::NoteGain | Self::Reduce | Self::Gain | Self::Threshold => KnobUnit::Db,
            Self::Pieces => KnobUnit::Count,
        }
    }

    /// The knob's travel, in its unit. A typed value may go past it where
    /// the unit allows (a note moved two octaves).
    pub fn range(self) -> (f32, f32) {
        match self {
            Self::Centre => (-200.0, 200.0),
            Self::Formant => (-600.0, 600.0),
            Self::Drift | Self::Amount | Self::Sensitivity | Self::SliceSensitivity => (0.0, 100.0),
            Self::Vibrato => (0.0, 200.0),
            Self::GlideIn | Self::GlideOut => (0.0, 400.0),
            Self::NoteGain => (-12.0, 12.0),
            Self::Reduce => (0.0, 40.0),
            Self::FadeIn | Self::FadeOut => (0.0, 2_000.0),
            Self::Gain => (-24.0, 24.0),
            Self::Pieces => (2.0, 64.0),
            Self::Threshold => (-80.0, 0.0),
            Self::Release => (50.0, 5_000.0),
        }
    }

    /// What Alt-click puts back.
    pub fn default_value(self) -> f32 {
        match self {
            Self::Vibrato => 100.0,
            Self::GlideIn | Self::GlideOut => fontelle_types::DEFAULT_GLIDE_MS,
            Self::Reduce => 12.0,
            Self::Amount | Self::Sensitivity | Self::SliceSensitivity => 50.0,
            Self::Pieces => 8.0,
            Self::Threshold => -40.0,
            Self::Release => 1_000.0,
            _ => 0.0,
        }
    }

    /// How far a typed value may go.
    fn typed_range(self) -> (f32, f32) {
        match self {
            Self::Centre => (-2_400.0, 2_400.0),
            Self::Formant => (-1_200.0, 1_200.0),
            Self::FadeIn | Self::FadeOut => (0.0, 60_000.0),
            Self::Pieces => (2.0, 256.0),
            _ => self.range(),
        }
    }

    /// 0..1 along the knob's travel. Fades and the release are shaped, so
    /// the short ones a voice wants have room.
    pub fn to_unit(self, value: f32) -> f32 {
        let (lo, hi) = self.range();
        let t = ((value - lo) / (hi - lo)).clamp(0.0, 1.0);
        match self {
            Self::FadeIn | Self::FadeOut | Self::Release => t.sqrt(),
            _ => t,
        }
    }

    pub fn from_unit(self, t: f32) -> f32 {
        let (lo, hi) = self.range();
        let t = t.clamp(0.0, 1.0);
        let t = match self {
            Self::FadeIn | Self::FadeOut | Self::Release => t * t,
            _ => t,
        };
        let v = lo + t * (hi - lo);
        match self.unit() {
            KnobUnit::Count => v.round(),
            KnobUnit::Ms if v >= 100.0 => (v / 5.0).round() * 5.0,
            _ => v,
        }
    }

    /// The read-out: "+23 ct", "70 %", "40 ms", "-6.0 dB", "8".
    pub fn display(self, value: f32) -> String {
        match self.unit() {
            KnobUnit::Cents => format!("{:+} ct", value.round() as i32),
            KnobUnit::Percent => format!("{} %", value.round() as i32),
            KnobUnit::Ms if value >= 1_000.0 => format!("{:.2} s", value / 1000.0),
            KnobUnit::Ms => format!("{} ms", value.round() as i32),
            KnobUnit::Db => {
                if self == Self::Threshold || self == Self::Reduce {
                    format!("{} dB", value.round() as i32)
                } else {
                    format!("{:+.1} dB", value)
                }
            }
            KnobUnit::Count => format!("{}", value.round() as i32),
        }
    }

    /// A typed value in this knob's unit (`super::parse_typed`): "30", "+30
    /// ct", "1.5 s", "-6 dB", "70%".
    pub fn parse(self, text: &str) -> Option<f32> {
        let typed = super::parse_typed(text)?;
        let value = match (typed.unit.as_str(), self.unit()) {
            ("" | "c" | "ct", KnobUnit::Cents) => typed.value,
            ("st", KnobUnit::Cents) => typed.value * 100.0,
            ("%", KnobUnit::Percent) => typed.value * 100.0,
            ("", KnobUnit::Percent) => typed.value,
            ("" | "ms", KnobUnit::Ms) => typed.value,
            ("s", KnobUnit::Ms) => typed.value * 1000.0,
            ("" | "db", KnobUnit::Db) => typed.value,
            ("", KnobUnit::Count) => typed.value.round(),
            _ => return None,
        };
        let (lo, hi) = self.typed_range();
        Some(value.clamp(lo, hi))
    }

    /// What it does, for the tip.
    pub fn tip(self) -> &'static str {
        match self {
            Self::Centre => "Where the selected notes sit: drag to move them by the cent",
            Self::Drift => "Take out the slow wander about the note (F toggles 70 %)",
            Self::Vibrato => "The vibrato's depth: 100 % as sung, 0 % none (V steps it)",
            Self::GlideIn => "How long the move takes to arrive",
            Self::GlideOut => "How long the move takes to leave",
            Self::Formant => "Shift the voice's colour without its pitch",
            Self::NoteGain => "The selected notes louder or quieter",
            Self::Reduce => "How far the noise is turned down, at most",
            Self::Amount => "How hard the noise is taken out: more is cleaner and less natural",
            Self::Sensitivity => "How much counts as noise: higher takes out more",
            Self::FadeIn => "A fade in from the start of what is kept",
            Self::FadeOut => "A fade out to the end of what is kept",
            Self::Gain => "The whole take louder or quieter",
            Self::SliceSensitivity => "How quiet a hit still makes a slice: watch the lane",
            Self::Pieces => "How many equal slices",
            Self::Threshold => "How loud the sound has to be to start a take",
            Self::Release => "How long it stays quiet before the take ends",
        }
    }
}

/// The selected notes a Pitch knob shows and moves: the first selected note
/// that can be moved, `None` with none.
pub fn pitch_focus(view: &super::AnalyzeView, state: &super::AnalyzeState) -> Option<usize> {
    let notes = state.notes(view);
    state
        .selected()
        .into_iter()
        .find(|i| notes.get(*i).is_some_and(|n| !n.poly))
}

/// What a knob reads now; `None` while it has nothing to act on (a Pitch
/// knob with no note selected, a Record knob with no recorder).
pub fn analyze_knob_value(
    knob: AnalyzeKnob,
    view: &super::AnalyzeView,
    state: &super::AnalyzeState,
) -> Option<f32> {
    use AnalyzeKnob as K;
    let clean = &view.clean;
    let ms = |frames: i64| frames as f32 * 1000.0 / view.rate.max(1) as f32;
    Some(match knob {
        K::Centre | K::Drift | K::Vibrato | K::GlideIn | K::GlideOut | K::Formant | K::NoteGain => {
            let note = state.notes(view).get(pitch_focus(view, state)?)?;
            let e = note.edit.unwrap_or_default();
            match knob {
                K::Centre => note.cents + e.shift_cents,
                K::Drift => e.flatten * 100.0,
                K::Vibrato => e.vibrato * 100.0,
                K::GlideIn => e.glide_in_ms,
                K::GlideOut => e.glide_out_ms,
                K::Formant => e.formant_cents,
                _ => e.gain_db,
            }
        }
        K::Reduce => clean.denoise.reduce_db,
        K::Amount => clean.denoise.amount * 100.0,
        K::Sensitivity => clean.denoise.sensitivity * 100.0,
        K::FadeIn => ms(clean.fade_in),
        K::FadeOut => ms(clean.fade_out),
        K::Gain => clean.gain_db,
        K::SliceSensitivity => match state.auto {
            AutoSlice::Transients { sensitivity } => sensitivity * 100.0,
            _ => return None,
        },
        K::Pieces => match state.auto {
            AutoSlice::Equal { pieces } => pieces as f32,
            _ => return None,
        },
        K::Threshold => view.record.as_ref()?.threshold_db,
        K::Release => view.record.as_ref()?.release_ms,
    })
}

/// What setting a knob to `value` asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum AnalyzeKnobChange {
    /// The selected notes' edits.
    Edits(Vec<super::AnalyzeEditChange>),
    /// The Clean page, whole.
    Clean(fontelle_types::StudyClean),
    /// The window's own (auto-slice): already done to the state.
    State,
    Record(AnalyzeRecordOp),
    /// Nothing to act on; why.
    Nothing(String),
}

/// `knob` set to `value`, as a change for the host (or done to `state`).
/// A Pitch knob sets every selected note: Centre moves them all by as much
/// as the focused one moves; the others set each one alike.
pub fn analyze_knob_change(
    knob: AnalyzeKnob,
    value: f32,
    view: &super::AnalyzeView,
    state: &mut super::AnalyzeState,
) -> AnalyzeKnobChange {
    use AnalyzeKnob as K;
    let frames =
        |ms: f32| (f64::from(ms.max(0.0)) * f64::from(view.rate.max(1)) / 1000.0).round() as i64;
    match knob {
        K::Centre | K::Drift | K::Vibrato | K::GlideIn | K::GlideOut | K::Formant | K::NoteGain => {
            let Some(focus) = pitch_focus(view, state) else {
                return AnalyzeKnobChange::Nothing(super::SELECT_FIRST.to_string());
            };
            let notes = state.notes(view);
            let held = &notes[focus];
            let delta = value - (held.cents + held.edit.unwrap_or_default().shift_cents);
            let changes = state
                .selected()
                .into_iter()
                .filter_map(|i| notes.get(i).filter(|n| !n.poly))
                .map(|n| {
                    let mut e = n.edit.unwrap_or_default();
                    match knob {
                        K::Centre => e.shift_cents += delta,
                        K::Drift => e.flatten = (value / 100.0).clamp(0.0, 1.0),
                        K::Vibrato => e.vibrato = (value / 100.0).max(0.0),
                        K::GlideIn => e.glide_in_ms = value.max(0.0),
                        K::GlideOut => e.glide_out_ms = value.max(0.0),
                        K::Formant => e.formant_cents = value,
                        _ => e.gain_db = value,
                    }
                    super::AnalyzeEditChange {
                        start: n.start,
                        end: n.end,
                        edit: Some(e).filter(|e| !e.is_identity()),
                    }
                })
                .collect();
            AnalyzeKnobChange::Edits(changes)
        }
        K::Reduce | K::Amount | K::Sensitivity | K::FadeIn | K::FadeOut | K::Gain => {
            let mut clean = view.clean.clone();
            match knob {
                K::Reduce => clean.denoise.reduce_db = value,
                K::Amount => clean.denoise.amount = value / 100.0,
                K::Sensitivity => clean.denoise.sensitivity = value / 100.0,
                K::FadeIn => clean.fade_in = frames(value),
                K::FadeOut => clean.fade_out = frames(value),
                _ => clean.gain_db = value,
            }
            // Turning a denoise knob means wanting it on.
            if matches!(knob, K::Reduce | K::Amount | K::Sensitivity)
                && clean.denoise.noise.is_some()
            {
                clean.denoise.on = true;
            }
            AnalyzeKnobChange::Clean(clean)
        }
        K::SliceSensitivity => {
            state.auto = AutoSlice::Transients {
                sensitivity: (value / 100.0).clamp(0.0, 1.0),
            };
            AnalyzeKnobChange::State
        }
        K::Pieces => {
            state.auto = AutoSlice::Equal {
                pieces: value.round().clamp(2.0, 256.0) as u32,
            };
            AnalyzeKnobChange::State
        }
        K::Threshold => AnalyzeKnobChange::Record(AnalyzeRecordOp::Threshold(value)),
        K::Release => AnalyzeKnobChange::Record(AnalyzeRecordOp::Release(value)),
    }
}

// ============================================================= slicing ===

/// Where auto-slice (or the markers) cut, in the lane's seconds, inside
/// the trim: what the lane shows and what Send to sampler cuts at.
pub fn analyze_cuts(view: &super::AnalyzeView, state: &super::AnalyzeState) -> Vec<f64> {
    let (from, to) = view.trim_seconds();
    let inside = |t: &f64| *t > from + 1e-3 && *t < to - 1e-3;
    let mut cuts: Vec<f64> = match state.auto {
        AutoSlice::Off => view.markers.iter().map(|m| view.seconds_of(m.at)).collect(),
        AutoSlice::Transients { sensitivity } => {
            let line = 1.0 - sensitivity.clamp(0.0, 1.0);
            view.onsets
                .iter()
                .filter(|(_, s)| *s >= line * 0.9)
                .map(|(t, _)| *t)
                .collect()
        }
        AutoSlice::Notes => state.notes(view).iter().map(|n| n.start).collect(),
        AutoSlice::Beats => match view.beat_seconds {
            Some(beat) if beat > 0.01 => {
                let mut t = from + beat;
                let mut out = Vec::new();
                while t < to && out.len() < 512 {
                    out.push(t);
                    t += beat;
                }
                out
            }
            _ => Vec::new(),
        },
        AutoSlice::Equal { pieces } => {
            let n = pieces.max(2);
            (1..n)
                .map(|i| from + (to - from) * f64::from(i) / f64::from(n))
                .collect()
        }
    };
    cuts.retain(inside);
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|a, b| (*a - *b).abs() < 0.005);
    cuts
}

// ============================================================ controls ===

/// A control on a page's cards (or the header's Studies chip).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnalyzeControl {
    Knob(AnalyzeKnob),
    // Clean.
    CaptureNoise,
    ListenRemoved,
    DenoiseOn,
    VoiceDenoise,
    FadeShape,
    TrimToSelection,
    ResetClean,
    // Slice.
    AutoSlice,
    UseAsMarkers,
    ClearMarkers,
    Layout(AnalyzeSliceLayout),
    Replay,
    SendToSampler,
    // Record.
    Source,
    PostFader,
    Arm,
    ArmMode(fontelle_types::ArmMode),
    SendToArrangement,
    UseComp,
    ClearComp,
}

/// How a control is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalyzeControlKind {
    /// A knob in a whole Flopsynth cell: caption, knob, read-out.
    Knob,
    Button,
    /// The main action of its card: the accent's button.
    Primary,
    /// A switch with its words beside it.
    Switch,
    /// A chooser: a chip with its value and a wedge.
    Chip,
    /// One part of a segmented choice.
    Segment,
    /// The record lamp.
    Lamp,
}

impl AnalyzeControl {
    pub fn kind(self) -> AnalyzeControlKind {
        use AnalyzeControlKind as K;
        match self {
            Self::Knob(_) => K::Knob,
            Self::ListenRemoved | Self::DenoiseOn | Self::VoiceDenoise | Self::Replay => K::Switch,
            Self::PostFader => K::Switch,
            Self::FadeShape | Self::AutoSlice | Self::Source => K::Chip,
            Self::Layout(_) | Self::ArmMode(_) => K::Segment,
            Self::Arm => K::Lamp,
            Self::SendToSampler | Self::SendToArrangement => K::Primary,
            _ => K::Button,
        }
    }

    /// The words on it (a switch's are beside it). Knobs, chips and the lamp
    /// say their value, from [`control_text`].
    pub fn label(self) -> &'static str {
        match self {
            Self::Knob(k) => k.caption(),
            Self::CaptureNoise => "Capture noise",
            Self::ListenRemoved => "Hear what's removed",
            Self::DenoiseOn => "On",
            Self::VoiceDenoise => "Voice",
            Self::FadeShape => "Shape",
            Self::TrimToSelection => "Trim to selection",
            Self::ResetClean => "Reset",
            Self::AutoSlice => "Find",
            Self::UseAsMarkers => "Use as markers",
            Self::ClearMarkers => "Clear markers",
            Self::Layout(l) => l.label(),
            Self::Replay => "Also a clip that replays them",
            Self::SendToSampler => "Send to sampler",
            Self::Source => "Source",
            Self::PostFader => "After the fader",
            Self::Arm => "Arm",
            Self::ArmMode(m) => match m {
                fontelle_types::ArmMode::OnPlay => "On play",
                fontelle_types::ArmMode::OnInput => "On input",
                fontelle_types::ArmMode::Now => "Now",
            },
            Self::SendToArrangement => "Send to arrangement",
            Self::UseComp => "Use comp",
            Self::ClearComp => "Clear comp",
        }
    }

    /// Its name for the tests' sweep: `card.clean.capture-noise`.
    pub fn id(self) -> String {
        match self {
            Self::Knob(k) => format!("knob.{}", k.caption().to_lowercase().replace(' ', "-")),
            other => format!(
                "control.{}",
                other.label().to_lowercase().replace([' ', '\''], "-")
            ),
        }
    }
}

/// What a chip or the lamp says now.
pub fn control_text(
    control: AnalyzeControl,
    view: &super::AnalyzeView,
    state: &super::AnalyzeState,
) -> String {
    match control {
        AnalyzeControl::FadeShape => view.clean.fade_shape.label().to_string(),
        AnalyzeControl::AutoSlice => state.auto.label().to_string(),
        AnalyzeControl::Source => source_text(view),
        AnalyzeControl::Arm => {
            if state.meter.recording || view.record.as_ref().is_some_and(|r| r.recording) {
                "Recording".to_string()
            } else if view.record.as_ref().is_some_and(|r| r.armed) {
                "Armed".to_string()
            } else {
                "Arm".to_string()
            }
        }
        other => other.label().to_string(),
    }
}

/// The Source chip: "This track (Vox)" or the device's name.
pub fn source_text(view: &super::AnalyzeView) -> String {
    match (&view.record, &view.source) {
        (Some(record), _) if record.input.is_some() => record.input.clone().unwrap_or_default(),
        (_, AnalyzeSource::Insert { track }) => format!("This track ({track})"),
        _ => "No input".to_string(),
    }
}

/// Whether a switch is on.
pub fn switch_on(
    control: AnalyzeControl,
    view: &super::AnalyzeView,
    state: &super::AnalyzeState,
) -> bool {
    match control {
        AnalyzeControl::ListenRemoved => state.listen_removed,
        AnalyzeControl::DenoiseOn => view.clean.denoise.on,
        AnalyzeControl::VoiceDenoise => view.clean.denoise.voice,
        AnalyzeControl::Replay => state.replay,
        AnalyzeControl::PostFader => view.record.as_ref().is_some_and(|r| r.post_fader),
        AnalyzeControl::Layout(l) => state.layout == l,
        AnalyzeControl::ArmMode(m) => view.record.as_ref().is_some_and(|r| r.arm == m),
        _ => false,
    }
}

/// Whether a control can do anything now: a greyed one says why in its tip.
pub fn control_enabled(
    control: AnalyzeControl,
    view: &super::AnalyzeView,
    state: &super::AnalyzeState,
) -> bool {
    match control {
        AnalyzeControl::Knob(k) => analyze_knob_value(k, view, state).is_some(),
        AnalyzeControl::CaptureNoise | AnalyzeControl::TrimToSelection => state.span.is_some(),
        AnalyzeControl::ListenRemoved => view.clean.denoise.active(),
        AnalyzeControl::DenoiseOn => view.clean.denoise.noise.is_some() || view.clean.denoise.voice,
        AnalyzeControl::ResetClean => !view.clean.is_identity(),
        AnalyzeControl::UseAsMarkers => state.auto != AutoSlice::Off,
        AnalyzeControl::ClearMarkers => !view.markers.is_empty(),
        AnalyzeControl::SendToSampler => view.has_audio && view.rendering.is_none(),
        AnalyzeControl::SendToArrangement => view.has_audio && view.rendering.is_none(),
        AnalyzeControl::UseComp | AnalyzeControl::ClearComp => !view.comp.is_empty(),
        AnalyzeControl::PostFader => view.record.as_ref().is_some_and(|r| r.input.is_none()),
        AnalyzeControl::Arm | AnalyzeControl::ArmMode(_) | AnalyzeControl::Source => {
            view.record.is_some()
        }
        _ => true,
    }
}

/// The tip over a control.
pub fn control_tip(
    control: AnalyzeControl,
    view: &super::AnalyzeView,
    state: &super::AnalyzeState,
) -> String {
    let enabled = control_enabled(control, view, state);
    match control {
        AnalyzeControl::Knob(k) => match analyze_knob_value(k, view, state) {
            Some(v) => format!(
                "{} \u{2014} {} (drag; Shift fine; Alt-click resets; double-click types)",
                k.display(v),
                k.tip()
            ),
            None if AnalyzeKnob::PITCH.contains(&k) => {
                format!("{} \u{2014} select a note first", k.tip())
            }
            None => k.tip().to_string(),
        },
        AnalyzeControl::CaptureNoise if !enabled => {
            "Drag over a stretch with only noise in it (the Noise tool captures as you let go)"
                .to_string()
        }
        AnalyzeControl::CaptureNoise => "Capture the noise in the selection".to_string(),
        AnalyzeControl::ListenRemoved => {
            "Hear only what the denoiser takes out: it should be noise, not the voice".to_string()
        }
        AnalyzeControl::DenoiseOn if !enabled => {
            "Capture some noise first, with the Noise tool".to_string()
        }
        AnalyzeControl::DenoiseOn => "Take the captured noise out".to_string(),
        AnalyzeControl::VoiceDenoise => {
            "A denoiser that knows voices: no profile needed".to_string()
        }
        AnalyzeControl::FadeShape => "The fades' curve".to_string(),
        AnalyzeControl::TrimToSelection if !enabled => {
            "Select a stretch on the lane first, or drag the lane's ends".to_string()
        }
        AnalyzeControl::TrimToSelection => "Keep only the selection".to_string(),
        AnalyzeControl::ResetClean => "Trim, fades, gain and denoise back as recorded".to_string(),
        AnalyzeControl::AutoSlice => {
            "Where to cut: your markers, or found for you \u{2014} the lane shows them".to_string()
        }
        AnalyzeControl::UseAsMarkers => {
            "Make these cuts markers, to move or remove one by one".to_string()
        }
        AnalyzeControl::ClearMarkers => "Remove every marker".to_string(),
        AnalyzeControl::Layout(AnalyzeSliceLayout::Chop) => {
            "One slice a key from C3 up, each at its own pitch".to_string()
        }
        AnalyzeControl::Layout(AnalyzeSliceLayout::ByPitch) => {
            "Each slice on the key it sings: a playable instrument".to_string()
        }
        AnalyzeControl::Layout(AnalyzeSliceLayout::DrumMap) => {
            "Kicks on C1, snares on D1, hats on F#1: ready for a drum pattern".to_string()
        }
        AnalyzeControl::Replay => {
            "Also a note clip that plays the slices back in order".to_string()
        }
        AnalyzeControl::SendToSampler => format!(
            "{} slices onto a new Sampler (one undo)",
            analyze_cuts(view, state).len() + 1
        ),
        AnalyzeControl::Source => "What it records: this track, or an input".to_string(),
        AnalyzeControl::PostFader => {
            "Record after the track's fader and pan, rather than before".to_string()
        }
        AnalyzeControl::Arm => match view.record.as_ref().map(|r| r.arm) {
            Some(fontelle_types::ArmMode::OnPlay) => {
                "Arm: records while the song plays".to_string()
            }
            Some(fontelle_types::ArmMode::OnInput) => {
                "Arm: records when the sound crosses the threshold".to_string()
            }
            _ => "Arm: records from now until you press again".to_string(),
        },
        AnalyzeControl::ArmMode(fontelle_types::ArmMode::OnPlay) => {
            "Record while the song plays".to_string()
        }
        AnalyzeControl::ArmMode(fontelle_types::ArmMode::OnInput) => {
            "Record when the sound starts, stop when it goes quiet".to_string()
        }
        AnalyzeControl::ArmMode(fontelle_types::ArmMode::Now) => {
            "Record from the press until the next, playing or not".to_string()
        }
        AnalyzeControl::SendToArrangement => {
            "The take in the lane, cleaned and edited, as a clip where it was recorded".to_string()
        }
        AnalyzeControl::UseComp if !enabled => {
            "Drag across the takes to choose the best of each first".to_string()
        }
        AnalyzeControl::UseComp => "Make the comp a take, and put it in the lane".to_string(),
        AnalyzeControl::ClearComp => "Forget the comp's spans".to_string(),
    }
}

// ============================================================== layout ===

/// One row of the Record page's takes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnalyzeTakeRow {
    pub id: u32,
    pub row: Rect,
    pub star: Rect,
    pub name: Rect,
    pub discard: Rect,
    /// Its audio across the takes' shared time: drag here to comp.
    pub lane: Rect,
}

use crate::layout::Rect;

/// A Flopsynth cell, design size.
const CELL_W: f32 = 56.0;
const CELL_H: f32 = 72.0;
/// The Pitch card's cells: room for "GLIDE OUT" over its knob.
const PITCH_CELL_W: f32 = 62.0;
const ROW_H: f32 = 26.0;
const TAKE_ROW_H: f32 = 30.0;
const TAKE_HEAD_W: f32 = 190.0;

/// Places a page's cards and their controls along the bottom of the body,
/// from `cards_y`, `cards_h` tall. `card` makes a card's layout from its
/// frame; `width_of` is a chip's width for a string.
pub(super) fn lay_out_cards(
    l: &mut super::AnalyzeLayout,
    view: &super::AnalyzeView,
    state: &super::AnalyzeState,
    cards_y: f32,
    cards_h: f32,
    card: &dyn Fn(Rect) -> super::AnalyzeCardLayout,
    width_of: &dyn Fn(&str) -> f32,
) {
    use super::AnalyzeCard as C;
    let s = l.scale;
    let sc = |v: f32| (v * s).round();
    let gap = sc(8.0);
    let body = l.body;
    let button = |text: &str| width_of(text) + sc(8.0);
    let row_h = sc(ROW_H);
    // Three cards: the first and the middle `w0`/`w1` wide, the last the
    // rest.
    let three = |l: &mut super::AnalyzeLayout, kinds: [C; 3], w0: f32, w1: f32| {
        let w0 = w0.min(body.width * 0.4);
        let w1 = w1.min((body.width - w0 - gap * 2.0) * 0.6);
        let frames = [
            Rect::new(body.x, cards_y, w0, cards_h),
            Rect::new(body.x + w0 + gap, cards_y, w1, cards_h),
            Rect::new(
                body.x + w0 + w1 + gap * 2.0,
                cards_y,
                (body.width - w0 - w1 - gap * 2.0).max(0.0),
                cards_h,
            ),
        ];
        for (kind, frame) in kinds.into_iter().zip(frames) {
            l.cards.push((kind, card(frame)));
        }
    };
    let body_of = |l: &super::AnalyzeLayout, kind: C| {
        l.cards
            .iter()
            .find(|(k, _)| *k == kind)
            .map_or(Rect::ZERO, |(_, c)| c.body)
    };
    // Knob cells across a card's body, from the left.
    // A knob's cell: Flopsynth's, wider where its caption needs it.
    let cell_w = |knob: AnalyzeKnob| sc(CELL_W).max(width_of(knob.caption()) - sc(14.0));
    let knobs = |l: &mut super::AnalyzeLayout, inner: Rect, list: &[AnalyzeKnob], x: &mut f32| {
        for knob in list {
            let w = cell_w(*knob);
            let r = Rect::new(
                *x,
                inner.y + ((inner.height - sc(CELL_H)) / 2.0).max(0.0),
                w,
                sc(CELL_H),
            );
            if r.right() <= inner.right() + 0.5 {
                l.controls.push((AnalyzeControl::Knob(*knob), r));
            }
            *x += w;
        }
    };
    match state.page {
        super::AnalyzePage::Notes => {
            // The Output card's two rows of buttons decide what is left for
            // the other two: the Note card gives first, then the Pitch
            // card's knobs go to two rows of small ones.
            let output_w = {
                let row1 = button(super::COPY_NOTES)
                    + button(super::MAKE_CLIP)
                    + button(super::RENDER_TO_CLIP)
                    + sc(24.0);
                let row2 = sc(40.0)
                    + width_of(super::KEEP_BENDS)
                    + button(super::COPY_SCALE)
                    + button(super::REVERT);
                row1.max(row2) + gap * 2.0 + sc(20.0)
            };
            let whole = sc(PITCH_CELL_W) * AnalyzeKnob::PITCH.len() as f32 + sc(20.0);
            let half = sc(CELL_W + 4.0) * 4.0 + sc(20.0);
            let room = body.width - gap * 2.0 - output_w;
            let mut note_w = (body.width * 0.2).clamp(sc(180.0), sc(250.0));
            let compact = note_w + whole > room;
            let pitch_w = if compact { half } else { whole };
            if note_w + pitch_w > room {
                note_w = (room - pitch_w).max(sc(150.0));
            }
            three(l, [C::Note, C::Pitch, C::Output], note_w, pitch_w);
            let inner = body_of(l, C::Pitch);
            if compact {
                let (w, h) = (sc(CELL_W + 4.0), (inner.height / 2.0).floor());
                for (i, knob) in AnalyzeKnob::PITCH.iter().enumerate() {
                    let (col, row) = ((i % 4) as f32, (i / 4) as f32);
                    l.controls.push((
                        AnalyzeControl::Knob(*knob),
                        Rect::new(inner.x + col * w, inner.y + row * h, w, h),
                    ));
                }
            } else {
                for (i, knob) in AnalyzeKnob::PITCH.iter().enumerate() {
                    l.controls.push((
                        AnalyzeControl::Knob(*knob),
                        Rect::new(
                            inner.x + i as f32 * sc(PITCH_CELL_W),
                            inner.y + ((inner.height - sc(CELL_H)) / 2.0).max(0.0),
                            sc(PITCH_CELL_W),
                            sc(CELL_H),
                        ),
                    ));
                }
            }
        }
        super::AnalyzePage::Clean => {
            let noise_w = (body.width * 0.24).clamp(sc(220.0), sc(290.0));
            let denoise_w = sc(CELL_W + 20.0)
                + [
                    AnalyzeKnob::Reduce,
                    AnalyzeKnob::Amount,
                    AnalyzeKnob::Sensitivity,
                ]
                .iter()
                .map(|k| cell_w(*k))
                .sum::<f32>()
                + sc(20.0);
            three(l, [C::Noise, C::Denoise, C::Shape], noise_w, denoise_w);
            // The Noise card: what was captured and the button on the
            // first row, hearing what is taken out on the second.
            let inner = body_of(l, C::Noise);
            let w = button(AnalyzeControl::CaptureNoise.label()).min(inner.width);
            l.controls.push((
                AnalyzeControl::CaptureNoise,
                Rect::new(inner.right() - w, inner.y + sc(2.0), w, row_h),
            ));
            l.info = Rect::new(
                inner.x,
                inner.y + sc(2.0),
                (inner.width - w - gap).max(0.0),
                row_h,
            );
            l.controls.push((
                AnalyzeControl::ListenRemoved,
                Rect::new(inner.x, inner.y + row_h + sc(14.0), inner.width, row_h),
            ));
            // Denoise: its switch over the knobs' column, then the knobs.
            let inner = body_of(l, C::Denoise);
            let mut x = inner.x;
            let first = Rect::new(
                x,
                inner.y + ((inner.height - sc(CELL_H)) / 2.0).max(0.0),
                sc(CELL_W + 16.0),
                sc(CELL_H),
            );
            l.controls.push((
                AnalyzeControl::DenoiseOn,
                Rect::new(first.x, first.y + sc(10.0), first.width, row_h),
            ));
            if view.voice_denoise {
                l.controls.push((
                    AnalyzeControl::VoiceDenoise,
                    Rect::new(first.x, first.y + sc(14.0) + row_h, first.width, row_h),
                ));
            }
            x += first.width + sc(4.0);
            knobs(
                l,
                inner,
                &[
                    AnalyzeKnob::Reduce,
                    AnalyzeKnob::Amount,
                    AnalyzeKnob::Sensitivity,
                ],
                &mut x,
            );
            // Trim & fades: fade in, fade out, gain; the shape; the two
            // buttons stacked at the right.
            let inner = body_of(l, C::Shape);
            let mut x = inner.x;
            knobs(
                l,
                inner,
                &[AnalyzeKnob::FadeIn, AnalyzeKnob::FadeOut, AnalyzeKnob::Gain],
                &mut x,
            );
            let chip_w = width_of("Exponential") + sc(18.0);
            x += sc(4.0);
            l.controls.push((
                AnalyzeControl::FadeShape,
                Rect::new(x, inner.y + sc(14.0) + sc(12.0), chip_w, row_h),
            ));
            let bw = button(AnalyzeControl::TrimToSelection.label());
            let bx = (inner.right() - bw).max(x + chip_w + gap);
            l.controls.push((
                AnalyzeControl::TrimToSelection,
                Rect::new(bx, inner.y + sc(4.0), (inner.right() - bx).max(0.0), row_h),
            ));
            l.controls.push((
                AnalyzeControl::ResetClean,
                Rect::new(
                    bx,
                    inner.y + sc(4.0) + row_h + sc(8.0),
                    (inner.right() - bx).max(0.0),
                    row_h,
                ),
            ));
        }
        super::AnalyzePage::Slice => {
            let points_w = (body.width * 0.32).clamp(sc(300.0), sc(380.0));
            let layout_w = (body.width * 0.36).clamp(sc(300.0), sc(420.0));
            three(l, [C::Points, C::Layout, C::Send], points_w, layout_w);
            let inner = body_of(l, C::Points);
            let chip_w = width_of("Transients") + sc(18.0);
            l.controls.push((
                AnalyzeControl::AutoSlice,
                Rect::new(inner.x, inner.y + sc(18.0), chip_w, row_h),
            ));
            let mut x = inner.x + chip_w + gap;
            match state.auto {
                AutoSlice::Transients { .. } => {
                    knobs(l, inner, &[AnalyzeKnob::SliceSensitivity], &mut x)
                }
                AutoSlice::Equal { .. } => knobs(l, inner, &[AnalyzeKnob::Pieces], &mut x),
                _ => {}
            }
            let bw = button(AnalyzeControl::UseAsMarkers.label());
            let bx = (inner.right() - bw).max(x + sc(4.0));
            l.controls.push((
                AnalyzeControl::UseAsMarkers,
                Rect::new(bx, inner.y + sc(4.0), (inner.right() - bx).max(0.0), row_h),
            ));
            l.controls.push((
                AnalyzeControl::ClearMarkers,
                Rect::new(
                    bx,
                    inner.y + sc(4.0) + row_h + sc(8.0),
                    (inner.right() - bx).max(0.0),
                    row_h,
                ),
            ));
            l.info = Rect::new(inner.x, inner.bottom() - row_h, chip_w, row_h);
            // Layout: the three ways across the top, the keyboard under.
            let inner = body_of(l, C::Layout);
            let seg_w = (inner.width / 3.0).floor();
            for (i, layout) in AnalyzeSliceLayout::ALL.into_iter().enumerate() {
                l.controls.push((
                    AnalyzeControl::Layout(layout),
                    Rect::new(inner.x + seg_w * i as f32, inner.y, seg_w, row_h),
                ));
            }
            l.keyboard = Rect::new(
                inner.x,
                inner.y + row_h + sc(6.0),
                inner.width,
                (inner.height - row_h - sc(6.0)).max(0.0),
            );
            // Send: the replay switch, and the button.
            let inner = body_of(l, C::Send);
            l.controls.push((
                AnalyzeControl::Replay,
                Rect::new(inner.x, inner.y, inner.width, row_h),
            ));
            let bw = button(AnalyzeControl::SendToSampler.label()) + sc(16.0);
            l.controls.push((
                AnalyzeControl::SendToSampler,
                Rect::new(
                    inner.x,
                    inner.bottom() - row_h - sc(6.0),
                    bw.min(inner.width),
                    row_h + sc(6.0),
                ),
            ));
        }
        super::AnalyzePage::Record => {
            let source_w = (body.width * 0.26).clamp(sc(230.0), sc(300.0));
            let record_w = (body.width * 0.4).clamp(sc(360.0), sc(470.0));
            three(l, [C::Source, C::Record, C::Takes], source_w, record_w);
            let inner = body_of(l, C::Source);
            l.controls.push((
                AnalyzeControl::Source,
                Rect::new(inner.x, inner.y + sc(4.0), inner.width, row_h),
            ));
            l.controls.push((
                AnalyzeControl::PostFader,
                Rect::new(inner.x, inner.y + sc(12.0) + row_h, inner.width, row_h),
            ));
            // Record: the lamp, the modes, the On input knobs, the meter.
            let inner = body_of(l, C::Record);
            let lamp = sc(CELL_H - 8.0).min(inner.height);
            l.controls.push((
                AnalyzeControl::Arm,
                Rect::new(inner.x, inner.y + (inner.height - lamp) / 2.0, lamp, lamp),
            ));
            let mut x = inner.x + lamp + gap;
            let modes_w = width_of("On input") * 3.0 + sc(12.0);
            let seg_w = (modes_w / 3.0).floor();
            for (i, mode) in fontelle_types::ArmMode::ALL.into_iter().enumerate() {
                l.controls.push((
                    AnalyzeControl::ArmMode(mode),
                    Rect::new(x + seg_w * i as f32, inner.y + sc(4.0), seg_w, row_h),
                ));
            }
            l.meter = Rect::new(
                x,
                inner.y + sc(4.0) + row_h + sc(12.0),
                seg_w * 3.0,
                sc(10.0),
            );
            l.info = Rect::new(x, l.meter.bottom() + sc(4.0), seg_w * 3.0, row_h * 0.8);
            x += seg_w * 3.0 + gap;
            if view.record.as_ref().map(|r| r.arm) == Some(fontelle_types::ArmMode::OnInput) {
                knobs(
                    l,
                    inner,
                    &[AnalyzeKnob::Threshold, AnalyzeKnob::Release],
                    &mut x,
                );
            }
            // Takes: send, and the comp's two.
            let inner = body_of(l, C::Takes);
            let bw = button(AnalyzeControl::SendToArrangement.label()) + sc(16.0);
            l.controls.push((
                AnalyzeControl::SendToArrangement,
                Rect::new(inner.x, inner.y, bw.min(inner.width), row_h + sc(6.0)),
            ));
            let cw = button(AnalyzeControl::UseComp.label());
            let y = inner.bottom() - row_h;
            l.controls.push((
                AnalyzeControl::UseComp,
                Rect::new(inner.x, y, cw.min(inner.width), row_h),
            ));
            let kw = button(AnalyzeControl::ClearComp.label());
            l.controls.push((
                AnalyzeControl::ClearComp,
                Rect::new(
                    inner.x + cw + gap,
                    y,
                    kw.min((inner.right() - inner.x - cw - gap).max(0.0)),
                    row_h,
                ),
            ));
        }
    }
    // Nothing past its card.
    l.controls.retain(|(_, r)| r.width > 1.0);
}

/// The takes stack on the Record page's screen: a row a take, its name at
/// the left and its audio across the takes' shared time.
pub(super) fn lay_out_takes(l: &mut super::AnalyzeLayout, view: &super::AnalyzeView, area: Rect) {
    let s = l.scale;
    let sc = |v: f32| (v * s).round();
    l.takes_area = area;
    let row_h = sc(TAKE_ROW_H);
    let head = sc(TAKE_HEAD_W).min(area.width * 0.4);
    let icon = sc(20.0);
    for (i, take) in view.takes.iter().enumerate() {
        let y = area.y + i as f32 * row_h;
        if y + row_h > area.bottom() + 0.5 {
            break;
        }
        let row = Rect::new(area.x, y, area.width, row_h - sc(2.0));
        let star = Rect::new(
            row.x + sc(4.0),
            row.y + (row.height - icon) / 2.0,
            icon,
            icon,
        );
        let discard = Rect::new(
            row.x + head - icon - sc(6.0),
            row.y + (row.height - icon) / 2.0,
            icon,
            icon,
        );
        l.takes.push(AnalyzeTakeRow {
            id: take.id,
            row,
            star,
            name: Rect::new(
                star.right() + sc(4.0),
                row.y,
                (discard.x - star.right() - sc(8.0)).max(0.0),
                row.height,
            ),
            discard,
            lane: Rect::new(row.x + head, row.y, (row.width - head).max(0.0), row.height),
        });
    }
}

/// The takes' shared time across a take's lane: seconds at `x`, and back.
pub fn take_seconds_at(view: &super::AnalyzeView, lane: Rect, x: f32) -> f64 {
    let longest = takes_length(view);
    f64::from(((x - lane.x) / lane.width.max(1.0)).clamp(0.0, 1.0)) * longest
}

pub fn take_x_of(view: &super::AnalyzeView, lane: Rect, seconds: f64) -> f32 {
    let longest = takes_length(view).max(1e-6);
    lane.x + (seconds / longest) as f32 * lane.width
}

/// The longest take, in seconds: the takes' lanes are all this long.
pub fn takes_length(view: &super::AnalyzeView) -> f64 {
    view.takes
        .iter()
        .map(|t| t.frames as f64 / f64::from(t.sample_rate.max(1)))
        .fold(0.0, f64::max)
}

// =============================================================== words ===

/// The header's chip that lists every study.
pub const STUDIES: &str = "Studies";

/// The Noise card's line.
pub fn noise_text(view: &super::AnalyzeView) -> String {
    match &view.clean.denoise.noise {
        Some(noise) => format!("Noise captured: {} dB", noise.level_db.round() as i32),
        None => "No noise captured yet".to_string(),
    }
}

/// Under the Noise card's line, how to capture.
pub const NOISE_HINT: &str = "Noise tool: drag over a quiet bit";

/// The Slice points card's count.
pub fn slices_text(view: &super::AnalyzeView, state: &super::AnalyzeState) -> String {
    let n = analyze_cuts(view, state).len() + 1;
    if n == 1 {
        "1 slice".to_string()
    } else {
        format!("{n} slices")
    }
}

/// The Record card's line: the take's time, or what was lost, or why it
/// cannot record.
pub fn record_text(view: &super::AnalyzeView, state: &super::AnalyzeState) -> String {
    let Some(record) = &view.record else {
        return String::new();
    };
    if let Some(problem) = &record.problem {
        return problem.clone();
    }
    let dropped = state.meter.dropped_frames.max(record.dropped_frames);
    if dropped > 0 {
        return format!(
            "\u{26a0} {} ms lost \u{2014} the disk could not keep up",
            dropped * 1000 / u64::from(view.rate.max(1))
        );
    }
    if state.meter.recording || record.recording {
        format!(
            "Recording {}",
            super::time_text(state.meter.take_seconds, 1)
        )
    } else if record.armed {
        match record.arm {
            fontelle_types::ArmMode::OnPlay => "Waiting for the song to play".to_string(),
            fontelle_types::ArmMode::OnInput => "Waiting for sound".to_string(),
            fontelle_types::ArmMode::Now => "Recording".to_string(),
        }
    } else {
        "Not armed".to_string()
    }
}

/// What the Record page's screen says with no takes yet.
pub fn no_takes_lines(view: &super::AnalyzeView) -> [String; 2] {
    let first = match &view.record {
        Some(r) if r.armed => "Armed \u{2014} the take appears here when it ends".to_string(),
        Some(_) => "Arm, then play (or sing) \u{2014} each take appears here".to_string(),
        None => "Recording is for a mixer insert or a microphone".to_string(),
    };
    let second = match (&view.record, &view.source) {
        (None, _) => {
            "Add Analyze Musically to a mixer track, or Record \u{25b8} Record into Analyze Musically\u{2026}"
                .to_string()
        }
        (_, AnalyzeSource::Insert { .. }) => {
            "Takes come from what plays through this track; nothing goes on the arrangement until you send it"
                .to_string()
        }
        _ => "Takes come from the input; nothing goes on the arrangement until you send it"
            .to_string(),
    };
    [first, second]
}

/// A take's length, for its row.
pub fn take_length_text(take: &fontelle_types::StudyTake) -> String {
    super::time_text(take.frames as f64 / f64::from(take.sample_rate.max(1)), 1)
}

/// The words a knob draws: its read-out, or a dash with nothing to act on.
pub fn knob_text(
    knob: AnalyzeKnob,
    view: &super::AnalyzeView,
    state: &super::AnalyzeState,
) -> String {
    if let Some(typing) = &state.typing
        && typing.target == super::AnalyzeTypingTarget::Knob(knob)
    {
        return format!("{}\u{2502}", typing.text);
    }
    analyze_knob_value(knob, view, state)
        .map_or_else(|| "\u{2014}".to_string(), |v| knob.display(v))
}

/// A take's name as its row draws it (with the caret while it is renamed).
pub fn take_name_text(take: &fontelle_types::StudyTake, state: &super::AnalyzeState) -> String {
    match &state.typing {
        Some(t) if t.target == super::AnalyzeTypingTarget::TakeName(take.id) => {
            format!("{}\u{2502}", t.text)
        }
        _ => take.name.clone(),
    }
}

/// Every string the pages draw, for the shaper.
pub(super) fn page_strings(
    view: &super::AnalyzeView,
    state: &super::AnalyzeState,
    layout: &super::AnalyzeLayout,
    out: &mut Vec<(String, super::AnalyzeText)>,
) {
    use super::AnalyzeText::{Caption, Heading, Value};
    out.push((STUDIES.to_string(), Value));
    out.push((control_text(AnalyzeControl::Arm, view, state), Caption));
    out.push(("SHAPE".to_string(), Caption));
    out.push(("FIND".to_string(), Caption));
    for (card, _) in &layout.cards {
        out.push((card.label().to_string(), Heading));
    }
    for (control, _) in &layout.controls {
        match control {
            AnalyzeControl::Knob(k) => {
                out.push((k.caption().to_string(), Caption));
                out.push((knob_text(*k, view, state), Value));
            }
            other => out.push((control_text(*other, view, state), Value)),
        }
    }
    match state.page {
        super::AnalyzePage::Clean => {
            out.push((noise_text(view), Value));
            out.push((NOISE_HINT.to_string(), Caption));
            if let Some(noise) = &view.clean.denoise.noise {
                out.push((format!("{} dB", noise.level_db.round() as i32), Caption));
            }
        }
        super::AnalyzePage::Slice => {
            out.push((slices_text(view, state), Value));
            let n = analyze_cuts(view, state).len() + 1;
            for i in 1..=n.min(128) {
                out.push((i.to_string(), Caption));
            }
            for k in [24u8, 36, 48, 60, 72, 84] {
                out.push((super::key_name(k), Caption));
            }
        }
        super::AnalyzePage::Record => {
            out.push((record_text(view, state), Caption));
            for line in no_takes_lines(view) {
                out.push((line, Value));
            }
            out.push(("Takes".to_string(), Caption));
            for take in &view.takes {
                out.push((take_name_text(take, state), Value));
                out.push((take_length_text(take), Caption));
            }
            for word in ["\u{2606}", "\u{2605}", "\u{00d7}", "Comp"] {
                out.push((word.to_string(), Value));
            }
        }
        super::AnalyzePage::Notes => {}
    }
    if let Some(at) = view.rendering {
        out.push((render_job_text(at), Value));
    }
}

/// The job strip while a render runs.
pub fn render_job_text(fraction: f32) -> String {
    format!(
        "Rendering\u{2026} {} %",
        (fraction.clamp(0.0, 1.0) * 100.0).round() as i32
    )
}

/// The tips for the pages' hits.
pub(super) fn page_tip(
    hit: &super::AnalyzeHit,
    view: &super::AnalyzeView,
    state: &super::AnalyzeState,
) -> Option<String> {
    use super::AnalyzeHit as H;
    Some(match hit {
        H::Studies => {
            "Every study in this song: clips, recordings, inserts \u{2014} open one".to_string()
        }
        H::Control(c) => control_tip(*c, view, state),
        H::TrimEnd(true) => "Drag: where the audio starts (the file is not cut)".to_string(),
        H::TrimEnd(false) => "Drag: where the audio ends (the file is not cut)".to_string(),
        H::FadeEnd(true) => "Drag: the fade in's length".to_string(),
        H::FadeEnd(false) => "Drag: the fade out's length".to_string(),
        H::Marker(_) => "A marker: drag to move it, Del removes it".to_string(),
        H::TakeStar(_) => "Star the takes worth keeping".to_string(),
        H::TakeName(id) => {
            let name = view
                .takes
                .iter()
                .find(|t| t.id == *id)
                .map_or_else(String::new, |t| t.name.clone());
            if view.current_take == Some(*id) {
                format!("{name} is in the lane \u{2014} double-click to rename")
            } else {
                format!("Click: {name} into the lane \u{b7} double-click renames")
            }
        }
        H::TakeDiscard(_) => "Discard this take (Ctrl+Z brings it back)".to_string(),
        H::TakeLane(_) => "Drag across a take to use that part of it in the comp".to_string(),
        H::Takes => "Takes appear here as they finish".to_string(),
        H::Keyboard => "Where the slices land on the Sampler's keys".to_string(),
        H::Lane => match (state.page, state.tool) {
            (super::AnalyzePage::Clean, super::AnalyzeTool::Noise) => {
                "Drag over a stretch with only noise in it: it is captured as you let go"
                    .to_string()
            }
            (super::AnalyzePage::Slice, super::AnalyzeTool::Marker) => {
                "Click to add a marker; drag one to move it, Del removes it".to_string()
            }
            _ => "Drag to select a stretch \u{b7} Z zooms to it, Shift+Z shows it all".to_string(),
        },
        _ => return None,
    })
}

//! DisgustingBeat: two bars of memory with curves drawn over them
//! (`docs/disgusting-beat-plan.md`).
//!
//! > *"a gross beat like plugin … a section for time manipulation and a
//! > section for volume and you can draw in patterns for these and make
//! > curves and stuff in the editor easily and it affects the playback
//! > correctly."* — Ty, 2026-09-22
//!
//! # The whole machine, in one line
//!
//! The insert keeps the last few seconds of everything that passed through
//! it. A **time** curve says, for each point in the bar, how far back in that
//! memory to read; a **volume** curve says how loud to play it. So
//!
//! ```text
//! read(t) = write(t) − delay(t)      rate(t) = 1 − d(delay)/dt
//! ```
//!
//! and everything the effect does falls out of that: a curve falling at one
//! beat per beat holds the sound still, half that plays it an octave down,
//! steeper than it plays it backwards, and a vertical drop is a stutter's
//! repeat. That is why the window draws a 45° guide — the freeze slope is a
//! *shape*, not a setting.
//!
//! # Why the curves are not in the config
//!
//! [`DisgustingBeatConfig`] is the knobs. The drawn material is a
//! [`DisgustingBeatBank`] on the slot beside it, exactly as a notepad's pages
//! and a hosted plugin's state are, and for the same reason: an
//! [`EffectConfig`](crate::EffectConfig) is `Copy`, fixed-size and handed to
//! the audio thread every block, and twelve scenes of four lanes is 48 KB.
//!
//! Unlike a notepad's pages, the curves **do** cross to the audio thread, and
//! they have to cross while somebody is dragging one. That is what
//! [`DisgustingBeatGrid`] is for: the document's shape is a `Vec` of points,
//! because that is what an editor and an undo stack want, and the grid is the
//! same thing as a fixed-size POD that a triple buffer can carry with no
//! allocation on either end.
//!
//! # Why a lane's values are in natural units and not 0..1
//!
//! `docs/disgusting-beat-plan.md` §6 said normalised, and building it said
//! otherwise. The **time** lane's value has to have an exact zero — a fresh
//! DisgustingBeat is a wire, and "no offset" may not be a number like 0.6667
//! that a preset can miss by a rounding error. And its scale has to be *the
//! lane's own length*, because that is what makes a freeze a freeze at any
//! length: an offset falling by one lane-length over one lane is one beat per
//! beat whatever the lane is. So the time lane's value is **in lane-lengths**,
//! zero is now, −1 is a whole lane back, and the freeze is the diagonal.
//!
//! The other three follow the same principle of naming their own neutral:
//! volume is an amplitude with unity at 1, tone and pan are bipolar around 0.

use crate::PPQN;
use crate::curve::CurveShape;
use crate::effect::{MIX, all_wet, percent_param, with_mix};

/// How many scenes a DisgustingBeat holds.
///
/// Twelve because it is an octave: pointing the slot's `notes` at a channel
/// makes C to B pick a scene, so a whole kit is playable from a keyboard
/// (§4.7). Thirty-six would need three.
pub const DISGUSTING_BEAT_SCENES: usize = 12;

/// Time, volume, tone, pan — see [`DisgustingBeatLaneKind`].
pub const DISGUSTING_BEAT_LANES: usize = 4;

/// The most points one lane can hold.
///
/// Enforced in the document rather than in the window, because
/// [`DisgustingBeatGrid`] is fixed-size and a silent truncation would be a
/// curve that plays differently from the one on screen. Sixty-four is four
/// bars of sixteenths with a point to spare.
pub const DISGUSTING_BEAT_POINTS: usize = 64;

/// Seconds of audio a DisgustingBeat remembers, per channel.
///
/// 4.6 MB stereo at 48 kHz. A constant rather than something derived from the
/// tempo because `prepare` is the only place that may allocate (INVARIANT 1)
/// and the tempo changes afterwards; twelve seconds is four bars of 4/4 at
/// 80 bpm, or two bars at 40. Past that the read clamps to what is actually
/// in the buffer and the window says so, which is the same rule that covers
/// the first bar after a seek.
pub const DISGUSTING_BEAT_MEMORY_SECONDS: f32 = 12.0;

/// The four things a DisgustingBeat scene draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum DisgustingBeatLaneKind {
    /// How far back in the memory to read, **in lane-lengths**: 0 is now, −1
    /// is a whole lane back. A segment falling at one lane-length per lane is
    /// a freeze.
    Time,
    /// An amplitude. 1 is unity, 0 is silence.
    Volume,
    /// Bipolar: below 0 a low-pass sweeping down, above it a high-pass
    /// sweeping up, 0 is no filter at all.
    Tone,
    /// Bipolar: −1 hard left, +1 hard right, 0 is where the signal already
    /// was.
    Pan,
}

impl DisgustingBeatLaneKind {
    pub const ALL: [Self; DISGUSTING_BEAT_LANES] =
        [Self::Time, Self::Volume, Self::Tone, Self::Pan];

    pub fn label(self) -> &'static str {
        match self {
            Self::Time => "Time",
            Self::Volume => "Volume",
            Self::Tone => "Tone",
            Self::Pan => "Pan",
        }
    }

    /// Its place in a scene's lanes.
    pub fn index(self) -> usize {
        match self {
            Self::Time => 0,
            Self::Volume => 1,
            Self::Tone => 2,
            Self::Pan => 3,
        }
    }

    /// What this lane means when nothing is drawn on it — the value that
    /// makes it a wire.
    pub fn neutral(self) -> f64 {
        match self {
            Self::Volume => 1.0,
            _ => 0.0,
        }
    }

    /// The lowest and highest value a point on this lane may take.
    ///
    /// The time lane reaches one lane-length either way, which is one
    /// freeze's worth. A slope *steeper* than the freeze — a reverse — is
    /// drawn by making the segment **shorter**, not the value bigger: half a
    /// lane falling by one lane-length is `dv/dp = −2`, which is reverse at
    /// the song's own speed. Widening this instead would put the freeze at
    /// 26° on the grid and cost the one thing about the window that teaches
    /// itself.
    pub fn range(self) -> (f64, f64) {
        match self {
            Self::Volume => (0.0, 1.0),
            _ => (-1.0, 1.0),
        }
    }

    /// Whether this lane is drawn by default. Time and volume are what was
    /// asked for; tone and pan are off until somebody wants them, so a fresh
    /// DisgustingBeat is a wire and its window is two lanes rather than four.
    pub fn on_by_default(self) -> bool {
        matches!(self, Self::Time | Self::Volume)
    }
}

/// How long one lane's pattern is before it comes round again.
///
/// Bars follow the project's metre rather than always meaning four beats, so
/// a pattern written in 4/4 is still a bar in 3/4. The sub-bar positions are
/// in beats for the same reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum DisgustingBeatLength {
    Beat,
    TwoBeats,
    ThreeBeats,
    Bar,
    TwoBars,
    FourBars,
}

impl DisgustingBeatLength {
    pub const ALL: [Self; 6] = [
        Self::Beat,
        Self::TwoBeats,
        Self::ThreeBeats,
        Self::Bar,
        Self::TwoBars,
        Self::FourBars,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Beat => "1 beat",
            Self::TwoBeats => "2 beats",
            Self::ThreeBeats => "3 beats",
            Self::Bar => "1 bar",
            Self::TwoBars => "2 bars",
            Self::FourBars => "4 bars",
        }
    }

    /// How many beats long this is in a bar of `beats_per_bar`.
    pub fn beats(self, beats_per_bar: u32) -> f64 {
        let bar = beats_per_bar.max(1) as f64;
        match self {
            Self::Beat => 1.0,
            Self::TwoBeats => 2.0,
            Self::ThreeBeats => 3.0,
            Self::Bar => bar,
            Self::TwoBars => bar * 2.0,
            Self::FourBars => bar * 4.0,
        }
    }

    /// And in ticks, which is what the phase is walked in (INVARIANT 5).
    pub fn ticks(self, beats_per_bar: u32) -> f64 {
        self.beats(beats_per_bar) * PPQN as f64
    }
}

/// One drawn point: where it is along the lane, what the lane is worth there,
/// and how the segment *after* it gets to the next one.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DisgustingBeatPoint {
    /// 0..1 across the lane's own length.
    pub at: f64,
    /// In the lane kind's own unit — see [`DisgustingBeatLaneKind`].
    pub value: f64,
    /// Shape of the segment *following* this point, as an automation lane
    /// means it.
    pub curve: CurveShape,
    pub tension: f32,
}

impl DisgustingBeatPoint {
    pub fn new(at: f64, value: f64, curve: CurveShape) -> Self {
        Self {
            at,
            value,
            curve,
            tension: 0.0,
        }
    }
}

/// One lane of one scene.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DisgustingBeatLane {
    pub length: DisgustingBeatLength,
    pub on: bool,
    /// **Always at least one**, sorted by `at`, no two sharing one: an empty
    /// lane would have to mean something and the two candidates (silence, and
    /// the wire) are both wrong half the time.
    pub points: Vec<DisgustingBeatPoint>,
}

impl DisgustingBeatLane {
    /// A lane with nothing drawn on it: one point at its neutral value.
    pub fn flat(kind: DisgustingBeatLaneKind) -> Self {
        Self {
            length: DisgustingBeatLength::Bar,
            on: kind.on_by_default(),
            points: vec![DisgustingBeatPoint::new(
                0.0,
                kind.neutral(),
                CurveShape::Linear,
            )],
        }
    }

    /// Puts the lane back in the shape everything else assumes: sorted, no
    /// two points at one place, at least one point, every value in range.
    ///
    /// Called by the edit algebra after every change rather than trusted to
    /// each edit, because "the points are sorted" is the property the RT
    /// evaluator and the window both rest on and one place to enforce it is
    /// one place to get it right.
    pub fn tidy(&mut self, kind: DisgustingBeatLaneKind) {
        let (low, high) = kind.range();
        for point in &mut self.points {
            point.at = point.at.clamp(0.0, 1.0);
            point.value = point.value.clamp(low, high);
            if !point.tension.is_finite() {
                point.tension = 0.0;
            }
        }
        self.points
            .sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
        // Two points at one place is how a *vertical* segment would be
        // spelled, and it is not how this spells one — a `Stepped` point is.
        // The later one wins, which is what dragging one onto another means.
        self.points.dedup_by(|b, a| {
            (b.at - a.at).abs() < 1e-9 && {
                *a = *b;
                true
            }
        });
        self.points.truncate(DISGUSTING_BEAT_POINTS);
        if self.points.is_empty() {
            self.points.push(DisgustingBeatPoint::new(
                0.0,
                kind.neutral(),
                CurveShape::Linear,
            ));
        }
    }

    /// What this lane is worth `phase` of the way round it — the **same**
    /// function the audio thread reads, so the picture and the sound cannot
    /// disagree.
    pub fn value_at(&self, phase: f64, kind: DisgustingBeatLaneKind) -> f64 {
        if !self.on {
            return kind.neutral();
        }
        curve_at(&self.points, phase, kind.neutral())
    }

    /// Whether this lane would change the sound — a lane at its neutral value
    /// the whole way round is a wire however many points draw it.
    pub fn is_flat(&self, kind: DisgustingBeatLaneKind) -> bool {
        self.points
            .iter()
            .all(|p| (p.value - kind.neutral()).abs() < 1e-9)
    }
}

/// One scene: four lanes and a name.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DisgustingBeatScene {
    /// What the scene chip says. Empty means the chip shows its number, which
    /// is what most scenes will stay as — a name that has to be filled in is
    /// a name most things never get (`NotepadPages::caption` argued this
    /// first).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub lanes: Vec<DisgustingBeatLane>,
}

impl DisgustingBeatScene {
    pub fn flat() -> Self {
        Self {
            name: String::new(),
            lanes: DisgustingBeatLaneKind::ALL
                .iter()
                .map(|k| DisgustingBeatLane::flat(*k))
                .collect(),
        }
    }

    pub fn lane(&self, kind: DisgustingBeatLaneKind) -> Option<&DisgustingBeatLane> {
        self.lanes.get(kind.index())
    }

    pub fn lane_mut(&mut self, kind: DisgustingBeatLaneKind) -> Option<&mut DisgustingBeatLane> {
        self.lanes.get_mut(kind.index())
    }

    /// Whether every lane on it is a wire.
    pub fn is_flat(&self) -> bool {
        DisgustingBeatLaneKind::ALL
            .iter()
            .all(|kind| match self.lane(*kind) {
                Some(lane) => !lane.on || lane.is_flat(*kind),
                None => true,
            })
    }
}

/// The twelve scenes an insert holds — what a preset for this device saves,
/// and what lives on the slot beside the config.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DisgustingBeatBank {
    pub scenes: Vec<DisgustingBeatScene>,
}

impl DisgustingBeatBank {
    /// Twelve flat scenes: a DisgustingBeat somebody has just added and not
    /// yet drawn in, which is a wire.
    pub fn new() -> Self {
        Self {
            scenes: (0..DISGUSTING_BEAT_SCENES)
                .map(|_| DisgustingBeatScene::flat())
                .collect(),
        }
    }

    /// A bank read out of a file may be short or long — `fill` is what makes
    /// the rest of the program able to index twelve without asking.
    pub fn fill(&mut self) {
        self.scenes.truncate(DISGUSTING_BEAT_SCENES);
        while self.scenes.len() < DISGUSTING_BEAT_SCENES {
            self.scenes.push(DisgustingBeatScene::flat());
        }
        for scene in &mut self.scenes {
            scene.lanes.truncate(DISGUSTING_BEAT_LANES);
            while scene.lanes.len() < DISGUSTING_BEAT_LANES {
                let kind = DisgustingBeatLaneKind::ALL[scene.lanes.len()];
                scene.lanes.push(DisgustingBeatLane::flat(kind));
            }
            for (index, lane) in scene.lanes.iter_mut().enumerate() {
                lane.tidy(DisgustingBeatLaneKind::ALL[index]);
            }
        }
    }

    pub fn scene(&self, index: usize) -> Option<&DisgustingBeatScene> {
        self.scenes.get(index)
    }
}

impl Default for DisgustingBeatBank {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------- the edits

/// One thing the editor does to a bank (`docs/disgusting-beat-plan.md` §6).
///
/// [`DisgustingBeatBank::apply`] does it and hands back **the inverse of what
/// it just did**, so `EditLapse` in `fontelle-model` has nothing to work out
/// and no second copy of these rules — the shape
/// [`NotepadEdit`](crate::NotepadEdit) and
/// [`WavetableEdit`](crate::WavetableEdit) both take, for the same reason.
#[derive(Debug, Clone, PartialEq)]
pub enum DisgustingBeatEdit {
    AddPoint {
        scene: usize,
        lane: usize,
        point: DisgustingBeatPoint,
    },
    MovePoint {
        scene: usize,
        lane: usize,
        index: usize,
        to: (f64, f64),
    },
    RemovePoint {
        scene: usize,
        lane: usize,
        index: usize,
    },
    SetCurve {
        scene: usize,
        lane: usize,
        index: usize,
        curve: CurveShape,
    },
    SetTension {
        scene: usize,
        lane: usize,
        index: usize,
        tension: f32,
    },
    /// Free-hand: every point under the stroke is replaced by the line from
    /// `from` to `to`. A drag is a run of these, one per pointer step, which
    /// the command coalesces into one undo entry.
    Draw {
        scene: usize,
        lane: usize,
        from: (f64, f64),
        to: (f64, f64),
    },
    SetLength {
        scene: usize,
        lane: usize,
        length: DisgustingBeatLength,
    },
    SetLaneOn {
        scene: usize,
        lane: usize,
        on: bool,
    },
    /// The lane back to its neutral value — one point, not none.
    ClearLane {
        scene: usize,
        lane: usize,
    },
    /// Every point of one lane at once: what a fill, a paste and an undo of
    /// either are made of.
    SetLane {
        scene: usize,
        lane: usize,
        points: Vec<DisgustingBeatPoint>,
    },
    CopyScene {
        from: usize,
        to: usize,
    },
    /// The whole scene at once — `CopyScene`'s inverse, and a paste.
    SetScene {
        scene: usize,
        value: Box<DisgustingBeatScene>,
    },
    RenameScene {
        scene: usize,
        name: String,
    },
}

impl DisgustingBeatEdit {
    /// What the history calls it.
    pub fn label(&self) -> &'static str {
        match self {
            Self::AddPoint { .. } => "add a point",
            Self::MovePoint { .. } => "move a point",
            Self::RemovePoint { .. } => "remove a point",
            Self::SetCurve { .. } => "change a curve",
            Self::SetTension { .. } => "bend a curve",
            Self::Draw { .. } => "draw",
            Self::SetLength { .. } => "set a lane's length",
            Self::SetLaneOn { .. } => "switch a lane",
            Self::ClearLane { .. } | Self::SetLane { .. } => "change a lane",
            Self::CopyScene { .. } | Self::SetScene { .. } => "change a scene",
            Self::RenameScene { .. } => "rename a scene",
        }
    }

    /// Which lane it is about, so two of them can be told apart when the
    /// command decides whether to coalesce.
    pub fn target(&self) -> Option<(usize, usize)> {
        match self {
            Self::AddPoint { scene, lane, .. }
            | Self::MovePoint { scene, lane, .. }
            | Self::RemovePoint { scene, lane, .. }
            | Self::SetCurve { scene, lane, .. }
            | Self::SetTension { scene, lane, .. }
            | Self::Draw { scene, lane, .. }
            | Self::SetLength { scene, lane, .. }
            | Self::SetLaneOn { scene, lane, .. }
            | Self::ClearLane { scene, lane }
            | Self::SetLane { scene, lane, .. } => Some((*scene, *lane)),
            _ => None,
        }
    }

    /// What it costs the history, for the memory budget.
    pub fn memory_cost(&self) -> usize {
        std::mem::size_of::<Self>()
            + match self {
                Self::SetLane { points, .. } => {
                    points.len() * std::mem::size_of::<DisgustingBeatPoint>()
                }
                Self::SetScene { value, .. } => value
                    .lanes
                    .iter()
                    .map(|lane| lane.points.len() * std::mem::size_of::<DisgustingBeatPoint>())
                    .sum(),
                Self::RenameScene { name, .. } => name.len(),
                _ => 0,
            }
    }
}

impl DisgustingBeatBank {
    /// Does one edit and returns **the edit that puts it back**, or `None`
    /// when it changes nothing.
    ///
    /// A refused edit is not an entry in the history, so Ctrl+Z never walks
    /// back through edits that never happened — the rule
    /// [`NotepadEdit`](crate::NotepadEdit) established.
    pub fn apply(&mut self, edit: &DisgustingBeatEdit) -> Option<DisgustingBeatEdit> {
        match edit {
            DisgustingBeatEdit::AddPoint { scene, lane, point } => {
                let kind = *DisgustingBeatLaneKind::ALL.get(*lane)?;
                let target = self.scenes.get_mut(*scene)?.lanes.get_mut(*lane)?;
                if target.points.len() >= DISGUSTING_BEAT_POINTS {
                    return None;
                }
                let before = target.points.clone();
                target.points.push(*point);
                target.tidy(kind);
                if target.points == before {
                    return None;
                }
                Some(DisgustingBeatEdit::SetLane {
                    scene: *scene,
                    lane: *lane,
                    points: before,
                })
            }
            DisgustingBeatEdit::MovePoint {
                scene,
                lane,
                index,
                to,
            } => {
                let kind = *DisgustingBeatLaneKind::ALL.get(*lane)?;
                let target = self.scenes.get_mut(*scene)?.lanes.get_mut(*lane)?;
                let before = target.points.clone();
                let point = target.points.get_mut(*index)?;
                point.at = to.0;
                point.value = to.1;
                target.tidy(kind);
                if target.points == before {
                    return None;
                }
                Some(DisgustingBeatEdit::SetLane {
                    scene: *scene,
                    lane: *lane,
                    points: before,
                })
            }
            DisgustingBeatEdit::RemovePoint { scene, lane, index } => {
                let kind = *DisgustingBeatLaneKind::ALL.get(*lane)?;
                let target = self.scenes.get_mut(*scene)?.lanes.get_mut(*lane)?;
                if target.points.len() <= 1 || *index >= target.points.len() {
                    // A lane always has a point. See `DisgustingBeatLane`.
                    return None;
                }
                let before = target.points.clone();
                target.points.remove(*index);
                target.tidy(kind);
                Some(DisgustingBeatEdit::SetLane {
                    scene: *scene,
                    lane: *lane,
                    points: before,
                })
            }
            DisgustingBeatEdit::SetCurve {
                scene,
                lane,
                index,
                curve,
            } => {
                let point = self
                    .scenes
                    .get_mut(*scene)?
                    .lanes
                    .get_mut(*lane)?
                    .points
                    .get_mut(*index)?;
                if point.curve == *curve {
                    return None;
                }
                let was = point.curve;
                point.curve = *curve;
                Some(DisgustingBeatEdit::SetCurve {
                    scene: *scene,
                    lane: *lane,
                    index: *index,
                    curve: was,
                })
            }
            DisgustingBeatEdit::SetTension {
                scene,
                lane,
                index,
                tension,
            } => {
                let point = self
                    .scenes
                    .get_mut(*scene)?
                    .lanes
                    .get_mut(*lane)?
                    .points
                    .get_mut(*index)?;
                let asked = if tension.is_finite() {
                    tension.clamp(-1.0, 1.0)
                } else {
                    0.0
                };
                if (point.tension - asked).abs() < 1e-9 {
                    return None;
                }
                let was = point.tension;
                point.tension = asked;
                Some(DisgustingBeatEdit::SetTension {
                    scene: *scene,
                    lane: *lane,
                    index: *index,
                    tension: was,
                })
            }
            DisgustingBeatEdit::Draw {
                scene,
                lane,
                from,
                to,
            } => {
                let kind = *DisgustingBeatLaneKind::ALL.get(*lane)?;
                let target = self.scenes.get_mut(*scene)?.lanes.get_mut(*lane)?;
                let before = target.points.clone();
                let (low, high) = if from.0 <= to.0 {
                    (*from, *to)
                } else {
                    (*to, *from)
                };
                // Everything the stroke passed over goes; the two ends of the
                // stroke stay. A free-hand line is a run of these, so the
                // points it leaves behind are its own path.
                target
                    .points
                    .retain(|p| p.at < low.0 - 1e-9 || p.at > high.0 + 1e-9);
                target.points.push(DisgustingBeatPoint::new(
                    low.0,
                    low.1,
                    before
                        .iter()
                        .find(|p| p.at <= low.0)
                        .map_or(CurveShape::Linear, |p| p.curve),
                ));
                if (high.0 - low.0).abs() > 1e-9 {
                    target.points.push(DisgustingBeatPoint::new(
                        high.0,
                        high.1,
                        CurveShape::Linear,
                    ));
                }
                target.tidy(kind);
                if target.points == before {
                    return None;
                }
                Some(DisgustingBeatEdit::SetLane {
                    scene: *scene,
                    lane: *lane,
                    points: before,
                })
            }
            DisgustingBeatEdit::SetLength {
                scene,
                lane,
                length,
            } => {
                let target = self.scenes.get_mut(*scene)?.lanes.get_mut(*lane)?;
                if target.length == *length {
                    return None;
                }
                let was = target.length;
                target.length = *length;
                Some(DisgustingBeatEdit::SetLength {
                    scene: *scene,
                    lane: *lane,
                    length: was,
                })
            }
            DisgustingBeatEdit::SetLaneOn { scene, lane, on } => {
                let target = self.scenes.get_mut(*scene)?.lanes.get_mut(*lane)?;
                if target.on == *on {
                    return None;
                }
                target.on = *on;
                Some(DisgustingBeatEdit::SetLaneOn {
                    scene: *scene,
                    lane: *lane,
                    on: !*on,
                })
            }
            DisgustingBeatEdit::ClearLane { scene, lane } => {
                let kind = *DisgustingBeatLaneKind::ALL.get(*lane)?;
                let target = self.scenes.get_mut(*scene)?.lanes.get_mut(*lane)?;
                let before = target.points.clone();
                target.points = vec![DisgustingBeatPoint::new(
                    0.0,
                    kind.neutral(),
                    CurveShape::Linear,
                )];
                if target.points == before {
                    return None;
                }
                Some(DisgustingBeatEdit::SetLane {
                    scene: *scene,
                    lane: *lane,
                    points: before,
                })
            }
            DisgustingBeatEdit::SetLane {
                scene,
                lane,
                points,
            } => {
                let kind = *DisgustingBeatLaneKind::ALL.get(*lane)?;
                let target = self.scenes.get_mut(*scene)?.lanes.get_mut(*lane)?;
                let before = target.points.clone();
                target.points = points.clone();
                target.tidy(kind);
                if target.points == before {
                    return None;
                }
                Some(DisgustingBeatEdit::SetLane {
                    scene: *scene,
                    lane: *lane,
                    points: before,
                })
            }
            DisgustingBeatEdit::CopyScene { from, to } => {
                if from == to {
                    return None;
                }
                let source = self.scenes.get(*from)?.clone();
                let before = self.scenes.get(*to)?.clone();
                if source == before {
                    return None;
                }
                *self.scenes.get_mut(*to)? = source;
                Some(DisgustingBeatEdit::SetScene {
                    scene: *to,
                    value: Box::new(before),
                })
            }
            DisgustingBeatEdit::SetScene { scene, value } => {
                let before = self.scenes.get(*scene)?.clone();
                if before == **value {
                    return None;
                }
                *self.scenes.get_mut(*scene)? = (**value).clone();
                Some(DisgustingBeatEdit::SetScene {
                    scene: *scene,
                    value: Box::new(before),
                })
            }
            DisgustingBeatEdit::RenameScene { scene, name } => {
                let target = self.scenes.get_mut(*scene)?;
                if target.name == *name {
                    return None;
                }
                let was = std::mem::replace(&mut target.name, name.clone());
                Some(DisgustingBeatEdit::RenameScene {
                    scene: *scene,
                    name: was,
                })
            }
        }
    }
}

// ------------------------------------------------------- the realised grid

/// One point as the audio thread reads it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RtPoint {
    pub at: f32,
    pub value: f32,
    pub curve: CurveShape,
    pub tension: f32,
}

impl RtPoint {
    const NOTHING: Self = Self {
        at: 0.0,
        value: 0.0,
        curve: CurveShape::Linear,
        tension: 0.0,
    };
}

/// One lane as the audio thread reads it: a fixed array and a count.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RtLane {
    pub points: [RtPoint; DISGUSTING_BEAT_POINTS],
    pub len: u8,
    pub length: DisgustingBeatLength,
    pub on: bool,
}

impl RtLane {
    pub const fn empty() -> Self {
        Self {
            points: [RtPoint::NOTHING; DISGUSTING_BEAT_POINTS],
            len: 0,
            length: DisgustingBeatLength::Bar,
            on: false,
        }
    }

    /// **RT.** What this lane is worth `phase` of the way round it.
    ///
    /// `neutral` is the lane kind's own (a lane that is off, or has nothing
    /// on it, is a wire). The body is [`curve_at`], which the *window* calls
    /// too over the document's own points: two evaluators would be a place
    /// for the picture and the sound to disagree, and a curve you drew that
    /// plays as something else is the worst bug this effect could have.
    pub fn value_at(&self, phase: f64, neutral: f64) -> f64 {
        if !self.on {
            return neutral;
        }
        curve_at(&self.points[..self.len as usize], phase, neutral)
    }
}

/// What a point on a drawn curve is, whichever side of the triple buffer it
/// is on.
///
/// The document holds [`DisgustingBeatPoint`] and the audio thread holds
/// [`RtPoint`] — the same four numbers at two precisions — and [`curve_at`] is
/// written once over both.
pub trait CurvePoint {
    fn at(&self) -> f64;
    fn value(&self) -> f64;
    fn curve(&self) -> CurveShape;
    fn tension(&self) -> f32;
}

impl CurvePoint for RtPoint {
    fn at(&self) -> f64 {
        self.at as f64
    }
    fn value(&self) -> f64 {
        self.value as f64
    }
    fn curve(&self) -> CurveShape {
        self.curve
    }
    fn tension(&self) -> f32 {
        self.tension
    }
}

impl CurvePoint for DisgustingBeatPoint {
    fn at(&self) -> f64 {
        self.at
    }
    fn value(&self) -> f64 {
        self.value
    }
    fn curve(&self) -> CurveShape {
        self.curve
    }
    fn tension(&self) -> f32 {
        self.tension
    }
}

/// What a lane drawn with `points` is worth `phase` of the way round it.
///
/// **RT-safe**: a binary search and one eased interpolation, no allocation.
/// Six compares at sixty-four points, no state to reset on a seek, and it is
/// the choice [`bpm_at`](crate::CompiledTimeline::bpm_at) already made for
/// the same reason.
///
/// The lane is a **loop**, so the segment after the last point runs round to
/// the first one. A `Hold` point stops the lane where it sits until the loop
/// comes round again, which falls out of that: everything after the freeze is
/// cut off, so the wrapping segment carries the held point's own shape.
pub fn curve_at<P: CurvePoint>(points: &[P], phase: f64, neutral: f64) -> f64 {
    if points.is_empty() {
        return neutral;
    }
    // A `Hold` ends the lane where it sits — the whole difference between the
    // two flat shapes, and without it `Stepped` and `Hold` would be one shape
    // with two names. `curve_value` truncates an automation lane the same way.
    let points = match points.iter().position(|p| p.curve().freezes()) {
        Some(freeze) => &points[..=freeze],
        None => points,
    };
    let len = points.len();
    if len == 1 {
        return points[0].value();
    }
    let phase = phase.rem_euclid(1.0);
    let index = points.partition_point(|p| p.at() <= phase);
    let (from, to, from_at, to_at) = if index == 0 {
        // Before the first point: the segment that wrapped round.
        let last = &points[len - 1];
        (last, &points[0], last.at() - 1.0, points[0].at())
    } else if index == len {
        let last = &points[len - 1];
        (last, &points[0], last.at(), points[0].at() + 1.0)
    } else {
        (
            &points[index - 1],
            &points[index],
            points[index - 1].at(),
            points[index].at(),
        )
    };
    if from.curve().holds() {
        return from.value();
    }
    let span = (to_at - from_at).max(1e-9);
    let t = from.curve().eased((phase - from_at) / span, from.tension());
    from.value() + (to.value() - from.value()) * t
}

/// One scene as the audio thread reads it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RtScene {
    pub lanes: [RtLane; DISGUSTING_BEAT_LANES],
}

impl RtScene {
    pub const fn empty() -> Self {
        Self {
            lanes: [RtLane::empty(); DISGUSTING_BEAT_LANES],
        }
    }
}

/// The whole bank, realised: what crosses to the audio thread on its own
/// channel (`docs/disgusting-beat-plan.md` §3.3).
///
/// 48 KB, `Copy`, no allocation at either end. Read by reference on the RT
/// side — a copy per block per insert would be 19 MB/s of memcpy for nothing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DisgustingBeatGrid {
    pub scenes: [RtScene; DISGUSTING_BEAT_SCENES],
}

impl DisgustingBeatGrid {
    /// Every lane off: what an insert with no bank reads, and a wire.
    pub const fn empty() -> Self {
        Self {
            scenes: [RtScene::empty(); DISGUSTING_BEAT_SCENES],
        }
    }

    pub fn scene(&self, index: usize) -> &RtScene {
        &self.scenes[index.min(DISGUSTING_BEAT_SCENES - 1)]
    }

    pub fn lane(&self, scene: usize, kind: DisgustingBeatLaneKind) -> &RtLane {
        &self.scene(scene).lanes[kind.index()]
    }
}

impl Default for DisgustingBeatGrid {
    fn default() -> Self {
        Self::empty()
    }
}

impl From<&DisgustingBeatBank> for DisgustingBeatGrid {
    /// The document's shape, clamped to what the audio thread can hold.
    ///
    /// Clamping rather than refusing, because this runs on every publish and
    /// a bank out of a file from a later version of the program is a thing
    /// that happens; the *editor* is where the cap is a rule with a message.
    fn from(bank: &DisgustingBeatBank) -> Self {
        let mut grid = Self::empty();
        for (scene_index, scene) in bank.scenes.iter().take(DISGUSTING_BEAT_SCENES).enumerate() {
            for (lane_index, lane) in scene.lanes.iter().take(DISGUSTING_BEAT_LANES).enumerate() {
                let out = &mut grid.scenes[scene_index].lanes[lane_index];
                out.length = lane.length;
                out.on = lane.on;
                let mut count = 0;
                for point in lane.points.iter().take(DISGUSTING_BEAT_POINTS) {
                    out.points[count] = RtPoint {
                        at: point.at as f32,
                        value: point.value as f32,
                        curve: point.curve,
                        tension: point.tension,
                    };
                    count += 1;
                }
                out.len = count as u8;
            }
        }
        grid
    }
}

// ------------------------------------------------------------- the choosers

/// How fast the whole pattern runs against the song — half-time and
/// double-time as one automatable knob.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DisgustingBeatRate {
    Quarter,
    Half,
    One,
    Two,
    Four,
}

impl DisgustingBeatRate {
    pub const ALL: [Self; 5] = [Self::Quarter, Self::Half, Self::One, Self::Two, Self::Four];

    pub fn label(self) -> &'static str {
        match self {
            Self::Quarter => "1/4",
            Self::Half => "1/2",
            Self::One => "1x",
            Self::Two => "2x",
            Self::Four => "4x",
        }
    }

    /// What every lane's length is multiplied by. A *slower* rate is a
    /// *longer* lane, which is why this is the reciprocal of what the chip
    /// says.
    pub fn stretch(self) -> f64 {
        match self {
            Self::Quarter => 4.0,
            Self::Half => 2.0,
            Self::One => 1.0,
            Self::Two => 0.5,
            Self::Four => 0.25,
        }
    }
}

/// What the pattern's phase is measured from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DisgustingBeatSync {
    /// The song's own position: bar 37 behaves like bar 1. The default, and
    /// the whole reason the tick reaches the audio thread.
    Song,
    /// The last note on the slot's notes channel — the pattern becomes
    /// something you play.
    Retrigger,
    /// Wherever playback began, which is what somebody jamming over a loop
    /// that does not start on bar 1 actually wants.
    Free,
}

impl DisgustingBeatSync {
    pub const ALL: [Self; 3] = [Self::Song, Self::Retrigger, Self::Free];

    pub fn label(self) -> &'static str {
        match self {
            Self::Song => "song",
            Self::Retrigger => "retrigger",
            Self::Free => "free",
        }
    }
}

/// What a note arriving on the slot's notes channel does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DisgustingBeatNotes {
    Off,
    /// The pitch class picks the scene: C is the first, B is the twelfth, in
    /// any octave.
    Select,
    /// And resets the phase as well.
    Retrigger,
}

impl DisgustingBeatNotes {
    pub const ALL: [Self; 3] = [Self::Off, Self::Select, Self::Retrigger];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Select => "select",
            Self::Retrigger => "retrigger",
        }
    }
}

/// What the moving read head costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DisgustingBeatQuality {
    /// Four-point Hermite.
    Normal,
    /// Eight-point windowed sinc — worth it where the curve speeds the sound
    /// up, which is where a moving read aliases.
    High,
}

impl DisgustingBeatQuality {
    pub const ALL: [Self; 2] = [Self::Normal, Self::High];

    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::High => "high",
        }
    }
}

/// How far ahead the effect is allowed to read — the half of the time axis
/// the thing this is modelled on cannot reach (§4.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DisgustingBeatLook {
    Off,
    Beat,
    Bar,
}

impl DisgustingBeatLook {
    pub const ALL: [Self; 3] = [Self::Off, Self::Beat, Self::Bar];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Beat => "1 beat",
            Self::Bar => "1 bar",
        }
    }

    /// How many beats of latency this costs.
    pub fn beats(self, beats_per_bar: u32) -> f64 {
        match self {
            Self::Off => 0.0,
            Self::Beat => 1.0,
            Self::Bar => beats_per_bar.max(1) as f64,
        }
    }
}

/// What the tone lane does with its two halves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DisgustingBeatTone {
    /// Down is a low-pass closing, up is a high-pass opening.
    LowHigh,
    /// Both at once: a band-pass narrowing away from the centre.
    Band,
    /// A shelf pair that lifts one end as it drops the other.
    Tilt,
}

impl DisgustingBeatTone {
    pub const ALL: [Self; 3] = [Self::LowHigh, Self::Band, Self::Tilt];

    pub fn label(self) -> &'static str {
        match self {
            Self::LowHigh => "low/high",
            Self::Band => "band",
            Self::Tilt => "tilt",
        }
    }
}

// -------------------------------------------------------------- the config

/// The knobs. The curves are a [`DisgustingBeatBank`] beside it.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DisgustingBeatConfig {
    /// Which scene is playing, 0-based. A parameter, so it is automatable,
    /// MIDI-learnable and recordable — which is the workflow this effect is
    /// actually used with.
    pub scene: u8,
    pub rate: DisgustingBeatRate,
    pub sync: DisgustingBeatSync,
    /// 0..1. Warps the phase inside each 1/8 so the offbeat is late.
    pub swing: f32,
    pub notes: DisgustingBeatNotes,
    /// 0..1. Scales every time offset — the one knob that dials the whole
    /// effect in, and what makes a preset usable gently.
    pub time: f32,
    pub smooth_ms: f32,
    pub quality: DisgustingBeatQuality,
    pub look: DisgustingBeatLook,
    /// 0..1.
    pub volume: f32,
    /// 0..1. Zero bypasses the filter entirely rather than running it flat.
    pub tone: f32,
    pub tone_mode: DisgustingBeatTone,
    /// Octaves the tone lane sweeps over.
    pub tone_range: f32,
    /// 0..1.
    pub pan: f32,
    pub output_db: f32,
    #[serde(default = "all_wet")]
    pub mix: f32,
}

impl DisgustingBeatConfig {
    /// A fresh DisgustingBeat. The depths are at full and the bank beside it
    /// is flat, so it is a wire until somebody draws something — which is the
    /// right way round: the depth knobs are there to dial a *preset* back, and
    /// a preset is what somebody will load ten seconds after adding one.
    pub fn new() -> Self {
        Self {
            scene: 0,
            rate: DisgustingBeatRate::One,
            sync: DisgustingBeatSync::Song,
            swing: 0.0,
            notes: DisgustingBeatNotes::Select,
            time: 1.0,
            smooth_ms: 12.0,
            quality: DisgustingBeatQuality::Normal,
            look: DisgustingBeatLook::Off,
            volume: 1.0,
            tone: 0.0,
            tone_mode: DisgustingBeatTone::LowHigh,
            tone_range: 3.0,
            pan: 0.0,
            output_db: 0.0,
            mix: 1.0,
        }
    }

    pub(crate) fn get(&self, id: &str) -> Option<f32> {
        Some(match id {
            MIX => self.mix * 100.0,
            "scene" => self.scene as f32 + 1.0,
            "rate" => position_of(&DisgustingBeatRate::ALL, self.rate),
            "sync" => position_of(&DisgustingBeatSync::ALL, self.sync),
            "swing" => self.swing * 100.0,
            "notes" => position_of(&DisgustingBeatNotes::ALL, self.notes),
            "time" => self.time * 100.0,
            "smooth" => self.smooth_ms,
            "quality" => position_of(&DisgustingBeatQuality::ALL, self.quality),
            "look" => position_of(&DisgustingBeatLook::ALL, self.look),
            "volume" => self.volume * 100.0,
            "tone" => self.tone * 100.0,
            "tone_mode" => position_of(&DisgustingBeatTone::ALL, self.tone_mode),
            "tone_range" => self.tone_range,
            "pan" => self.pan * 100.0,
            "out" => self.output_db,
            _ => return None,
        })
    }

    pub(crate) fn set(&mut self, id: &str, value: f32) {
        match id {
            MIX => self.mix = value / 100.0,
            "scene" => self.scene = (value.round() as i32 - 1).clamp(0, 11) as u8,
            "rate" => self.rate = at_position(&DisgustingBeatRate::ALL, value),
            "sync" => self.sync = at_position(&DisgustingBeatSync::ALL, value),
            "swing" => self.swing = value / 100.0,
            "notes" => self.notes = at_position(&DisgustingBeatNotes::ALL, value),
            "time" => self.time = value / 100.0,
            "smooth" => self.smooth_ms = value,
            "quality" => self.quality = at_position(&DisgustingBeatQuality::ALL, value),
            "look" => self.look = at_position(&DisgustingBeatLook::ALL, value),
            "volume" => self.volume = value / 100.0,
            "tone" => self.tone = value / 100.0,
            "tone_mode" => self.tone_mode = at_position(&DisgustingBeatTone::ALL, value),
            "tone_range" => self.tone_range = value,
            "pan" => self.pan = value / 100.0,
            "out" => self.output_db = value,
            _ => {}
        }
    }
}

impl Default for DisgustingBeatConfig {
    fn default() -> Self {
        Self::new()
    }
}

fn position_of<T: PartialEq + Copy>(all: &[T], value: T) -> f32 {
    all.iter().position(|v| *v == value).unwrap_or(0) as f32
}

fn at_position<T: Copy>(all: &[T], value: f32) -> T {
    all[(value.round().max(0.0) as usize).min(all.len() - 1)]
}

const SCENE_NAMES: [&str; DISGUSTING_BEAT_SCENES] = [
    "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12",
];
const RATES: [&str; 5] = ["1/4", "1/2", "1x", "2x", "4x"];
const SYNCS: [&str; 3] = ["song", "retrigger", "free"];
const NOTE_MODES: [&str; 3] = ["off", "select", "retrigger"];
const QUALITIES: [&str; 2] = ["normal", "high"];
const LOOKS: [&str; 3] = ["off", "1 beat", "1 bar"];
const TONES: [&str; 3] = ["low/high", "band", "tilt"];

pub(crate) static DISGUSTING_BEAT_PARAMS: [crate::ParamSpec; 16] =
    with_mix(&DISGUSTING_BEAT_OWN_PARAMS, ALL_WET);

/// DisgustingBeat replaces the signal rather than sitting under it — a hold at
/// half mix is the hold flamming against the live sound, which is a real thing
/// to want and not a default.
const ALL_WET: f32 = 100.0;

pub(crate) static DISGUSTING_BEAT_SECTIONS: [crate::ParamSection; 6] = [
    crate::ParamSection {
        name: "Pattern",
        count: 5,
    },
    crate::ParamSection {
        name: "Time",
        count: 4,
    },
    crate::ParamSection {
        name: "Volume",
        count: 1,
    },
    crate::ParamSection {
        name: "Tone",
        count: 3,
    },
    crate::ParamSection {
        name: "Pan",
        count: 1,
    },
    crate::ParamSection {
        name: "Output",
        count: 2,
    },
];

static DISGUSTING_BEAT_OWN_PARAMS: [crate::ParamSpec; 15] = [
    crate::ParamSpec {
        id: "scene",
        name: "Scene",
        min: 1.0,
        max: DISGUSTING_BEAT_SCENES as f32,
        default: 1.0,
        unit: crate::Unit::None,
        taper: crate::Taper::Stepped(DISGUSTING_BEAT_SCENES as u32),
        positions: &SCENE_NAMES,
    },
    chooser("rate", "Rate", &RATES, 2.0),
    chooser("sync", "Sync", &SYNCS, 0.0),
    percent_param("swing", "Swing", 0.0),
    chooser("notes", "Notes", &NOTE_MODES, 1.0),
    percent_param("time", "Time", 100.0),
    crate::ParamSpec {
        id: "smooth",
        name: "Smooth",
        min: 0.0,
        max: 50.0,
        default: 12.0,
        unit: crate::Unit::Milliseconds,
        taper: crate::Taper::Linear,
        positions: &[],
    },
    chooser("quality", "Quality", &QUALITIES, 0.0),
    chooser("look", "Look ahead", &LOOKS, 0.0),
    percent_param("volume", "Volume", 100.0),
    percent_param("tone", "Tone", 0.0),
    chooser("tone_mode", "Tone mode", &TONES, 0.0),
    crate::ParamSpec {
        id: "tone_range",
        name: "Tone range",
        min: 1.0,
        max: 6.0,
        default: 3.0,
        unit: crate::Unit::None,
        taper: crate::Taper::Linear,
        positions: &[],
    },
    percent_param("pan", "Pan", 0.0),
    crate::ParamSpec {
        id: "out",
        name: "Output",
        min: -24.0,
        max: 24.0,
        default: 0.0,
        unit: crate::Unit::Decibels,
        taper: crate::Taper::Linear,
        positions: &[],
    },
];

const fn chooser(
    id: &'static str,
    name: &'static str,
    positions: &'static [&'static str],
    default: f32,
) -> crate::ParamSpec {
    crate::ParamSpec {
        id,
        name,
        min: 0.0,
        max: (positions.len() - 1) as f32,
        default,
        unit: crate::Unit::None,
        taper: crate::Taper::Stepped(positions.len() as u32),
        positions,
    }
}

/// Where the song is, as an effect that has to land on a beat needs it.
///
/// Built from [`TransportSnapshot`] by the node, so the DSP stays a function
/// of numbers and never sees the engine. Everything in it is about the
/// block's **first** sample: what a node asks "where am I" about is the audio
/// it is being asked to render.
///
/// [`TransportSnapshot`]: fontelle_engine
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MusicalTime {
    /// The song position at the block's first sample, in ticks.
    pub tick: f64,
    /// How far the song moves per sample from here, in ticks. Never zero.
    pub ticks_per_sample: f64,
    pub beats_per_bar: u32,
    pub bpm: f32,
    /// Whether the song is advancing.
    ///
    /// A stopped transport does not move `tick`, and an insert being
    /// auditioned with the transport stopped still has to stutter — or
    /// everybody who adds one before pressing play concludes it is broken. So
    /// a DisgustingBeat runs its own clock at `bpm` while this is false and
    /// re-locks to the song on the first rolling block.
    pub rolling: bool,
}

impl MusicalTime {
    /// A stopped transport at the top of a song.
    pub fn stopped(bpm: f32, sample_rate: f32) -> Self {
        Self {
            tick: 0.0,
            ticks_per_sample: bpm as f64 / 60.0 * PPQN as f64 / sample_rate.max(1.0) as f64,
            beats_per_bar: 4,
            bpm,
            rolling: false,
        }
    }

    /// And the same, rolling.
    pub fn playing(bpm: f32, sample_rate: f32) -> Self {
        Self {
            rolling: true,
            ..Self::stopped(bpm, sample_rate)
        }
    }

    /// How many samples one tick lasts.
    pub fn samples_per_tick(&self) -> f64 {
        1.0 / self.ticks_per_sample.max(f64::MIN_POSITIVE)
    }
}

impl DisgustingBeatConfig {
    /// What looking ahead costs, in samples.
    ///
    /// Here rather than in the node so that the document, the graph and the
    /// window all ask one function — `TuneConfig::latency_samples` is the
    /// same decision for the same reason. A beat and a bar are
    /// tempo-dependent, so this is computed at `prepare` time and held: a
    /// latency that changed per block would be a latency nothing could
    /// compensate.
    pub fn latency_samples(&self, bpm: f32, beats_per_bar: u32, sample_rate: f32) -> u32 {
        let beats = self.look.beats(beats_per_bar);
        if beats <= 0.0 {
            return 0;
        }
        let seconds = beats * 60.0 / bpm.max(1.0) as f64;
        (seconds * sample_rate.max(1.0) as f64).round() as u32
    }
}

/// How long one lane is, in ticks, once the rate has stretched it.
pub fn lane_ticks(
    length: DisgustingBeatLength,
    rate: DisgustingBeatRate,
    beats_per_bar: u32,
) -> f64 {
    (length.ticks(beats_per_bar) * rate.stretch()).max(1.0)
}

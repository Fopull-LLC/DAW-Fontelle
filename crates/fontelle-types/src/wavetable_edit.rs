//! What the wavetable editor asks of a table (`docs/flopsynth-next.md`
//! §4.3), as data: the window sends one of these through the host, and
//! `fontelle-core` applies it to the patch's own table. Here rather than
//! in `fontelle-core` because the window names the edit and the window
//! does not depend on the core (§4.1).

/// One thing the editor does to a table. The frame indices are the table's
/// own, 0-based; the coordinates are the cycle's — `x` across it in 0..1,
/// `y` in −1..1.
#[derive(Debug, Clone, PartialEq)]
pub enum WavetableEdit {
    /// A straight segment from `from` to `to`; the samples under it take
    /// the line, nothing outside it moves. Free drawing is a run of these
    /// from one pointer position to the next.
    Draw {
        frame: usize,
        from: (f32, f32),
        to: (f32, f32),
    },
    /// Harmonic `index` (0 is the fundamental) set to `amplitude`, the
    /// frame's other partials kept.
    Harmonic {
        frame: usize,
        index: usize,
        amplitude: f32,
    },
    /// The frame replaced by `text` evaluated over the phase.
    Formula { frame: usize, text: String },
    /// A silent frame inserted after `after`.
    AddFrame { after: usize },
    /// A copy of `frame` inserted after it.
    CopyFrame { frame: usize },
    /// `frame` taken out; the last one stays.
    RemoveFrame { frame: usize },
    /// The frames strictly between `from` and `to` filled with the way from
    /// one to the other — on the samples, or on the partials.
    Morph {
        from: usize,
        to: usize,
        spectral: bool,
    },
    /// The frame scaled to a peak of one.
    Normalise { frame: usize },
}

/// Which tool the editor's picture is: the position knob's own view of the
/// wave, a pencil over it, or the harmonic bars. The window's state, not
/// the patch's — a tool is how the picture is looked at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WaveTool {
    #[default]
    Position,
    Draw,
    Bars,
}

impl WaveTool {
    pub const ALL: [Self; 3] = [Self::Position, Self::Draw, Self::Bars];

    pub fn label(self) -> &'static str {
        match self {
            Self::Position => "position",
            Self::Draw => "draw",
            Self::Bars => "bars",
        }
    }
}

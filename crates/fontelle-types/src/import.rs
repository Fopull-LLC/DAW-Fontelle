//! Which kind of file an import is about.
//!
//! Here, in the crate both sides can see, because the question has two halves
//! that live in different places: the **settings** remember a folder per kind
//! (`fontelle-app`), and the **browser** draws a tab per kind
//! (`fontelle-ui`), and neither of those crates may depend on the other.

use std::path::Path;

/// One of the folders the import browser reads.
///
/// An enum rather than two of everything, because "which folder is this
/// about" is a question every one of these functions has to answer and the
/// answer being a parameter is what stops one of them getting it wrong — this
/// program has already shipped a *"change my projects folder"* button that
/// replaced the soundfont bank.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderKind {
    /// `.mid` and `.midi`.
    Midi,
    /// FL Studio's `.fsc` piano-roll scores.
    Scores,
    /// Sounds and loops: whatever the decoder reads (TDD §15).
    ///
    /// *"i want to also be able to record my voice into the daw or import
    /// different sounds and loops and whatnot to make songs with."* A third
    /// variant and nothing else, which is what this enum is for.
    Audio,
}

impl FolderKind {
    pub const ALL: [Self; 3] = [Self::Midi, Self::Scores, Self::Audio];

    /// The extensions a file has to have to show up in this folder's browser.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Midi => &["mid", "midi"],
            Self::Scores => &["fsc"],
            // What `symphonia` is built with here. Listing a format nothing
            // can open would be a lie, which is the same rule that keeps
            // `.sf3` out of the soundfont bank.
            Self::Audio => &["wav", "wave", "flac", "mp3", "ogg", "oga"],
        }
    }

    /// Whether `path` is a sound this folder would list **or** a
    /// multisample (`.sfz`) — what a drop on an oscillator takes. The
    /// listing itself is [`accepts`](Self::accepts): an SFZ is a text file
    /// naming sounds, not a sound, and the audio bank cannot play one.
    pub fn accepts_as_sound(self, path: &Path) -> bool {
        self.accepts(path) || (self == Self::Audio && is_multisample_path(path))
    }

    /// Whether `path` is a file this folder is for. Extension only, and
    /// case-insensitively, because the alternative is opening every file in
    /// the folder to find out.
    pub fn accepts(self, path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|found| {
                self.extensions()
                    .iter()
                    .any(|want| found.eq_ignore_ascii_case(want))
            })
    }

    /// What the folder picker calls itself.
    ///
    /// Not decoration: every picker in this program was once titled
    /// "Soundfont folder" whatever it was asking for, and a wrong answer is
    /// much easier to believe when the dialog agrees with it.
    pub fn picker_title(self) -> &'static str {
        match self {
            Self::Midi => "MIDI file folder",
            Self::Scores => "FL Studio score folder",
            Self::Audio => "Audio and loop folder",
        }
    }

    /// What the tab's row is called.
    pub fn label(self) -> &'static str {
        match self {
            Self::Midi => "MIDI files",
            Self::Scores => "FL scores",
            Self::Audio => "Audio files",
        }
    }

    /// What the browser tab says while it is showing this kind.
    pub fn tab_label(self) -> &'static str {
        match self {
            Self::Midi => "MIDI",
            Self::Scores => "Scores",
            Self::Audio => "Audio",
        }
    }

    /// The next one round, so a chip that shows one cycles them.
    pub fn next(self) -> Self {
        match self {
            Self::Midi => Self::Scores,
            Self::Scores => Self::Audio,
            Self::Audio => Self::Midi,
        }
    }
}

/// Whether `path` is an SFZ multisample (`docs/flopsynth-next.md` §4.3):
/// by extension, because that is all a drop carries.
pub fn is_multisample_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("sfz"))
}

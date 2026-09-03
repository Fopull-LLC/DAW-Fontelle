//! Which kind of file an import is about.
//!
//! Here, in the crate both sides can see, because the question has two halves
//! that live in different places: the **settings** remember a folder per kind
//! (`fontelle-app`), and the **browser** draws a tab per kind
//! (`fontelle-ui`), and neither of those crates may depend on the other.

use std::path::Path;

/// One of the two folders the import browser reads.
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
}

impl FolderKind {
    pub const ALL: [Self; 2] = [Self::Midi, Self::Scores];

    /// The extensions a file has to have to show up in this folder's browser.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Midi => &["mid", "midi"],
            Self::Scores => &["fsc"],
        }
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
        }
    }

    /// What the tab's row is called.
    pub fn label(self) -> &'static str {
        match self {
            Self::Midi => "MIDI files",
            Self::Scores => "FL scores",
        }
    }

    /// What the browser tab says while it is showing this kind.
    pub fn tab_label(self) -> &'static str {
        match self {
            Self::Midi => "MIDI",
            Self::Scores => "Scores",
        }
    }

    /// The other one, so a chip that shows it toggles.
    pub fn next(self) -> Self {
        match self {
            Self::Midi => Self::Scores,
            Self::Scores => Self::Midi,
        }
    }
}

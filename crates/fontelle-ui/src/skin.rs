//! Textures for the bridge, dropped into a folder — see
//! `assets/flopsynth/skin/README.md`.
//!
//! > *"sci fi materials (you can find some free assets online or make a
//! > folder for me to drop specific ones you request from me into etc.)"*
//!
//! Loaded once when the window opens, **never compiled in**: every surface
//! in `render/bridge.rs` is procedural without them, and a file here
//! replaces the procedural one under it. A file that does not decode is
//! skipped with a line on stderr, so a bad PNG is a bare surface rather
//! than a crash on open.

use std::path::{Path, PathBuf};

use vello::peniko::ImageData;

/// The textures found, each `None` where the folder had nothing usable.
#[derive(Debug, Clone, Default)]
pub struct Skin {
    /// Tiled under the consoles.
    pub hull: Option<ImageData>,
    /// Stretched over the canopy's opening, alpha and all.
    pub glass: Option<ImageData>,
    /// Scaled onto every knob's cap.
    pub knob: Option<ImageData>,
    /// Tiled across each console's face.
    pub console: Option<ImageData>,
}

impl Skin {
    /// The folder the window looks in: `FONTELLE_SKIN_DIR`, else
    /// `assets/flopsynth/skin` under the current directory, else the data
    /// directory's `skin`. The first that exists.
    pub fn folder() -> Option<PathBuf> {
        if let Ok(dir) = std::env::var("FONTELLE_SKIN_DIR") {
            let dir = PathBuf::from(dir);
            return dir.is_dir().then_some(dir);
        }
        let local = PathBuf::from("assets/flopsynth/skin");
        if local.is_dir() {
            return Some(local);
        }
        let data = std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .ok()
            .or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .map(|h| PathBuf::from(h).join(".local/share"))
            })?
            .join("fontelle/skin");
        data.is_dir().then_some(data)
    }

    /// Whatever `dir` holds of the four.
    pub fn load(dir: &Path) -> Self {
        let read = |name: &str| {
            let path = dir.join(name);
            if !path.is_file() {
                return None;
            }
            match std::fs::read(&path) {
                Ok(bytes) => match crate::branding::decode_png(&bytes) {
                    Ok(image) => Some(image),
                    Err(e) => {
                        eprintln!("skin: {} skipped: {e}", path.display());
                        None
                    }
                },
                Err(e) => {
                    eprintln!("skin: {} skipped: {e}", path.display());
                    None
                }
            }
        };
        Self {
            hull: read("hull.png"),
            glass: read("glass.png"),
            knob: read("knob.png"),
            console: read("console.png"),
        }
    }

    /// The skin from [`folder`](Self::folder), or none.
    pub fn find() -> Self {
        Self::folder()
            .map(|dir| Self::load(&dir))
            .unwrap_or_default()
    }

    pub fn is_empty(&self) -> bool {
        self.hull.is_none() && self.glass.is_none() && self.knob.is_none() && self.console.is_none()
    }
}

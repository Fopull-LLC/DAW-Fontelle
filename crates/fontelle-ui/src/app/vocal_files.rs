//! A vocal track's song files and renders, from the window: its own rows on
//! the rack menu (import, export, freeze, render a selection) and the new
//! instrument menu's *Vocal track from a song file…*. What each does is the
//! host's (`StudioHost::choose_and_import_vocal` and the rest); this is the
//! plumbing between the menu and it.

use super::*;

use crate::canvas::VocalTrackChoice;

/// How many rows the rack's channel menu has before a vocal track's own.
pub(super) const CHANNEL_MENU_ROWS: usize = 6;

/// The new instrument menu's last row.
pub(super) const VOCAL_FROM_FILE: &str = "Vocal track from a song file\u{2026}";

impl WindowApp {
    /// Row `row` of vocal track `channel`'s own rows was chosen.
    pub(super) fn choose_vocal_track_row(&mut self, channel: usize, row: usize) {
        let Some(doc) = &mut self.options.document else {
            return;
        };
        let frozen = doc.vocal_frozen(channel);
        let Some(choice) = crate::canvas::vocal_track_menu_choice(row, frozen) else {
            return;
        };
        let said = match choice {
            VocalTrackChoice::Import => doc
                .choose_and_import_vocal(Some(channel))
                .map(Option::unwrap_or_default),
            VocalTrackChoice::Export(format) => doc.export_vocal(channel, format),
            VocalTrackChoice::Freeze => doc.freeze_vocal(channel),
            VocalTrackChoice::Unfreeze => doc.unfreeze_vocal(channel),
            VocalTrackChoice::RenderSelection => {
                doc.select_channel(channel);
                doc.render_vocal_selection()
            }
        };
        self.said(said);
    }

    /// A vocal track (or several) made from a song file the desktop's
    /// picker is asked for.
    pub(super) fn import_vocal_file_as_track(&mut self) {
        let Some(doc) = &mut self.options.document else {
            return;
        };
        let said = doc
            .choose_and_import_vocal(None)
            .map(Option::unwrap_or_default);
        if said.as_ref().is_ok_and(|s| !s.is_empty()) {
            self.rack_scroll = usize::MAX;
        }
        self.said(said);
    }

    fn said(&mut self, said: Result<String, String>) {
        match said {
            Ok(s) if !s.is_empty() => self.status = s,
            Ok(_) => {}
            Err(e) => self.status = e,
        }
        self.refresh_studio();
        self.refresh_title();
        self.refresh_vocal();
        self.lane_made();
        self.tree.invalidate(PANEL);
        self.tree.invalidate(RACK);
        self.tree.invalidate(TIMELINE);
    }
}

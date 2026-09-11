//! An LV2 plugin's own editor, on a platform that has no LV2 — see
//! `lv2_stub.rs` for why this is a stub and not a `cfg` at every use.

use crate::gui::GuiSize;

/// The editor that would be open. Uninhabited: `Lv2Plugin::open_editor` is
/// the only thing that makes one, and there is no `Lv2Plugin` here.
pub(crate) enum Lv2Ui {}

impl Lv2Ui {
    pub(crate) fn tick(&mut self) -> bool {
        match *self {}
    }

    pub(crate) fn take_resize(&self) -> Option<GuiSize> {
        match *self {}
    }
}

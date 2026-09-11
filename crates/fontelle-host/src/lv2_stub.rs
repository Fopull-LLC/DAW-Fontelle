//! LV2, on a platform that has none (TDD §8.4).
//!
//! The real arm is `lv2.rs`, which loads bundles through `lilv` — a system
//! library that lives on Linux, where the format does. A Windows or macOS
//! build cannot link it and has nothing to point it at, so this stands in:
//! the same names with the same signatures, every one of which answers
//! "not on this platform". The plugin types are **uninhabited** — an `enum`
//! with no variants — so the `Inner::Lv2` arm the rest of the host carries
//! is one the compiler can see is never taken, and the host's own code is
//! one code on every platform rather than a thicket of `cfg`s.
//!
//! Chosen over gating LV2 out of `plugin.rs`, `processor.rs` and `scan.rs`
//! site by site because that is forty sites today and a new one every time
//! the format grows, each a chance to build a Linux binary that differs in
//! behaviour rather than only in what it links.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use fontelle_types::{PluginFormat, PluginKey};

use crate::param::{HostedParam, ParamValues};
use crate::plugin::HostError;
use crate::scan::PluginInfo;

/// The largest block an LV2 instance is asked for, as on Linux — see
/// `lv2.rs`. Kept identical so a project's limits do not depend on the
/// platform it is read on.
pub const LV2_MAX_BLOCK: usize = 8192;

/// One lilv world per bundle, on Linux. Here, a world nobody can load.
pub(crate) struct World;

/// The features a world's plugins share, on Linux. Here, nothing.
pub(crate) struct Features;

/// A loaded LV2 plugin. Uninhabited: none is ever opened here.
pub(crate) enum Lv2Plugin {}

/// An activated instance. Uninhabited, like the plugin it would come from.
pub(crate) enum Lv2Processor {}

/// What `open` would return — the shape the caller unpacks on Linux.
pub(crate) struct Opened {
    pub info: PluginInfo,
    pub params: Vec<HostedParam>,
    pub audio_inputs: u32,
    pub audio_outputs: u32,
    pub input_ports: crate::plugin::PortLayout,
    pub accepts_notes: bool,
    pub keeps_state: bool,
    pub plugin: Lv2Plugin,
}

fn not_here() -> String {
    "LV2 plugins are hosted on Linux only".to_string()
}

pub(crate) fn load_world(_path: &Path) -> Result<World, String> {
    Err(not_here())
}

pub(crate) fn read_lv2_bundle(_path: &Path) -> Result<Vec<PluginInfo>, String> {
    Err(not_here())
}

pub(crate) fn open(
    _path: &Path,
    _key: &PluginKey,
    _world: &World,
    _features: &Arc<Features>,
) -> Result<Opened, HostError> {
    Err(HostError::Unsupported(PluginFormat::Lv2))
}

pub(crate) fn build_features(_world: &World) -> Arc<Features> {
    Arc::new(Features)
}

/// Where LV2 bundles would be looked for. Nowhere: a folder full of
/// `.lv2` bundles on a machine that cannot load them is a menu of rows that
/// all fail.
pub(crate) fn search_paths(_home: Option<&PathBuf>) -> Vec<PathBuf> {
    Vec::new()
}

impl Lv2Plugin {
    pub(crate) fn atom_ports(&self) -> (Option<u32>, Option<u32>) {
        match *self {}
    }

    pub(crate) fn has_editor(&self) -> bool {
        match *self {}
    }

    pub(crate) fn open_editor(
        &self,
        _plugin_uri: &str,
        _bundle: &Path,
        _values: Arc<ParamValues>,
        _atoms: Arc<crate::atom::AtomPipes>,
        _params: &[HostedParam],
        _window: &crate::gui::PluginWindow,
    ) -> Result<crate::lv2_ui::Lv2Ui, crate::gui::GuiError> {
        match *self {}
    }

    pub(crate) fn activate(
        &self,
        _key: &PluginKey,
        _values: Arc<ParamValues>,
        _atoms: Arc<crate::atom::AtomPipes>,
        _sample_rate: f64,
        _max_block: usize,
    ) -> Result<Lv2Processor, HostError> {
        match *self {}
    }

    pub(crate) fn keeps_state(&self) -> bool {
        match *self {}
    }

    pub(crate) fn pending_state(&self) -> Option<&[u8]> {
        match *self {}
    }

    pub(crate) fn stash_state(&mut self, _bytes: &[u8]) -> bool {
        match *self {}
    }
}

impl Lv2Processor {
    pub(crate) fn max_block(&self) -> usize {
        match *self {}
    }

    pub(crate) fn input(&mut self) -> &mut [Vec<f32>] {
        match *self {}
    }

    pub(crate) fn fill_key(&mut self, _key: Option<&[f32]>, _frames: usize) {
        match *self {}
    }

    pub(crate) fn output(&self) -> &[Vec<f32>] {
        match *self {}
    }

    pub(crate) fn save_state(&mut self) -> Option<Vec<u8>> {
        match *self {}
    }

    pub(crate) fn note_on(&mut self, _frame: usize, _key: u8, _velocity: f64) {
        match *self {}
    }

    pub(crate) fn note_off(&mut self, _frame: usize, _key: u8) {
        match *self {}
    }

    pub(crate) fn controller(&mut self, _frame: usize, _controller: u8, _value: u8) {
        match *self {}
    }

    pub(crate) fn pitch_bend(&mut self, _frame: usize, _value: i16) {
        match *self {}
    }

    pub(crate) fn channel_pressure(&mut self, _frame: usize, _value: u8) {
        match *self {}
    }

    pub(crate) fn reset(&mut self) {
        match *self {}
    }

    pub(crate) fn run(&mut self, _frames: usize) {
        match *self {}
    }
}

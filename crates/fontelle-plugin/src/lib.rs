//! `nice-plug` wrapper exporting the sampler as a standalone CLAP (and, licensing
//! permitting, VST3) plugin (FONTELLE_TDD.md §8). This lands at M2, deliberately
//! early — it proves the `fontelle-core` / `fontelle-model` boundary (INVARIANT 4)
//! is real before the DAW grows around it, and produces a shippable artefact long
//! before the DAW itself is finished.

mod editor;
mod params;
mod plugin;

pub use editor::PluginEditor;
pub use params::{PluginParam, ValueDistribution};
pub use plugin::FontellePlugin;

use fontelle_core::{Patch, Sampler};

use crate::params::PluginParam;

/// The standalone-plugin entry point (TDD §8.1): the sampler ships as CLAP (and,
/// licensing permitting, VST3 — §3.4) from the first release. Wiring this to
/// `nice-plug`'s `ClapPlugin`/`Vst3Plugin` traits is M2 work; the boundary this
/// crate exists to prove is `fontelle-core`'s public API (construct from a patch,
/// receive events, render, report parameters) plus the `PluginParam` contract —
/// nothing else should be needed to host it.
pub struct FontellePlugin {
    sampler: Sampler,
    params: Vec<PluginParam>,
}

impl FontellePlugin {
    pub fn new(patch: Patch) -> Self {
        Self {
            sampler: Sampler::new(patch),
            params: Vec::new(),
        }
    }

    pub fn params(&self) -> &[PluginParam] {
        &self.params
    }

    pub fn sampler(&self) -> &Sampler {
        &self.sampler
    }
}

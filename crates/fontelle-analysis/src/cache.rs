//! Analyses kept on disk, keyed by the samples' hash and the engine version
//! (plan §3.10). Where is the caller's to say (INVARIANT 10: the project's
//! `cache/analysis/` or the XDG cache).

use crate::analysis::Analysis;
use std::path::Path;

/// The key an analysis of these samples is kept under: the SHA-256 of the
/// rate and the samples' bytes, then the engine version.
pub fn cache_key(samples: &[f32], sample_rate: u32) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(sample_rate.to_le_bytes());
    for s in samples {
        hash.update(s.to_le_bytes());
    }
    let hex: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    format!("{hex}-{}", crate::analysis::ENGINE_VERSION)
}

fn path(dir: &Path, key: &str) -> std::path::PathBuf {
    dir.join(format!("{key}.json"))
}

/// Writes `analysis` under `key` in `dir` (made if missing): to a temporary
/// file first and renamed over, so a reader never sees half of one.
pub fn store(dir: &Path, key: &str, analysis: &Analysis) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let json = serde_json::to_vec(analysis).map_err(std::io::Error::other)?;
    let target = path(dir, key);
    let partial = dir.join(format!("{key}.json.partial"));
    std::fs::write(&partial, json)?;
    std::fs::rename(&partial, &target)
}

/// The analysis kept under `key` in `dir`; `None` if there is none, it does
/// not parse, or another engine version made it.
pub fn load(dir: &Path, key: &str) -> Option<Analysis> {
    let bytes = std::fs::read(path(dir, key)).ok()?;
    let analysis: Analysis = serde_json::from_slice(&bytes).ok()?;
    (analysis.engine == crate::analysis::ENGINE_VERSION).then_some(analysis)
}

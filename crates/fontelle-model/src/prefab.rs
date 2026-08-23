use std::collections::{HashMap, HashSet};

use fontelle_types::PrefabId;

use crate::clip::ClipSource;

/// An opaque handle to one addressable element inside a `ClipSource` (a note, an
/// automation point, ...). Persistent (INVARIANT 8) — never an index — so an
/// override survives edits to the source, insertions before it, and a save/load
/// round trip (TDD §10.2, §10.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ElementId(pub fontelle_types::PersistentId);

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PropKey {
    Transpose,
    Velocity,
    Length,
    // Extended as per-property override/revert UI lands (TDD §10.5.1, post-v1).
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum PropValue {
    Int(i64),
    Float(f64),
}

/// A variant/instance's deltas against its base. Structural overrides (`added`,
/// `removed`) are structure-only in v1 — the data shape ships now so no project
/// migration is needed when the feature lands (TDD §10.5.1).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct OverrideMap {
    pub props: HashMap<(ElementId, PropKey), PropValue>,
    pub added: Vec<ElementId>,
    pub removed: HashSet<ElementId>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Prefab {
    pub name: String,
    /// `Some(_)` makes this a variant. Structure-only in v1 (TDD §10.5.1).
    pub base: Option<PrefabId>,
    pub source: ClipSource,
    pub overrides: OverrideMap,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PrefabLink {
    pub prefab: PrefabId,
    pub overrides: OverrideMap,
}

/// Resolution order: base prefab → variant chain (outermost last) → instance
/// overrides. Memoised, invalidated when any ancestor changes. Must reject a
/// mutation that would create a cycle in the `base` chain rather than panicking
/// (TDD §10.5).
pub fn resolve(_prefab: &Prefab, _link: Option<&PrefabLink>) -> ClipSource {
    todo!("apply variant chain then instance overrides over the base ClipSource")
}

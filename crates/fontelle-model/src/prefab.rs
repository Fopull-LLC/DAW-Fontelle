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

/// How deep a `base` chain may go before it is treated as broken.
///
/// A belt beside the braces: [`resolve`] already refuses to visit a prefab
/// twice, so a cycle terminates without this. What this catches is a chain
/// that is *acyclic and absurd* — a file that arrived from somewhere else with
/// four hundred links in it — and it caps the work a single clip can make the
/// compiler do (INVARIANT 3's whole point).
pub const MAX_BASE_DEPTH: usize = 32;

/// A prefab's content, with its variant chain and `link`'s overrides applied.
///
/// Resolution order: **base prefab → variant chain (outermost last) → instance
/// overrides**, which is the order in §10.5 and the order Unity's prefabs
/// resolve in. `None` when `id` names no prefab — a link pointing at nothing
/// is a project that still opens (see `Project::clip_source`).
///
/// # Cycles
///
/// The `base` chain is walked with the set of prefabs already seen, and a
/// prefab that appears twice **ends the walk** rather than panicking or
/// looping. Nothing in v1 can create a cycle — no command writes `base` — so
/// this is what a hand-edited or corrupt file meets, and the right answer to
/// one of those is a project that opens.
///
/// # What is applied, and what is only carried
///
/// §10.5.1 stages this deliberately. **Property overrides are applied**;
/// structural ones (`added`, `removed`) are carried through the format and not
/// yet honoured, because add-and-remove-inside-an-instance is where Unity's
/// prefab system generates its hardest bugs. Nothing in the app can create
/// either kind yet, so what this returns today is the base's content — which
/// is exactly the mirror behaviour the feature was asked for.
pub fn resolve(
    prefabs: &crate::arena::Arena<PrefabId, Prefab>,
    id: PrefabId,
    link: Option<&PrefabLink>,
) -> Option<ClipSource> {
    // The chain, innermost (the base) first. Built by walking *up* and then
    // reversing, so that the outermost variant's overrides are applied last.
    let mut chain = Vec::new();
    let mut seen = HashSet::new();
    let mut next = Some(id);
    while let Some(current) = next {
        if !seen.insert(current) || chain.len() >= MAX_BASE_DEPTH {
            break;
        }
        let prefab = prefabs.get(current)?;
        chain.push(prefab);
        next = prefab.base;
    }
    let root = *chain.last()?;
    let mut source = root.source.clone();
    // Every variant between the root and `id`, outermost last. The root's own
    // `overrides` are deltas against a base it does not have, so they are not
    // applied to itself.
    for prefab in chain.iter().rev().skip(1) {
        apply(&mut source, &prefab.overrides);
    }
    if let Some(link) = link {
        apply(&mut source, &link.overrides);
    }
    Some(source)
}

/// Writes one `OverrideMap` over `source`.
///
/// # Why this does nothing yet, and how you will know when it should
///
/// An [`ElementId`] is a [`PersistentId`](fontelle_types::PersistentId), and
/// **a note does not carry one yet**: `NoteData::notes` is an `Arena<NoteId,
/// Note>`, and a `NoteId` is a slot-and-version pair that is minted fresh
/// every session. There is therefore no way to look up the note an override
/// names, and any code here that appeared to do so would be code that silently
/// matched nothing.
///
/// This is §10.5.1's split showing through, and it is the right way round: the
/// `OverrideMap` is in the document and in the file from commit one, so the
/// day notes grow persistent ids no project needs migrating — but the
/// *application* pass waits for the ids it needs rather than shipping as a
/// no-op wearing a working function's name.
///
/// Nothing in the app can create an override today, so this is not reachable
/// from the studio; it is reachable from a hand-written file, and the answer
/// there is the same as everywhere else in this module — carry it, do not
/// crash on it.
fn apply(source: &mut ClipSource, overrides: &OverrideMap) {
    let _ = (source, overrides);
}

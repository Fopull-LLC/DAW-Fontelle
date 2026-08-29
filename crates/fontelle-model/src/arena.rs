//! A keyed arena that can be given back an id it previously minted.

use std::marker::PhantomData;

use slotmap::{Key, KeyData};

/// Storage for every id-addressed collection in the document.
///
/// **Why not `slotmap`, which this was.** Undo needs the inverse of "delete
/// note A" to be "put note A back" — the same id, because the command above it
/// in the history refers to it. A slotmap can only mint a *fresh* key on
/// reinsertion, so a redo would then try to move a note that no longer exists,
/// and the plan's own acceptance test (apply, invert, and the document is
/// where it started) cannot pass. Nothing else about a slotmap was wrong, so
/// this is a slotmap with one operation added: [`Arena::insert_at`].
///
/// The shape is the same and so are the costs: a dense `Vec` indexed by the
/// key's index, a free list, and a version per slot so a stale key never reads
/// a slot that has been reused. Keys are `slotmap`'s own key types, which is
/// what keeps them a `Copy` integer pair and keeps TDD §10.2's on-disk story
/// (a `PersistentId` beside them) unchanged.
///
/// Iteration is in index order — deterministic, which matters more than it
/// looks: the sequencer numbers voice contexts by a clip's position in this
/// collection, and the realisation step numbers engine nodes by a channel's.
/// An unordered container would make two runs of the same project render
/// differently.
#[derive(Debug, Clone)]
pub struct Arena<K: Key, V> {
    slots: Vec<Slot<V>>,
    /// Indices of vacant slots, most recently freed first.
    free: Vec<u32>,
    len: usize,
    key: PhantomData<fn() -> K>,
}

#[derive(Debug, Clone)]
struct Slot<V> {
    /// Odd while occupied, even while vacant — `slotmap`'s own convention, and
    /// forced on us anyway because `KeyData::from_ffi` makes every version it
    /// builds odd.
    version: u32,
    value: Option<V>,
}

/// Splits a key into the slot it addresses and the version it expects.
fn parts<K: Key>(key: K) -> (u32, u32) {
    let ffi = key.data().as_ffi();
    ((ffi & 0xffff_ffff) as u32, (ffi >> 32) as u32)
}

fn key_from<K: Key>(index: u32, version: u32) -> K {
    K::from(KeyData::from_ffi(((version as u64) << 32) | index as u64))
}

impl<K: Key, V> Default for Arena<K, V> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
            len: 0,
            key: PhantomData,
        }
    }
}

impl<K: Key, V> Arena<K, V> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn insert(&mut self, value: V) -> K {
        let index = match self.free.pop() {
            Some(index) => index,
            None => {
                self.slots.push(Slot {
                    version: 0,
                    value: None,
                });
                (self.slots.len() - 1) as u32
            }
        };
        let slot = &mut self.slots[index as usize];
        // Vacant versions are even, occupied are odd, so this both marks the
        // slot taken and invalidates every key that named its previous
        // occupant.
        slot.version += 1;
        slot.value = Some(value);
        self.len += 1;
        key_from(index, slot.version)
    }

    /// Puts `value` back under an id this arena minted earlier.
    ///
    /// This is the operation undo is built on, and the only one a slotmap
    /// cannot do. It succeeds when the slot is vacant and fails when something
    /// else is living there — which, under a strictly last-in-first-out
    /// history, cannot happen: anything inserted after `key` was removed has
    /// itself been undone by the time this runs. A `false` therefore means a
    /// mutation reached the document without going through a command
    /// (INVARIANT 9), which is worth surfacing rather than papering over.
    pub fn insert_at(&mut self, key: K, value: V) -> bool {
        let (index, version) = parts(key);
        if self.slots.len() <= index as usize {
            self.slots.resize_with(index as usize + 1, || Slot {
                version: 0,
                value: None,
            });
        }
        if self.slots[index as usize].value.is_some() {
            return false;
        }
        self.free.retain(|free| *free != index);
        self.slots[index as usize] = Slot {
            version,
            value: Some(value),
        };
        self.len += 1;
        true
    }

    pub fn remove(&mut self, key: K) -> Option<V> {
        let (index, version) = parts(key);
        let slot = self.slots.get_mut(index as usize)?;
        if slot.version != version {
            return None;
        }
        let value = slot.value.take()?;
        // Even again: the key just removed will never match this slot, even if
        // the next `insert` reuses it.
        slot.version += 1;
        self.free.push(index);
        self.len -= 1;
        Some(value)
    }

    pub fn get(&self, key: K) -> Option<&V> {
        let (index, version) = parts(key);
        let slot = self.slots.get(index as usize)?;
        if slot.version != version {
            return None;
        }
        slot.value.as_ref()
    }

    pub fn get_mut(&mut self, key: K) -> Option<&mut V> {
        let (index, version) = parts(key);
        let slot = self.slots.get_mut(index as usize)?;
        if slot.version != version {
            return None;
        }
        slot.value.as_mut()
    }

    pub fn contains_key(&self, key: K) -> bool {
        self.get(key).is_some()
    }

    pub fn iter(&self) -> impl Iterator<Item = (K, &V)> {
        self.slots.iter().enumerate().filter_map(|(index, slot)| {
            slot.value
                .as_ref()
                .map(|value| (key_from(index as u32, slot.version), value))
        })
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (K, &mut V)> {
        self.slots
            .iter_mut()
            .enumerate()
            .filter_map(|(index, slot)| {
                let version = slot.version;
                slot.value
                    .as_mut()
                    .map(move |value| (key_from(index as u32, version), value))
            })
    }

    pub fn keys(&self) -> impl Iterator<Item = K> + '_ {
        self.iter().map(|(key, _)| key)
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.slots.iter().filter_map(|slot| slot.value.as_ref())
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.slots.iter_mut().filter_map(|slot| slot.value.as_mut())
    }
}

impl<K: Key, V> std::ops::Index<K> for Arena<K, V> {
    type Output = V;

    fn index(&self, key: K) -> &V {
        self.get(key).expect("no such element in this arena")
    }
}

impl<K: Key, V> std::ops::IndexMut<K> for Arena<K, V> {
    fn index_mut(&mut self, key: K) -> &mut V {
        self.get_mut(key).expect("no such element in this arena")
    }
}

impl<K: Key, V> FromIterator<V> for Arena<K, V> {
    fn from_iter<I: IntoIterator<Item = V>>(iter: I) -> Self {
        let mut arena = Self::new();
        for value in iter {
            arena.insert(value);
        }
        arena
    }
}

// --- Serialisation ---------------------------------------------------------
//
// A list of (key, value) pairs, not a map: a `slotmap` key serialises as a
// struct, and JSON has no way to write one as an object key. A list also keeps
// the ids visible and the file diffable, which is why TDD §17.2 chose JSON.

impl<K: Key + serde::Serialize, V: serde::Serialize> serde::Serialize for Arena<K, V> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(Some(self.len))?;
        for entry in self.iter() {
            seq.serialize_element(&entry)?;
        }
        seq.end()
    }
}

impl<'de, K: Key + serde::Deserialize<'de>, V: serde::Deserialize<'de>> serde::Deserialize<'de>
    for Arena<K, V>
{
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let entries: Vec<(K, V)> = Vec::deserialize(deserializer)?;
        let mut arena = Self::new();
        for (key, value) in entries {
            if !arena.insert_at(key, value) {
                return Err(serde::de::Error::custom(
                    "two elements in this collection claim the same id",
                ));
            }
        }
        // Every index below the high-water mark that nobody claimed is a hole
        // the next insert may take.
        arena.free = (0..arena.slots.len() as u32)
            .filter(|index| arena.slots[*index as usize].value.is_none())
            .rev()
            .collect();
        Ok(arena)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    slotmap::new_key_type! { struct TestId; }

    fn arena(values: &[&str]) -> (Arena<TestId, String>, Vec<TestId>) {
        let mut arena = Arena::new();
        let ids = values.iter().map(|v| arena.insert(v.to_string())).collect();
        (arena, ids)
    }

    #[test]
    fn what_goes_in_comes_back_out_under_the_id_it_was_given() {
        let (arena, ids) = arena(&["a", "b", "c"]);
        assert_eq!(arena.len(), 3);
        assert_eq!(arena[ids[1]], "b");
        assert_eq!(arena.get(ids[2]).map(String::as_str), Some("c"));
    }

    #[test]
    fn every_insert_gets_an_id_of_its_own() {
        let (_, ids) = arena(&["a", "b", "c"]);
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), 3);
    }

    #[test]
    fn insert_at_restores_the_exact_id_a_removal_freed() {
        // The whole reason this type exists. Undo's inverse of "delete note A"
        // is "put note A back", and it has to be *A* — the command above it in
        // the history refers to it by that id.
        let (mut arena, ids) = arena(&["a", "b", "c"]);
        let value = arena.remove(ids[1]).expect("b was there");
        assert!(!arena.contains_key(ids[1]));

        assert!(arena.insert_at(ids[1], value));
        assert_eq!(arena[ids[1]], "b");
        assert_eq!(arena.len(), 3);
    }

    #[test]
    fn a_removal_and_a_restore_leave_the_arena_exactly_where_it_started() {
        let (mut arena, ids) = arena(&["a", "b", "c"]);
        let before: Vec<_> = arena.iter().map(|(k, v)| (k, v.clone())).collect();

        let value = arena.remove(ids[1]).unwrap();
        arena.insert_at(ids[1], value);

        let after: Vec<_> = arena.iter().map(|(k, v)| (k, v.clone())).collect();
        assert_eq!(before, after, "ids and order both have to come back");
    }

    #[test]
    fn a_reused_slot_never_answers_to_the_key_it_just_freed() {
        // A stale key reading a slot's new occupant is the bug the version
        // counter exists to prevent, and it is silent: the wrong note moves.
        let (mut arena, ids) = arena(&["a"]);
        arena.remove(ids[0]);
        let replacement = arena.insert("b".to_string());

        assert_ne!(replacement, ids[0]);
        assert!(arena.get(ids[0]).is_none());
        assert_eq!(arena[replacement], "b");
    }

    #[test]
    fn insert_at_refuses_a_slot_something_else_is_living_in() {
        // Under a last-in-first-out history this cannot happen: anything
        // inserted after `a` was removed has itself been undone by the time
        // the restore runs. A `false` therefore means a mutation reached the
        // document without going through a command (INVARIANT 9).
        let (mut arena, ids) = arena(&["a"]);
        arena.remove(ids[0]);
        arena.insert("squatter".to_string());

        assert!(!arena.insert_at(ids[0], "a".to_string()));
        assert_eq!(arena.len(), 1);
    }

    #[test]
    fn removing_a_key_twice_is_not_a_second_removal() {
        let (mut arena, ids) = arena(&["a", "b"]);
        assert_eq!(arena.remove(ids[0]).as_deref(), Some("a"));
        assert!(arena.remove(ids[0]).is_none());
        assert_eq!(arena.len(), 1);
    }

    #[test]
    fn iteration_is_in_index_order_and_skips_the_holes() {
        // The sequencer numbers voice contexts by a clip's position here and
        // the realisation step numbers engine nodes by a channel's, so an
        // unordered container would make two runs of one project render
        // differently.
        let (mut arena, ids) = arena(&["a", "b", "c", "d"]);
        arena.remove(ids[1]);
        let seen: Vec<&str> = arena.values().map(String::as_str).collect();
        assert_eq!(seen, vec!["a", "c", "d"]);

        // And a reinsertion lands back in the hole rather than at the end,
        // which is what keeps the backing array dense.
        arena.insert("e".to_string());
        let seen: Vec<&str> = arena.values().map(String::as_str).collect();
        assert_eq!(seen, vec!["a", "e", "c", "d"]);
    }

    #[test]
    fn a_serde_round_trip_preserves_every_id_and_every_hole() {
        // A project reopens to the same ids it was saved with, or every
        // cross-reference in the document points at the wrong thing.
        let (mut arena, ids) = arena(&["a", "b", "c"]);
        arena.remove(ids[1]);

        let json = serde_json::to_string(&arena).unwrap();
        let back: Arena<TestId, String> = serde_json::from_str(&json).unwrap();

        assert_eq!(
            arena
                .iter()
                .map(|(k, v)| (k, v.clone()))
                .collect::<Vec<_>>(),
            back.iter().map(|(k, v)| (k, v.clone())).collect::<Vec<_>>()
        );
        assert!(back.get(ids[1]).is_none());
        assert_eq!(back[ids[2]], "c");
    }

    #[test]
    fn a_reloaded_arena_still_has_the_hole_to_insert_into() {
        // The free list is derived on load rather than saved, because a free
        // list that can disagree with the slots it describes is a bug waiting
        // to happen — the same reason `TempoMap` rebuilds its prefix table.
        let (mut arena, ids) = arena(&["a", "b", "c"]);
        arena.remove(ids[0]);
        let json = serde_json::to_string(&arena).unwrap();
        let mut back: Arena<TestId, String> = serde_json::from_str(&json).unwrap();

        back.insert("d".to_string());
        assert_eq!(
            back.values().map(String::as_str).collect::<Vec<_>>(),
            vec!["d", "b", "c"]
        );
    }

    #[test]
    fn the_serialised_form_is_a_list_of_pairs_rather_than_a_map() {
        // A `slotmap` key serialises as a struct, and JSON has no way to write
        // one as an object key — a map would simply fail to serialise.
        let (arena, _) = arena(&["a"]);
        let json = serde_json::to_string(&arena).unwrap();
        assert!(json.starts_with('['), "got {json}");
    }
}

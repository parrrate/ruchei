use std::{
    borrow::Borrow,
    collections::{BTreeMap, BTreeSet},
};

use slab::Slab;

use crate::{
    multi_trie::{MultiTrie, MultiTrieAddOwned, MultiTrieAddRef, MultiTriePrefix},
    nodes::NodeId,
    trie::Trie,
};

pub struct CloneMultiTrie<T> {
    keys: Trie<BTreeMap<T, usize>>,
    collections: BTreeMap<T, Slab<NodeId>>,
}

impl<T> Default for CloneMultiTrie<T> {
    fn default() -> Self {
        Self {
            keys: Default::default(),
            collections: Default::default(),
        }
    }
}

impl<T: Ord> MultiTrie<T> for CloneMultiTrie<T> {
    fn mt_remove(&mut self, collection: &T, key: &[u8]) {
        let Some(slab) = self.collections.get_mut(collection) else {
            return;
        };
        let Some((id, map)) = self.keys.get_mut(key) else {
            return;
        };
        let Some(index) = map.remove(collection) else {
            return;
        };
        slab.remove(index);
        if map.is_empty() {
            self.keys.remove_at(id).expect("invalid state");
        }
        if slab.is_empty() {
            self.collections.remove(collection).expect("invalid state");
        }
    }

    fn mt_contains(&self, collection: &T, key: &[u8]) -> bool {
        let Some((_, map)) = self.keys.get(key) else {
            return false;
        };
        map.contains_key(collection)
    }

    fn mt_clear(&mut self, collection: &T) {
        let Some(slab) = self.collections.remove(collection) else {
            return;
        };
        for (index_expected, id) in slab {
            let map = self.keys.try_index_mut(id).expect("invalid state");
            let index_actual = map.remove(collection).expect("invalid state");
            assert_eq!(index_actual, index_expected);
            if map.is_empty() {
                self.keys.remove_at(id).expect("invalid state");
            }
        }
    }

    fn mt_is_empty(&self, collection: &T) -> bool {
        !self.collections.contains_key(collection)
    }

    fn mt_len(&self, collection: &T) -> usize {
        self.collections
            .get(collection)
            .map(|slab| slab.len())
            .unwrap_or_default()
    }
}

impl<T: Clone + Ord> MultiTrieAddRef<T> for CloneMultiTrie<T> {
    fn mt_add_ref(&mut self, collection: &T, key: &[u8]) {
        if !self.collections.contains_key(collection) {
            self.collections.insert(collection.clone(), Slab::new());
        }
        if self.keys.get(key).is_none() {
            self.keys.insert(key, BTreeMap::new());
        }
        let slab = self.collections.get_mut(collection).expect("just inserted");
        let (id, map) = self.keys.get_mut(key).expect("just inserted");
        if !map.contains_key(collection) {
            map.insert(collection.clone(), slab.insert(id));
        }
    }
}

impl<T: Clone + Ord> MultiTrieAddOwned<T> for CloneMultiTrie<T> {
    fn mt_add_owned(&mut self, collection: T, key: &[u8]) {
        if !self.collections.contains_key(&collection) {
            self.collections.insert(collection.clone(), Slab::new());
        }
        if self.keys.get(key).is_none() {
            self.keys.insert(key, BTreeMap::new());
        }
        let slab = self
            .collections
            .get_mut(&collection)
            .expect("just inserted");
        let (id, map) = self.keys.get_mut(key).expect("just inserted");
        map.entry(collection).or_insert_with(|| slab.insert(id));
    }
}

impl<T: Ord> MultiTriePrefix for CloneMultiTrie<T> {
    type Collection = T;

    fn mt_prefix_collect<'a>(
        &'a self,
        suffix: &[u8],
    ) -> impl 'a + IntoIterator<Item: 'a + Borrow<Self::Collection>> {
        self.keys
            .prefix_of(suffix)
            .flat_map(|map| map.keys())
            .collect::<BTreeSet<_>>()
    }
}

#[test]
fn ab() {
    let mut mt = CloneMultiTrie::default();
    mt.mt_add_ref(b"col-a", b"key-a");
    assert!(mt.mt_contains(b"col-a", b"key-a"));
    mt.mt_add_ref(b"col-b", b"key-b");
    assert!(mt.mt_contains(b"col-b", b"key-b"));
    mt.mt_add_ref(b"col-a", b"key-b");
    assert!(mt.mt_contains(b"col-a", b"key-b"));
    mt.mt_add_ref(b"col-b", b"key-a");
    assert!(mt.mt_contains(b"col-b", b"key-a"));
    mt.mt_remove(b"col-a", b"key-a");
    assert!(!mt.mt_contains(b"col-a", b"key-a"));
    mt.mt_remove(b"col-b", b"key-b");
    assert!(!mt.mt_contains(b"col-b", b"key-b"));
    mt.mt_remove(b"col-a", b"key-b");
    assert!(!mt.mt_contains(b"col-a", b"key-b"));
    assert!(mt.mt_is_empty(b"col-a"));
    mt.mt_remove(b"col-b", b"key-a");
    assert!(!mt.mt_contains(b"col-b", b"key-a"));
    assert!(mt.mt_is_empty(b"col-b"));
}

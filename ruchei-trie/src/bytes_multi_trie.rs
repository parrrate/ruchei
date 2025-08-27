use std::{
    borrow::Borrow,
    collections::{BTreeMap, BTreeSet},
};

use slab::Slab;

use crate::{
    NodeId, Trie,
    multi_trie::{MultiTrie, MultiTrieAddRef, MultiTriePrefix},
};

#[derive(Debug, Default)]
pub struct BytesMultiTrie {
    keys: Trie<BTreeMap<NodeId, usize>>,
    collections: Trie<Slab<NodeId>>,
}

impl MultiTrie<[u8]> for BytesMultiTrie {
    fn remove(&mut self, collection: &[u8], key: &[u8]) {
        let Some((collection, slab)) = self.collections.get_mut(collection) else {
            return;
        };
        let Some((id, map)) = self.keys.get_mut(key) else {
            return;
        };
        let Some(index) = map.remove(&collection) else {
            return;
        };
        slab.remove(index);
        if map.is_empty() {
            self.keys.remove_at(id).expect("invalid state");
        }
        if slab.is_empty() {
            self.collections
                .remove_at(collection)
                .expect("invalid state");
        }
    }

    fn contains(&self, collection: &[u8], key: &[u8]) -> bool {
        let Some((collection, _)) = self.collections.get(collection) else {
            return false;
        };
        let Some((_, map)) = self.keys.get(key) else {
            return false;
        };
        map.contains_key(&collection)
    }

    fn clear(&mut self, collection: &[u8]) {
        let Some((collection, slab)) = self.collections.remove(collection) else {
            return;
        };
        for (index_expected, id) in slab {
            let map = self.keys.try_index_mut(id).expect("invalid state");
            let index_actual = map.remove(&collection).expect("invalid state");
            assert_eq!(index_actual, index_expected);
            if map.is_empty() {
                self.keys.remove_at(id).expect("invalid state");
            }
        }
    }

    fn is_empty(&self, collection: &[u8]) -> bool {
        !self.collections.contains_key(collection)
    }

    fn len(&self, collection: &[u8]) -> usize {
        self.collections
            .get(collection)
            .map(|(_, slab)| slab.len())
            .unwrap_or_default()
    }
}

impl MultiTrieAddRef<[u8]> for BytesMultiTrie {
    fn add_ref(&mut self, collection: &[u8], key: &[u8]) {
        if !self.collections.contains_key(collection) {
            self.collections.insert(collection, Slab::new());
        }
        if self.keys.get(key).is_none() {
            self.keys.insert(key, BTreeMap::new());
        }
        let (collection, slab) = self.collections.get_mut(collection).expect("just inserted");
        let (id, map) = self.keys.get_mut(key).expect("just inserted");
        map.entry(collection).or_insert_with(|| slab.insert(id));
    }
}

impl MultiTriePrefix for BytesMultiTrie {
    type Collection = [u8];

    fn prefix_collect<'a>(
        &'a self,
        suffix: &[u8],
    ) -> impl 'a + IntoIterator<Item: 'a + Borrow<Self::Collection>> {
        self.keys
            .prefix_of(suffix)
            .flat_map(|map| map.keys())
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|id| self.collections.collect_key(id).expect("invalid state"))
    }
}

#[test]
fn ab() {
    let mut mt = BytesMultiTrie::default();
    mt.add_ref(b"col-a", b"key-a");
    assert!(mt.contains(b"col-a", b"key-a"));
    mt.add_ref(b"col-b", b"key-b");
    assert!(mt.contains(b"col-b", b"key-b"));
    mt.add_ref(b"col-a", b"key-b");
    assert!(mt.contains(b"col-a", b"key-b"));
    mt.add_ref(b"col-b", b"key-a");
    assert!(mt.contains(b"col-b", b"key-a"));
    mt.remove(b"col-a", b"key-a");
    assert!(!mt.contains(b"col-a", b"key-a"));
    mt.remove(b"col-b", b"key-b");
    assert!(!mt.contains(b"col-b", b"key-b"));
    mt.remove(b"col-a", b"key-b");
    assert!(!mt.contains(b"col-a", b"key-b"));
    assert!(mt.is_empty(b"col-a"));
    mt.remove(b"col-b", b"key-a");
    assert!(!mt.contains(b"col-b", b"key-a"));
    assert!(mt.is_empty(b"col-b"));
}

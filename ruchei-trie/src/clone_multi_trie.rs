use std::collections::BTreeMap;

use slab::Slab;

use crate::{NodeId, Trie};

pub struct CloneMultiTrie<T> {
    trie: Trie<BTreeMap<T, usize>>,
    map: BTreeMap<T, Slab<NodeId>>,
}

impl<T: Clone + Ord> CloneMultiTrie<T> {
    pub fn add(&mut self, collection: T, key: &[u8]) {
        if !self.map.contains_key(&collection) {
            self.map.insert(collection.clone(), Slab::new());
        }
        if self.trie.get(key).is_none() {
            self.trie.insert(key, BTreeMap::new());
        }
        let slab = self.map.get_mut(&collection).expect("just inserted");
        let (id, map) = self.trie.get_mut(key).expect("just inserted");
        map.entry(collection).or_insert_with(|| slab.insert(id));
    }

    pub fn remove(&mut self, collection: &T, key: &[u8]) {
        let Some(slab) = self.map.get_mut(collection) else {
            return;
        };
        let Some((id, map)) = self.trie.get_mut(key) else {
            return;
        };
        let Some(index) = map.remove(collection) else {
            return;
        };
        slab.remove(index);
        if map.is_empty() {
            self.trie.remove_at(id).expect("invalid state");
        }
        if slab.is_empty() {
            self.map.remove(collection).expect("invalid state");
        }
    }

    pub fn clear(&mut self, collection: &T) {
        let Some(slab) = self.map.remove(collection) else {
            return;
        };
        for (index_expected, id) in slab {
            let map = self.trie.try_index_mut(id).expect("invalid state");
            let index_actual = map.remove(collection).expect("invalid state");
            assert_eq!(index_actual, index_expected);
            if map.is_empty() {
                self.trie.remove_at(id).expect("invalid state");
            }
        }
    }

    pub fn is_empty(&self, collection: &T) -> bool {
        !self.map.contains_key(collection)
    }

    pub fn len(&self, collection: &T) -> usize {
        self.map
            .get(collection)
            .map(|slab| slab.len())
            .unwrap_or_default()
    }
}

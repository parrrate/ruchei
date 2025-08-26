use std::collections::BTreeMap;

use slab::Slab;

use crate::{NodeId, Trie};

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

impl<T: Clone + Ord> CloneMultiTrie<T> {
    pub fn add(&mut self, collection: &T, key: &[u8]) {
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

    pub fn remove(&mut self, collection: &T, key: &[u8]) {
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

    pub fn contains(&mut self, collection: &T, key: &[u8]) -> bool {
        let Some((_, map)) = self.keys.get(key) else {
            return false;
        };
        map.contains_key(collection)
    }

    pub fn clear(&mut self, collection: &T) {
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

    pub fn is_empty(&self, collection: &T) -> bool {
        !self.collections.contains_key(collection)
    }

    pub fn len(&self, collection: &T) -> usize {
        self.collections
            .get(collection)
            .map(|slab| slab.len())
            .unwrap_or_default()
    }
}

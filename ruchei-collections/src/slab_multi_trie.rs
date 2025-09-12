use std::{
    collections::{BTreeMap, BTreeSet},
    ops::{Index, IndexMut},
};

use slab::Slab;

use crate::{
    multi_trie::{MultiTrie, MultiTrieAddOwned, MultiTrieAddRef, MultiTriePrefix},
    nodes::NodeId,
    trie::Trie,
};

pub struct SlabMultiTrie<T> {
    keys: Trie<BTreeMap<usize, usize>>,
    collections: Slab<(T, Slab<NodeId>)>,
}

impl<T> Default for SlabMultiTrie<T> {
    fn default() -> Self {
        Self {
            keys: Default::default(),
            collections: Default::default(),
        }
    }
}

impl<T> MultiTrie<usize> for SlabMultiTrie<T> {
    fn remove(&mut self, collection: &usize, key: &[u8]) {
        let (_, slab) = self
            .collections
            .get_mut(*collection)
            .expect("SMT expects pre-made collections");
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
    }

    fn contains(&self, collection: &usize, key: &[u8]) -> bool {
        let Some((_, map)) = self.keys.get(key) else {
            return false;
        };
        map.contains_key(collection)
    }

    fn clear(&mut self, collection: &usize) {
        let Some((_, slab)) = self.collections.get_mut(*collection) else {
            return;
        };
        for (index_expected, id) in std::mem::take(slab) {
            let map = self.keys.try_index_mut(id).expect("invalid state");
            let index_actual = map.remove(collection).expect("invalid state");
            assert_eq!(index_actual, index_expected);
            if map.is_empty() {
                self.keys.remove_at(id).expect("invalid state");
            }
        }
    }

    fn is_empty(&self, collection: &usize) -> bool {
        self.collections
            .get(*collection)
            .expect("SMT expects pre-made collections")
            .1
            .is_empty()
    }

    fn len(&self, collection: &usize) -> usize {
        self.collections
            .get(*collection)
            .expect("SMT expects pre-made collections")
            .1
            .len()
    }
}

impl<T> MultiTrieAddRef<usize> for SlabMultiTrie<T> {
    fn add_ref(&mut self, collection: &usize, key: &[u8]) {
        self.add_owned(*collection, key);
    }
}

impl<T> MultiTrieAddOwned<usize> for SlabMultiTrie<T> {
    fn add_owned(&mut self, collection: usize, key: &[u8]) {
        if self.keys.get(key).is_none() {
            self.keys.insert(key, BTreeMap::new());
        }
        let (_, slab) = self
            .collections
            .get_mut(collection)
            .expect("SMT expects pre-made collections");
        let (id, map) = self.keys.get_mut(key).expect("just inserted");
        map.entry(collection).or_insert_with(|| slab.insert(id));
    }
}

impl<T> MultiTriePrefix for SlabMultiTrie<T> {
    type Collection = usize;

    fn prefix_collect<'a>(
        &'a self,
        suffix: &[u8],
    ) -> impl 'a + IntoIterator<Item: 'a + std::borrow::Borrow<Self::Collection>> {
        self.keys
            .prefix_of(suffix)
            .flat_map(|map| map.keys())
            .collect::<BTreeSet<_>>()
    }
}

impl<T> SlabMultiTrie<T> {
    pub fn insert(&mut self, value: T) -> usize {
        self.collections.insert((value, Slab::new()))
    }

    pub fn pop(&mut self, collection: usize) -> T {
        self.clear(&collection);
        self.collections.remove(collection).0
    }
}

impl<T> Index<usize> for SlabMultiTrie<T> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        &self.collections[index].0
    }
}

impl<T> IndexMut<usize> for SlabMultiTrie<T> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.collections[index].0
    }
}

use std::{
    collections::{BTreeMap, BTreeSet},
    ops::{Index, IndexMut},
};

use slab::Slab;

use crate::{
    linked_slab::LinkedSlab,
    multi_trie::{MultiTrie, MultiTrieAddOwned, MultiTrieAddRef, MultiTriePrefix},
    nodes::NodeId,
    trie::Trie,
};

pub struct LinkedSlabMultiTrie<T, const N: usize> {
    keys: Trie<BTreeMap<usize, usize>>,
    collections: LinkedSlab<(T, Slab<NodeId>), N>,
}

impl<T, const N: usize> Default for LinkedSlabMultiTrie<T, N> {
    fn default() -> Self {
        Self {
            keys: Default::default(),
            collections: Default::default(),
        }
    }
}

impl<T, const N: usize> MultiTrie<usize> for LinkedSlabMultiTrie<T, N> {
    fn mt_remove(&mut self, collection: &usize, key: &[u8]) {
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

    fn mt_contains(&self, collection: &usize, key: &[u8]) -> bool {
        let Some((_, map)) = self.keys.get(key) else {
            return false;
        };
        map.contains_key(collection)
    }

    fn mt_clear(&mut self, collection: &usize) {
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

    fn mt_is_empty(&self, collection: &usize) -> bool {
        self.collections
            .get(*collection)
            .expect("SMT expects pre-made collections")
            .1
            .is_empty()
    }

    fn mt_len(&self, collection: &usize) -> usize {
        self.collections
            .get(*collection)
            .expect("SMT expects pre-made collections")
            .1
            .len()
    }
}

impl<T, const N: usize> MultiTrieAddRef<usize> for LinkedSlabMultiTrie<T, N> {
    fn mt_add_ref(&mut self, collection: &usize, key: &[u8]) {
        self.mt_add_owned(*collection, key);
    }
}

impl<T, const N: usize> MultiTrieAddOwned<usize> for LinkedSlabMultiTrie<T, N> {
    fn mt_add_owned(&mut self, collection: usize, key: &[u8]) {
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

impl<T, const N: usize> MultiTriePrefix for LinkedSlabMultiTrie<T, N> {
    type Collection = usize;

    fn mt_prefix_collect<'a>(
        &'a self,
        suffix: &[u8],
    ) -> impl 'a + IntoIterator<Item: 'a + std::borrow::Borrow<Self::Collection>> {
        self.keys
            .prefix_of(suffix)
            .flat_map(|map| map.keys())
            .collect::<BTreeSet<_>>()
    }
}

impl<T, const N: usize> LinkedSlabMultiTrie<T, N> {
    pub fn front<const M: usize>(&self) -> Option<usize> {
        self.collections.front::<M>()
    }

    pub fn link_empty<const M: usize>(&self) -> bool {
        self.collections.link_empty::<M>()
    }

    pub fn link_len<const M: usize>(&self) -> usize {
        self.collections.link_len::<M>()
    }

    pub fn link_contains<const M: usize>(&self, key: usize) -> bool {
        self.collections.link_contains::<M>(key)
    }

    pub fn link_push_back<const M: usize>(&mut self, key: usize) -> bool {
        self.collections.link_push_back::<M>(key)
    }

    pub fn link_pop_at<const M: usize>(&mut self, key: usize) -> bool {
        self.collections.link_pop_at::<M>(key)
    }

    pub fn link_pop_front<const M: usize>(&mut self) -> Option<usize> {
        self.collections.link_pop_front::<M>()
    }

    pub fn insert(&mut self, value: T) -> usize {
        self.collections.insert((value, Slab::new()))
    }

    pub fn pop(&mut self, collection: usize) -> T {
        self.mt_clear(&collection);
        self.collections.remove(collection).0
    }

    pub fn try_remove(&mut self, collection: usize) -> Option<T> {
        if self.collections.get(collection).is_some() {
            Some(self.pop(collection))
        } else {
            None
        }
    }

    pub fn vacant_key(&mut self) -> usize {
        self.collections.vacant_key()
    }

    pub fn get_refresh<const M: usize>(&mut self, key: usize) -> &mut T {
        &mut self
            .collections
            .get_refresh::<M>(key)
            .expect("SMT expects pre-made collections")
            .0
    }

    pub fn len(&self) -> usize {
        self.collections.len()
    }

    pub fn is_empty(&self) -> bool {
        self.collections.is_empty()
    }

    pub fn get_mut(&mut self, key: usize) -> Option<&mut T> {
        Some(&mut self.collections.get_mut(key)?.0)
    }

    pub fn link_pops<const M: usize, U, F: FnMut(usize, &mut Self) -> U>(
        &mut self,
        f: F,
    ) -> Pops<'_, T, F, N, M> {
        Pops(self, f)
    }
}

pub struct Pops<'a, T, F, const N: usize, const M: usize>(&'a mut LinkedSlabMultiTrie<T, N>, F);

impl<T, U, F: FnMut(usize, &mut LinkedSlabMultiTrie<T, N>) -> U, const N: usize, const M: usize>
    Iterator for Pops<'_, T, F, N, M>
{
    type Item = U;

    fn next(&mut self) -> Option<Self::Item> {
        let key = self.0.link_pop_front::<M>()?;
        Some(self.1(key, self.0))
    }
}

impl<T, const N: usize> Index<usize> for LinkedSlabMultiTrie<T, N> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        &self.collections[index].0
    }
}

impl<T, const N: usize> IndexMut<usize> for LinkedSlabMultiTrie<T, N> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.collections[index].0
    }
}

#[test]
fn ab() {
    let mut mt = LinkedSlabMultiTrie::<_, 0>::default();
    let col_a = mt.insert(b"col-a");
    let col_b = mt.insert(b"col-b");
    mt.mt_add_ref(&col_a, b"key-a");
    assert!(mt.mt_contains(&col_a, b"key-a"));
    mt.mt_add_ref(&col_b, b"key-b");
    assert!(mt.mt_contains(&col_b, b"key-b"));
    mt.mt_add_ref(&col_a, b"key-b");
    assert!(mt.mt_contains(&col_a, b"key-b"));
    mt.mt_add_ref(&col_b, b"key-a");
    assert!(mt.mt_contains(&col_b, b"key-a"));
    mt.mt_remove(&col_a, b"key-a");
    assert!(!mt.mt_contains(&col_a, b"key-a"));
    mt.mt_remove(&col_b, b"key-b");
    assert!(!mt.mt_contains(&col_b, b"key-b"));
    mt.mt_remove(&col_a, b"key-b");
    assert!(!mt.mt_contains(&col_a, b"key-b"));
    assert!(MultiTrie::mt_is_empty(&mt, &col_a));
    mt.mt_remove(&col_b, b"key-a");
    assert!(!mt.mt_contains(&col_b, b"key-a"));
    assert!(MultiTrie::mt_is_empty(&mt, &col_b));
}

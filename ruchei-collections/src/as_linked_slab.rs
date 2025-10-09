use std::ops::{Index, IndexMut};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SlabKey {
    pub(crate) key: usize,
    pub(crate) ctr: usize,
}

pub trait AsLinkedSlab: Index<SlabKey> + IndexMut<SlabKey> {
    const N: usize;
    type T;

    #[must_use]
    fn front<const M: usize>(&self) -> Option<SlabKey>;
    #[must_use]
    fn link_empty<const M: usize>(&self) -> bool;
    #[must_use]
    fn link_len<const M: usize>(&self) -> usize;
    #[must_use]
    fn link_contains<const M: usize>(&self, key: SlabKey) -> bool;
    fn link_push_back<const M: usize>(&mut self, key: SlabKey) -> bool;
    fn link_push_front<const M: usize>(&mut self, key: SlabKey) -> bool;
    fn link_pop_at<const M: usize>(&mut self, key: SlabKey) -> bool;
    #[must_use]
    fn link_of<const M: usize>(&self, key: Option<SlabKey>) -> (Option<SlabKey>, Option<SlabKey>);
    fn link_insert_between<const M: usize>(&mut self, prev: SlabKey, key: SlabKey, next: SlabKey);
    #[must_use]
    fn insert(&mut self, value: Self::T) -> SlabKey;
    #[must_use]
    fn vacant_key(&mut self) -> SlabKey;
    fn try_remove(&mut self, key: SlabKey) -> Option<Self::T>;
    #[must_use]
    fn contains(&self, key: SlabKey) -> bool;
    #[must_use]
    fn get(&self, key: SlabKey) -> Option<&Self::T>;
    #[must_use]
    fn get_mut(&mut self, key: SlabKey) -> Option<&mut Self::T>;
    #[must_use]
    fn is_empty(&self) -> bool;
    #[must_use]
    fn len(&self) -> usize;
    fn clear(&mut self);

    fn insert_at(&mut self, key: SlabKey, value: Self::T) {
        assert_eq!(self.insert(value), key);
    }

    fn link_pop_front<const M: usize>(&mut self) -> Option<SlabKey> {
        let key = self.front::<M>()?;
        let popped = self.link_pop_at::<M>(key);
        assert!(popped, "key not linked");
        Some(key)
    }

    fn link_insert<const M: usize>(
        &mut self,
        prev: Option<SlabKey>,
        key: SlabKey,
        next: Option<SlabKey>,
    ) {
        match (prev, next) {
            (None, next) => {
                let (_, prev_next) = self.link_of::<M>(None);
                assert_eq!(prev_next, next);
                assert!(self.link_push_front::<M>(key));
            }
            (prev, None) => {
                let (next_prev, _) = self.link_of::<M>(None);
                assert_eq!(next_prev, prev);
                assert!(self.link_push_back::<M>(key));
            }
            (Some(prev), Some(next)) => {
                self.link_insert_between::<M>(prev, key, next);
            }
        }
    }

    fn get_refresh<const M: usize>(&mut self, key: SlabKey) -> Option<&mut Self::T> {
        assert!(M < Self::N);
        self.link_pop_at::<M>(key);
        self.link_push_back::<M>(key);
        self.get_mut(key)
    }

    fn remove(&mut self, key: SlabKey) -> Self::T {
        self.try_remove(key).expect("invalid key")
    }

    fn link_pops<const M: usize, U, F: FnMut(SlabKey, &mut Self) -> U>(
        &mut self,
        f: F,
    ) -> Pops<'_, Self, F, M> {
        assert!(M < Self::N);
        Pops(self, f)
    }
}

#[must_use]
pub struct Pops<'a, L: ?Sized, F, const M: usize>(&'a mut L, F);

impl<L: AsLinkedSlab, U, F: FnMut(SlabKey, &mut L) -> U, const M: usize> Iterator
    for Pops<'_, L, F, M>
{
    type Item = U;

    fn next(&mut self) -> Option<Self::Item> {
        let key = self.0.link_pop_front::<M>()?;
        Some(self.1(key, self.0))
    }
}

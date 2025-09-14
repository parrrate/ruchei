use std::ops::{Index, IndexMut};

pub trait AsLinkedSlab: Index<usize> + IndexMut<usize> {
    const N: usize;
    type T;

    fn front<const M: usize>(&self) -> Option<usize>;
    fn link_empty<const M: usize>(&self) -> bool;
    fn link_len<const M: usize>(&self) -> usize;
    fn link_contains<const M: usize>(&self, key: usize) -> bool;
    fn link_push_back<const M: usize>(&mut self, key: usize) -> bool;
    fn link_pop_at<const M: usize>(&mut self, key: usize) -> bool;
    fn link_pop_front<const M: usize>(&mut self) -> Option<usize>;
    fn link_of<const M: usize>(&self, key: Option<usize>) -> (Option<usize>, Option<usize>);
    fn insert(&mut self, value: Self::T) -> usize;
    fn vacant_key(&mut self) -> usize;
    fn try_remove(&mut self, key: usize) -> Option<Self::T>;
    fn get(&self, key: usize) -> Option<&Self::T>;
    fn get_mut(&mut self, key: usize) -> Option<&mut Self::T>;
    fn is_empty(&self) -> bool;
    fn len(&self) -> usize;

    fn get_refresh<const M: usize>(&mut self, key: usize) -> Option<&mut Self::T> {
        assert!(M < Self::N);
        self.link_pop_at::<M>(key);
        self.link_push_back::<M>(key);
        self.get_mut(key)
    }

    fn remove(&mut self, key: usize) -> Self::T {
        self.try_remove(key).expect("invalid key")
    }

    fn link_pops<const M: usize, U, F: FnMut(usize, &mut Self) -> U>(
        &mut self,
        f: F,
    ) -> Pops<'_, Self, F, M> {
        assert!(M < Self::N);
        Pops(self, f)
    }
}

pub struct Pops<'a, L: ?Sized, F, const M: usize>(&'a mut L, F);

impl<L: AsLinkedSlab, U, F: FnMut(usize, &mut L) -> U, const M: usize> Iterator
    for Pops<'_, L, F, M>
{
    type Item = U;

    fn next(&mut self) -> Option<Self::Item> {
        let key = self.0.link_pop_front::<M>()?;
        Some(self.1(key, self.0))
    }
}

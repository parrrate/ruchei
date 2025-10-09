use std::ops::{Index, IndexMut};

use slab::Slab;

use crate::as_linked_slab::{AsLinkedSlab, SlabKey};

#[derive(Debug, PartialEq, Eq)]
#[must_use]
struct Link {
    prev: usize,
    next: usize,
}

impl Default for Link {
    fn default() -> Self {
        Self::EMPTY
    }
}

const EMPTY: usize = usize::MAX;

impl Link {
    const EMPTY: Self = Self::new(EMPTY);

    const fn new(key: usize) -> Self {
        Self {
            prev: key,
            next: key,
        }
    }
}

#[derive(Debug)]
#[must_use]
struct Value<T, const N: usize> {
    ctr: usize,
    value: T,
    links: [Option<Link>; N],
}

impl<T, const N: usize> Value<T, N> {
    fn new(ctr: usize, value: T) -> Self {
        const NO_LINK: Option<Link> = None;
        Self {
            ctr,
            value,
            links: [NO_LINK; N],
        }
    }
}

#[derive(Debug)]
pub struct LinkedSlab<T, const N: usize> {
    ctr: usize,
    slab: Slab<Value<T, N>>,
    links: [Link; N],
    lens: [usize; N],
}

impl<T, const N: usize> Default for LinkedSlab<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const N: usize> LinkedSlab<T, N> {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            ctr: 0,
            slab: Slab::new(),
            links: [Link::EMPTY; N],
            lens: [0; N],
        }
    }

    #[must_use]
    fn prev_next(&mut self, n: usize, prev: usize, next: usize) -> (&mut usize, &mut usize) {
        assert!(n < N);
        assert_ne!(self.lens[n], 0);
        let (prev, next) = self
            .slab
            .get2_mut(prev, next)
            .expect("prev or next not found");
        let prev_next = &mut prev.links[n].as_mut().expect("prev not linked").next;
        let next_prev = &mut next.links[n].as_mut().expect("next not linked").prev;
        (prev_next, next_prev)
    }

    #[inline(always)]
    fn unlink(&mut self, link: Link, n: usize, key: usize) {
        assert!(n < N);
        assert_ne!(self.lens[n], 0);
        self.lens[n] -= 1;
        assert!(self.lens[n] <= self.len());
        let (prev_next, next_prev) = match link {
            Link::EMPTY => {
                assert_eq!(self.lens[n], 0);
                let prev_next = &mut self.links[n].next;
                let next_prev = &mut self.links[n].prev;
                (prev_next, next_prev)
            }
            Link { prev: EMPTY, next } => {
                assert_ne!(self.lens[n], 0);
                let next = self.slab.get_mut(next).expect("next not found");
                let next_prev = &mut next.links[n].as_mut().expect("next not linked").prev;
                let prev_next = &mut self.links[n].next;
                (prev_next, next_prev)
            }
            Link { prev, next: EMPTY } => {
                assert_ne!(self.lens[n], 0);
                let prev = self.slab.get_mut(prev).expect("prev not found");
                let prev_next = &mut prev.links[n].as_mut().expect("prev not linked").next;
                let next_prev = &mut self.links[n].prev;
                (prev_next, next_prev)
            }
            Link { prev, next } => self.prev_next(n, prev, next),
        };
        assert_eq!(*prev_next, key);
        assert_eq!(*next_prev, key);
        *prev_next = link.next;
        *next_prev = link.prev;
    }

    #[must_use]
    fn linkable<const M: usize>(&self, SlabKey { key, ctr }: SlabKey) -> bool {
        assert!(M < N);
        if let Some(value) = self.slab.get(key)
            && value.ctr == ctr
        {
            value.links[M].is_none()
        } else {
            false
        }
    }

    #[must_use]
    fn link<const M: usize>(&self) -> Option<(usize, usize)> {
        assert!(M < N);
        match self.links[M] {
            Link::EMPTY => None,
            Link { prev: EMPTY, .. } => panic!("only next is linked"),
            Link { next: EMPTY, .. } => panic!("only prev is linked"),
            Link { prev, next } => Some((prev, next)),
        }
    }

    #[must_use]
    fn item_link<const M: usize>(&self, key: usize) -> (Option<usize>, Option<usize>) {
        assert!(M < N);
        let Link { prev, next } = *self.slab[key].links[M].as_ref().expect("not linked");
        let prev = (prev != EMPTY).then_some(prev);
        let next = (next != EMPTY).then_some(next);
        (prev, next)
    }

    #[must_use]
    fn slab_key(&self, key: usize) -> SlabKey {
        SlabKey {
            key,
            ctr: self.slab[key].ctr,
        }
    }

    #[must_use]
    fn key(&self, key: SlabKey) -> usize {
        assert_eq!(self.slab[key.key].ctr, key.ctr);
        key.key
    }
}

impl<T, const N: usize> AsLinkedSlab for LinkedSlab<T, N> {
    const N: usize = N;
    type T = T;

    fn front<const M: usize>(&self) -> Option<SlabKey> {
        assert!(M < N);
        let (_, next) = self.link::<M>()?;
        let next = self.slab_key(next);
        Some(next)
    }

    fn link_empty<const M: usize>(&self) -> bool {
        assert!(M < N);
        self.link::<M>().is_none()
    }

    fn link_len<const M: usize>(&self) -> usize {
        assert!(M < N);
        self.lens[M]
    }

    fn link_contains<const M: usize>(&self, SlabKey { key, ctr }: SlabKey) -> bool {
        assert!(M < N);
        if let Some(value) = self.slab.get(key)
            && value.ctr == ctr
        {
            value.links[M].is_some()
        } else {
            false
        }
    }

    fn link_push_back<const M: usize>(&mut self, key: SlabKey) -> bool {
        assert!(M < N);
        if !self.linkable::<M>(key) {
            return false;
        }
        let key = self.key(key);
        assert!(self.lens[M] < self.len());
        let link = match self.link::<M>() {
            None => {
                assert_eq!(self.lens[M], 0);
                self.links[M] = Link::new(key);
                Link::EMPTY
            }
            Some((prev, _)) => {
                assert_ne!(self.lens[M], 0);
                self.slab.get_mut(prev).expect("last not found").links[M]
                    .as_mut()
                    .expect("last not linked")
                    .next = key;
                self.links[M].prev = key;
                Link { prev, next: EMPTY }
            }
        };
        let link_ref = &mut self.slab.get_mut(key).expect("key not found").links[M];
        assert!(link_ref.is_none());
        *link_ref = Some(link);
        self.lens[M] += 1;
        true
    }

    fn link_push_front<const M: usize>(&mut self, key: SlabKey) -> bool {
        assert!(M < N);
        if !self.linkable::<M>(key) {
            return false;
        }
        let key = self.key(key);
        assert!(self.lens[M] < self.len());
        let link = match self.link::<M>() {
            None => {
                assert_eq!(self.lens[M], 0);
                self.links[M] = Link::new(key);
                Link::EMPTY
            }
            Some((_, next)) => {
                assert_ne!(self.lens[M], 0);
                self.slab.get_mut(next).expect("first not found").links[M]
                    .as_mut()
                    .expect("first not linked")
                    .prev = key;
                self.links[M].next = key;
                Link { next, prev: EMPTY }
            }
        };
        let link_ref = &mut self.slab.get_mut(key).expect("key not found").links[M];
        assert!(link_ref.is_none());
        *link_ref = Some(link);
        self.lens[M] += 1;
        true
    }

    fn link_pop_at<const M: usize>(&mut self, key: SlabKey) -> bool {
        assert!(M < N);
        let key = self.key(key);
        if let Some(link) = self.slab.get_mut(key).expect("key not found").links[M].take() {
            self.unlink(link, M, key);
            true
        } else {
            false
        }
    }

    fn link_of<const M: usize>(&self, key: Option<SlabKey>) -> (Option<SlabKey>, Option<SlabKey>) {
        match key {
            Some(key) => {
                assert_eq!(self.slab[key.key].ctr, key.ctr);
                let (prev, next) = self.item_link::<M>(key.key);
                let prev = prev.map(|prev| self.slab_key(prev));
                let next = next.map(|next| self.slab_key(next));
                (prev, next)
            }
            None => {
                let Some((prev, next)) = self.link::<M>() else {
                    return (None, None);
                };
                let prev = self.slab_key(prev);
                let next = self.slab_key(next);
                (Some(prev), Some(next))
            }
        }
    }

    fn link_insert_between<const M: usize>(&mut self, prev: SlabKey, key: SlabKey, next: SlabKey) {
        assert!(self.slab[key.key].links[M].is_none(), "already linked");
        let prev = self.key(prev);
        let key = self.key(key);
        let next = self.key(next);
        let (prev_next, next_prev) = self.prev_next(M, prev, next);
        *prev_next = key;
        *next_prev = key;
        self.slab[key].links[M] = Some(Link { prev, next });
        self.lens[M] += 1;
    }

    fn insert(&mut self, value: Self::T) -> SlabKey {
        let ctr = self.ctr;
        let key = self.slab.insert(Value::new(ctr, value));
        self.ctr += 1;
        SlabKey { key, ctr }
    }

    fn vacant_key(&mut self) -> SlabKey {
        let key = self.slab.vacant_key();
        let ctr = self.ctr;
        SlabKey { key, ctr }
    }

    fn try_remove(&mut self, key: SlabKey) -> Option<Self::T> {
        if self
            .slab
            .get(key.key)
            .is_some_and(|value| value.ctr != key.ctr)
        {
            return None;
        }
        let value = self.slab.try_remove(key.key)?;
        for (n, link) in value.links.into_iter().enumerate() {
            if let Some(link) = link {
                self.unlink(link, n, key.key);
            }
        }
        Some(value.value)
    }

    fn contains(&self, key: SlabKey) -> bool {
        self.slab
            .get(key.key)
            .is_some_and(|value| value.ctr == key.ctr)
    }

    fn get(&self, key: SlabKey) -> Option<&Self::T> {
        let value = self.slab.get(key.key)?;
        if value.ctr == key.ctr {
            Some(&value.value)
        } else {
            None
        }
    }

    fn get_mut(&mut self, key: SlabKey) -> Option<&mut Self::T> {
        let value = self.slab.get_mut(key.key)?;
        if value.ctr == key.ctr {
            Some(&mut value.value)
        } else {
            None
        }
    }

    fn is_empty(&self) -> bool {
        self.slab.is_empty()
    }

    fn len(&self) -> usize {
        self.slab.len()
    }

    fn clear(&mut self) {
        self.slab.clear();
        self.links = [Link::EMPTY; _];
        self.lens = [0; _];
    }
}

impl<T, const N: usize> Index<SlabKey> for LinkedSlab<T, N> {
    type Output = T;

    fn index(&self, key: SlabKey) -> &Self::Output {
        let key = self.key(key);
        &self.slab[key].value
    }
}

impl<T, const N: usize> IndexMut<SlabKey> for LinkedSlab<T, N> {
    fn index_mut(&mut self, key: SlabKey) -> &mut Self::Output {
        let key = self.key(key);
        &mut self.slab[key].value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test() {
        let mut slab = LinkedSlab::<i32, 3>::new();
        assert!(slab.is_empty());
        let a = slab.insert(123);
        let b = slab.insert(456);
        assert!(!slab.is_empty());
        slab.link_push_back::<0>(a);
        slab.link_push_back::<0>(b);
        slab.link_push_back::<1>(b);
        slab.link_push_back::<1>(a);
        slab.link_push_back::<2>(a);
        slab.link_push_back::<2>(b);
        assert_eq!(slab.link_pop_front::<0>().unwrap(), a);
        assert_eq!(slab.link_pop_front::<0>().unwrap(), b);
        assert!(slab.link_pop_front::<0>().is_none());
        assert_eq!(slab.link_pop_front::<1>().unwrap(), b);
        assert_eq!(slab.link_pop_front::<1>().unwrap(), a);
        assert!(slab.link_pop_front::<1>().is_none());
        slab.try_remove(a).unwrap();
        assert_eq!(slab.link_pop_front::<2>().unwrap(), b);
        assert!(slab.link_pop_front::<2>().is_none());
        assert!(slab.get_mut(a).is_none());
        assert_eq!(*slab.get_mut(b).unwrap(), 456);
        slab.try_remove(b).unwrap();
        assert!(slab.is_empty());
    }

    #[test]
    fn test_insert_between() {
        let mut slab = LinkedSlab::<i32, 1>::new();
        let a = slab.insert(1);
        let b = slab.insert(2);
        let c = slab.insert(3);
        slab.link_push_front::<0>(c);
        slab.link_push_front::<0>(a);
        slab.link_insert_between::<0>(a, b, c);
        assert_eq!(slab.link_pop_front::<0>().unwrap(), a);
        assert_eq!(slab.link_pop_front::<0>().unwrap(), b);
        assert_eq!(slab.link_pop_front::<0>().unwrap(), c);
        assert!(slab.link_empty::<0>());
    }
}

#[cfg(kani)]
mod verification {
    use super::*;

    #[kani::proof]
    pub fn check() {
        let mut slab = LinkedSlab::<i32, 3>::new();
        assert!(slab.is_empty());
        let va = kani::any();
        let vb = kani::any();
        kani::assume(va != vb);
        let a = slab.insert(va);
        assert!(!slab.is_empty());
        assert_eq!(slab.len(), 1);
        let b = slab.insert(vb);
        assert!(!slab.is_empty());
        assert_ne!(a, b);
        assert_eq!(slab.len(), 2);
        assert_ne!(slab.get_mut(a).copied(), slab.get_mut(b).copied());
        slab.link_push_back::<0>(a);
        slab.link_push_back::<0>(b);
        slab.link_push_back::<1>(b);
        slab.link_push_back::<1>(a);
        slab.link_push_back::<2>(a);
        slab.link_push_back::<2>(b);
        assert_eq!(slab.link_pop_front::<0>().unwrap(), a);
        assert_eq!(slab.link_pop_front::<0>().unwrap(), b);
        assert!(slab.link_pop_front::<0>().is_none());
        assert_eq!(slab.link_pop_front::<1>().unwrap(), b);
        assert_eq!(slab.link_pop_front::<1>().unwrap(), a);
        assert!(slab.link_pop_front::<1>().is_none());
        assert_eq!(slab.remove(a).unwrap(), va);
        assert_eq!(slab.link_pop_front::<2>().unwrap(), b);
        assert!(slab.link_pop_front::<2>().is_none());
        assert!(slab.get_mut(a).is_none());
        assert_eq!(*slab.get_mut(b).unwrap(), vb);
        assert_eq!(slab.remove(b).unwrap(), vb);
        assert!(slab.is_empty());
    }
}

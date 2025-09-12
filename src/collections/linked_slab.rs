use slab::Slab;

#[derive(Debug, PartialEq, Eq)]
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
struct Value<T, const N: usize> {
    value: T,
    links: [Option<Link>; N],
}

impl<T, const N: usize> Value<T, N> {
    fn new(value: T) -> Self {
        const NO_LINK: Option<Link> = None;
        Self {
            value,
            links: [NO_LINK; N],
        }
    }
}

#[derive(Debug)]
pub(crate) struct LinkedSlab<T, const N: usize> {
    slab: Slab<Value<T, N>>,
    links: [Link; N],
    lens: [usize; N],
}

impl<T, const N: usize> Default for LinkedSlab<T, N> {
    fn default() -> Self {
        Self {
            slab: Default::default(),
            links: [Link::EMPTY; N],
            lens: [0; N],
        }
    }
}

impl<T, const N: usize> LinkedSlab<T, N> {
    #[must_use]
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::default()
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
            Link { prev, next } => {
                assert_ne!(self.lens[n], 0);
                let (prev, next) = self
                    .slab
                    .get2_mut(prev, next)
                    .expect("prev or next not found");
                let prev_next = &mut prev.links[n].as_mut().expect("prev not linked").next;
                let next_prev = &mut next.links[n].as_mut().expect("next not linked").prev;
                (prev_next, next_prev)
            }
        };
        assert_eq!(*prev_next, key);
        assert_eq!(*next_prev, key);
        *prev_next = link.next;
        *next_prev = link.prev;
    }

    fn linkable<const M: usize>(&self, key: usize) -> bool {
        assert!(M < N);
        if let Some(value) = self.slab.get(key) {
            value.links[M].is_none()
        } else {
            false
        }
    }

    fn link<const M: usize>(&self) -> Option<(usize, usize)> {
        assert!(M < N);
        match self.links[M] {
            Link::EMPTY => None,
            Link { prev: EMPTY, .. } => panic!("only next is linked"),
            Link { next: EMPTY, .. } => panic!("only prev is linked"),
            Link { prev, next } => Some((prev, next)),
        }
    }

    pub(crate) fn front<const M: usize>(&self) -> Option<usize> {
        assert!(M < N);
        let (_, next) = self.link::<M>()?;
        Some(next)
    }

    pub(crate) fn link_empty<const M: usize>(&self) -> bool {
        assert!(M < N);
        self.link::<M>().is_none()
    }

    pub(crate) fn link_len<const M: usize>(&self) -> usize {
        assert!(M < N);
        self.lens[M]
    }

    pub(crate) fn link_contains<const M: usize>(&self, key: usize) -> bool {
        assert!(M < N);
        if let Some(value) = self.slab.get(key) {
            value.links[M].is_some()
        } else {
            false
        }
    }

    pub(crate) fn link_push_back<const M: usize>(&mut self, key: usize) -> bool {
        assert!(M < N);
        if !self.linkable::<M>(key) {
            return false;
        }
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

    pub(crate) fn link_pop_at<const M: usize>(&mut self, key: usize) -> bool {
        assert!(M < N);
        if let Some(link) = self.slab.get_mut(key).expect("key not found").links[M].take() {
            self.unlink(link, M, key);
            true
        } else {
            false
        }
    }

    pub(crate) fn link_pop_front<const M: usize>(&mut self) -> Option<usize> {
        assert!(M < N);
        let key = self.front::<M>()?;
        let popped = self.link_pop_at::<M>(key);
        assert!(popped, "key not linked");
        Some(key)
    }

    pub(crate) fn link_pops<const M: usize, U, F: FnMut(usize, &mut Self) -> U>(
        &mut self,
        f: F,
    ) -> Pops<'_, T, F, N, M> {
        Pops(self, f)
    }

    pub(crate) fn insert(&mut self, value: T) -> usize {
        self.slab.insert(Value::new(value))
    }

    pub(crate) fn vacant_key(&mut self) -> usize {
        self.slab.vacant_key()
    }

    pub(crate) fn remove(&mut self, key: usize) -> Option<T> {
        let value = self.slab.try_remove(key)?;
        for (n, link) in value.links.into_iter().enumerate() {
            if let Some(link) = link {
                self.unlink(link, n, key);
            }
        }
        Some(value.value)
    }

    pub(crate) fn get_mut(&mut self, key: usize) -> Option<&mut T> {
        Some(&mut self.slab.get_mut(key)?.value)
    }

    pub(crate) fn get_refresh<const M: usize>(&mut self, key: usize) -> Option<&mut T> {
        self.link_pop_at::<M>(key);
        self.link_push_back::<M>(key);
        self.get_mut(key)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.slab.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        self.slab.len()
    }
}

pub(crate) struct Pops<'a, T, F, const N: usize, const M: usize>(&'a mut LinkedSlab<T, N>, F);

impl<T, U, F: FnMut(usize, &mut LinkedSlab<T, N>) -> U, const N: usize, const M: usize> Iterator
    for Pops<'_, T, F, N, M>
{
    type Item = U;

    fn next(&mut self) -> Option<Self::Item> {
        let key = self.0.link_pop_front::<M>()?;
        Some(self.1(key, self.0))
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
        slab.remove(a).unwrap();
        assert_eq!(slab.link_pop_front::<2>().unwrap(), b);
        assert!(slab.link_pop_front::<2>().is_none());
        assert!(slab.get_mut(a).is_none());
        assert_eq!(*slab.get_mut(b).unwrap(), 456);
        slab.remove(b).unwrap();
        assert!(slab.is_empty());
    }
}

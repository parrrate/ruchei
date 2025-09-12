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
}

impl<T, const N: usize> Default for LinkedSlab<T, N> {
    fn default() -> Self {
        Self {
            slab: Default::default(),
            links: [Link::EMPTY; N],
        }
    }
}

impl<T, const N: usize> LinkedSlab<T, N> {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn unlink(&mut self, link: Link, n: usize) {
        assert!(n < N);
        let (prev, next) = match link {
            Link::EMPTY => {
                self.links[n] = Link::EMPTY;
                return;
            }
            Link { prev: EMPTY, next } => {
                let next = self.slab.get_mut(next).expect("next not found");
                let next = next.links[n].as_mut().expect("next not linked");
                let prev = &mut self.links[n];
                (prev, next)
            }
            Link { prev, next: EMPTY } => {
                let prev = self.slab.get_mut(prev).expect("prev not found");
                let prev = prev.links[n].as_mut().expect("prev not linked");
                let next = &mut self.links[n];
                (prev, next)
            }
            Link { prev, next } => {
                let (prev, next) = self
                    .slab
                    .get2_mut(prev, next)
                    .expect("prev or next not found");
                let prev = prev.links[n].as_mut().expect("prev not linked");
                let next = next.links[n].as_mut().expect("next not linked");
                (prev, next)
            }
        };
        prev.next = link.next;
        next.prev = link.prev;
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

    fn first<const M: usize>(&mut self) -> Option<usize> {
        assert!(M < N);
        let (_, next) = self.link::<M>()?;
        Some(next)
    }

    pub(crate) fn link_push<const M: usize>(&mut self, key: usize) {
        assert!(M < N);
        if !self.linkable::<M>(key) {
            return;
        }
        let link = match self.link::<M>() {
            None => {
                self.links[M] = Link::new(key);
                Link::EMPTY
            }
            Some((prev, _)) => {
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
    }

    pub(crate) fn link_pop<const M: usize>(&mut self) -> Option<usize> {
        assert!(M < N);
        let key = self.first::<M>()?;
        let link = self.slab.get_mut(key).expect("key not found").links[M]
            .take()
            .expect("key not linked");
        self.unlink(link, M);
        Some(key)
    }

    pub(crate) fn insert(&mut self, value: T) -> usize {
        self.slab.insert(Value::new(value))
    }

    pub(crate) fn remove(&mut self, key: usize) -> Option<T> {
        let value = self.slab.try_remove(key)?;
        for (n, link) in value.links.into_iter().enumerate() {
            if let Some(link) = link {
                self.unlink(link, n);
            }
        }
        Some(value.value)
    }

    pub(crate) fn get_mut(&mut self, key: usize) -> Option<&mut T> {
        Some(&mut self.slab.get_mut(key)?.value)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.slab.is_empty()
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
        slab.link_push::<0>(a);
        slab.link_push::<0>(b);
        slab.link_push::<1>(b);
        slab.link_push::<1>(a);
        slab.link_push::<2>(a);
        slab.link_push::<2>(b);
        assert_eq!(slab.link_pop::<0>().unwrap(), a);
        assert_eq!(slab.link_pop::<0>().unwrap(), b);
        assert!(slab.link_pop::<0>().is_none());
        assert_eq!(slab.link_pop::<1>().unwrap(), b);
        assert_eq!(slab.link_pop::<1>().unwrap(), a);
        assert!(slab.link_pop::<1>().is_none());
        slab.remove(a).unwrap();
        assert_eq!(slab.link_pop::<2>().unwrap(), b);
        assert!(slab.link_pop::<2>().is_none());
        assert!(slab.get_mut(a).is_none());
        assert_eq!(*slab.get_mut(b).unwrap(), 456);
        slab.remove(b).unwrap();
        assert!(slab.is_empty());
    }
}

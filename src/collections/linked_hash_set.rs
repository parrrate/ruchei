/// Based on <https://docs.rs/linked_hash_set/0.1.4/src/linked_hash_set/lib.rs.html>
use std::{
    borrow::Borrow,
    fmt::{Debug, Formatter},
    hash::Hash,
};

use linked_hash_map::{Keys, LinkedHashMap};

pub(crate) struct LinkedHashSet<T> {
    map: LinkedHashMap<T, ()>,
}

pub(crate) struct IntoIter<T>(linked_hash_map::IntoIter<T, ()>);

impl<T> Iterator for IntoIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|(k, ())| k)
    }
}

impl<T: Hash + Eq> IntoIterator for LinkedHashSet<T> {
    type Item = T;

    type IntoIter = IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        IntoIter(self.map.into_iter())
    }
}

impl<T: Hash + Eq> Default for LinkedHashSet<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Hash + Eq> LinkedHashSet<T> {
    #[inline]
    pub(crate) fn new() -> Self {
        Self {
            map: LinkedHashMap::new(),
        }
    }

    pub(crate) fn iter(&self) -> Iter<'_, T> {
        Iter {
            iter: self.map.keys(),
        }
    }

    #[inline]
    pub(crate) fn drain(&mut self) -> Drain<T> {
        Drain {
            drain: self.map.drain(),
        }
    }

    pub(crate) fn insert_if_absent(&mut self, value: T) -> bool {
        if !self.map.contains_key(&value) {
            self.map.insert(value, ()).is_none()
        } else {
            false
        }
    }

    pub(crate) fn remove<Q: ?Sized + Hash + Eq>(&mut self, value: &Q) -> bool
    where
        T: Borrow<Q>,
    {
        self.map.remove(value).is_some()
    }

    pub(crate) fn pop_front(&mut self) -> Option<T> {
        self.map.pop_front().map(|(k, ())| k)
    }
}

impl<T: Eq + Hash + Debug> Debug for LinkedHashSet<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

impl<T: Eq + Hash> Extend<T> for LinkedHashSet<T> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        self.map.extend(iter.into_iter().map(|k| (k, ())))
    }
}

pub(crate) struct Iter<'a, T> {
    iter: Keys<'a, T, ()>,
}

pub(crate) struct Drain<'a, T> {
    drain: linked_hash_map::Drain<'a, T, ()>,
}

impl<'a, T> Clone for Iter<'a, T> {
    fn clone(&self) -> Iter<'a, T> {
        Iter {
            iter: self.iter.clone(),
        }
    }
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<&'a T> {
        self.iter.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl<T> ExactSizeIterator for Iter<'_, T> {
    fn len(&self) -> usize {
        self.iter.len()
    }
}

impl<T: Debug> Debug for Iter<'_, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.clone()).finish()
    }
}

impl<T> Iterator for Drain<'_, T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.drain.next().map(|(k, ())| k)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.drain.size_hint()
    }
}

impl<T> ExactSizeIterator for Drain<'_, T> {
    fn len(&self) -> usize {
        self.drain.len()
    }
}

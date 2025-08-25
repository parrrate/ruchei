use std::collections::BTreeMap;

/// Has `value` and/or one of its descendants has `value`
struct Node<T> {
    value: Option<T>,
    children: BTreeMap<u8, (Vec<u8>, Self)>,
}

fn common_prefix<'a, 'b>(l: &'a [u8], r: &'b [u8]) -> (&'a [u8], &'a [u8], &'b [u8]) {
    let len = l.iter().zip(r).filter(|(l, r)| l == r).count();
    (&l[..len], &l[len..], &r[len..])
}

impl<T> Node<T> {
    fn new(value: Option<T>, children: BTreeMap<u8, (Vec<u8>, Self)>) -> Self {
        assert!(value.is_some() || !children.is_empty());
        Self { value, children }
    }

    fn new_value(value: T) -> Self {
        Self::new(Some(value), BTreeMap::new())
    }

    fn new_children(children: BTreeMap<u8, (Vec<u8>, Self)>) -> Self {
        assert!(!children.is_empty());
        Self::new(None, children)
    }

    fn new_child(first: u8, rest: &[u8], node: Self) -> Self {
        Self::new_children([(first, (rest.into(), node))].into())
    }

    fn get<'a>(&'a self, key: &[u8]) -> Option<&'a T> {
        let Some((first, rest)) = key.split_first() else {
            return self.value.as_ref();
        };
        let (prefix, node) = self.children.get(first)?;
        let key = {
            let sub_key = rest.strip_prefix(prefix.as_slice())?;
            assert!(sub_key.len() < key.len());
            sub_key
        };
        node.get(key)
    }

    fn get_mut<'a>(&'a mut self, key: &[u8]) -> Option<&'a mut T> {
        let Some((first, rest)) = key.split_first() else {
            return self.value.as_mut();
        };
        let (prefix, node) = self.children.get_mut(first)?;
        let key = {
            let sub_key = rest.strip_prefix(prefix.as_slice())?;
            assert!(sub_key.len() < key.len());
            sub_key
        };
        node.get_mut(key)
    }

    fn insert(&mut self, key: &[u8], value: T) -> Option<T> {
        let Some((first, rest)) = key.split_first() else {
            return self.value.replace(value);
        };
        let Some((prefix, node)) = self.children.get_mut(first) else {
            self.children
                .insert(*first, (rest.into(), Self::new_value(value)));
            return None;
        };
        if let Some(sub_key) = rest.strip_prefix(prefix.as_slice()) {
            assert!(sub_key.len() < key.len());
            let key = sub_key;
            return node.insert(key, value);
        }
        if let Some(old_sub) = prefix.strip_prefix(rest) {
            assert!(!old_sub.is_empty());
            let (old_first, old_rest) = old_sub.split_first().expect("not an empty string");
            let new_node = Self::new_value(value);
            let old_node = std::mem::replace(node, new_node);
            node.children
                .insert(*old_first, (old_rest.into(), old_node));
            prefix.truncate(rest.len());
            prefix.copy_from_slice(rest);
        } else {
            let (common, new, old) = common_prefix(rest, prefix);
            assert!(!new.is_empty());
            assert!(!old.is_empty());
            let (new_first, new_rest) = new.split_first().expect("not an empty string");
            let (old_first, old_rest) = old.split_first().expect("not an empty string");
            let new_node = Self::new_value(value);
            let common_node = Self::new_child(*new_first, new_rest, new_node);
            let old_node = std::mem::replace(node, common_node);
            node.children
                .insert(*old_first, (old_rest.into(), old_node));
            prefix.truncate(common.len());
            prefix.copy_from_slice(common);
        }
        None
    }

    fn is_empty(&self) -> bool {
        self.value.is_none() && self.children.is_empty()
    }

    fn is_collapsible(&self) -> bool {
        self.value.is_none() && self.children.len() < 2
    }

    fn collapse(&mut self) -> Option<(u8, (Vec<u8>, Self))> {
        assert!(self.is_collapsible());
        self.children.pop_first()
    }

    fn remove(&mut self, key: &[u8]) -> Option<T> {
        let Some((first, rest)) = key.split_first() else {
            return self.value.take();
        };
        let (prefix, node) = self.children.get_mut(first)?;
        let key = {
            let sub_key = rest.strip_prefix(prefix.as_slice())?;
            assert!(sub_key.len() < key.len());
            sub_key
        };
        let value = node.remove(key)?;
        if node.is_collapsible() {
            if let Some((inner_first, (mut inner_prefix, inner_node))) = node.collapse() {
                prefix.push(inner_first);
                prefix.append(&mut inner_prefix);
                *node = inner_node;
            } else {
                assert!(node.is_empty());
                self.children.remove(first);
            }
        }
        Some(value)
    }
}

pub struct Trie<T> {
    root: Option<Node<T>>,
}

impl<T> Default for Trie<T> {
    fn default() -> Self {
        Self {
            root: Default::default(),
        }
    }
}

impl<T> Trie<T> {
    pub fn get<'a>(&'a self, key: &[u8]) -> Option<&'a T> {
        self.root.as_ref()?.get(key)
    }

    pub fn get_mut<'a>(&'a mut self, key: &[u8]) -> Option<&'a mut T> {
        self.root.as_mut()?.get_mut(key)
    }

    pub fn insert(&mut self, key: &[u8], value: T) -> Option<T> {
        if let Some(node) = self.root.as_mut() {
            return node.insert(key, value);
        }
        let node = Node::new_value(value);
        self.root = Some(if let Some((first, rest)) = key.split_first() {
            Node::new_child(*first, rest, node)
        } else {
            node
        });
        None
    }

    pub fn remove(&mut self, key: &[u8]) -> Option<T> {
        let node = self.root.as_mut()?;
        let value = node.remove(key)?;
        if node.is_empty() {
            self.root.take();
        }
        Some(value)
    }

    pub fn prefix_of<'a, 'b>(&'a self, suffix: &'b [u8]) -> PrefixOf<'a, 'b, T> {
        PrefixOf {
            node: self.root.as_ref(),
            suffix,
        }
    }

    pub fn prefix_of_mut<'a, 'b>(&'a mut self, suffix: &'b [u8]) -> PrefixOfMut<'a, 'b, T> {
        PrefixOfMut {
            node: self.root.as_mut(),
            suffix,
        }
    }
}

pub struct PrefixOf<'a, 'b, T> {
    node: Option<&'a Node<T>>,
    suffix: &'b [u8],
}

impl<'a, 'b, T> Iterator for PrefixOf<'a, 'b, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let node = self.node.take()?;
            let value = node.value.as_ref();
            if let Some((first, rest)) = self.suffix.split_first()
                && let Some((prefix, node)) = node.children.get(first)
                && let Some(suffix) = rest.strip_prefix(prefix.as_slice())
            {
                assert!(suffix.len() < self.suffix.len());
                self.node = Some(node);
                self.suffix = suffix;
            }
            if value.is_some() {
                break value;
            }
        }
    }
}

pub struct PrefixOfMut<'a, 'b, T> {
    node: Option<&'a mut Node<T>>,
    suffix: &'b [u8],
}

impl<'a, 'b, T> Iterator for PrefixOfMut<'a, 'b, T> {
    type Item = &'a mut T;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let node = self.node.take()?;
            let value = node.value.as_mut();
            if let Some((first, rest)) = self.suffix.split_first()
                && let Some((prefix, node)) = node.children.get_mut(first)
                && let Some(suffix) = rest.strip_prefix(prefix.as_slice())
            {
                assert!(suffix.len() < self.suffix.len());
                self.node = Some(node);
                self.suffix = suffix;
            }
            if value.is_some() {
                break value;
            }
        }
    }
}

#[test]
fn empty_get() {
    let trie = Trie::<i32>::default();
    assert!(trie.get(b"").is_none());
    assert!(trie.get(b"sub").is_none());
}

#[test]
fn insert_at_root() {
    let mut trie = Trie::<i32>::default();
    trie.insert(b"", 123);
    assert_eq!(*trie.get(b"").unwrap(), 123);
    assert!(trie.get(b"sub").is_none());
}

#[test]
fn insert_at_sub() {
    let mut trie = Trie::<i32>::default();
    trie.insert(b"sub", 123);
    assert_eq!(*trie.get(b"sub").unwrap(), 123);
    assert!(trie.get(b"").is_none());
}

#[test]
fn insert_ab() {
    let mut trie = Trie::<i32>::default();
    trie.insert(b"a", 123);
    trie.insert(b"b", 456);
    assert_eq!(*trie.get(b"a").unwrap(), 123);
    assert_eq!(*trie.get(b"b").unwrap(), 456);
    assert!(trie.get(b"").is_none());
}

#[test]
fn insert_common_ab() {
    let mut trie = Trie::<i32>::default();
    trie.insert(b"common-a", 123);
    trie.insert(b"common-b", 456);
    assert_eq!(*trie.get(b"common-a").unwrap(), 123);
    assert_eq!(*trie.get(b"common-b").unwrap(), 456);
    assert!(trie.get(b"").is_none());
    assert!(trie.get(b"common-").is_none());
}

#[test]
fn insert_nested_abc() {
    let mut trie = Trie::<i32>::default();
    trie.insert(b"ab", 456);
    trie.insert(b"a", 123);
    trie.insert(b"abc", 789);
    assert_eq!(*trie.get(b"a").unwrap(), 123);
    assert_eq!(*trie.get(b"ab").unwrap(), 456);
    assert_eq!(*trie.get(b"abc").unwrap(), 789);
    assert!(trie.get(b"").is_none());
    assert!(trie.get(b"b").is_none());
    assert!(trie.get(b"c").is_none());
}

#[test]
fn insert_remove_a() {
    let mut trie = Trie::<i32>::default();
    trie.insert(b"a", 123);
    assert_eq!(trie.remove(b"a").unwrap(), 123);
    assert!(trie.get(b"a").is_none());
}

#[test]
fn insert_remove_ab() {
    let mut trie = Trie::<i32>::default();
    trie.insert(b"+a", 123);
    trie.insert(b"+b", 456);
    assert_eq!(trie.remove(b"+a").unwrap(), 123);
    assert_eq!(trie.remove(b"+b").unwrap(), 456);
    assert!(trie.get(b"+a").is_none());
    assert!(trie.get(b"+b").is_none());
    assert!(trie.root.is_none());
}

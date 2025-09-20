use std::collections::BTreeMap;

use crate::nodes::NodeId;

type Nodes<T> = crate::nodes::Nodes<Node<T>>;

impl<T> Nodes<T> {
    fn pop(&mut self, id: NodeId) {
        let node = &self[id];
        assert!(node.parent.is_none());
        assert!(node.value.is_none());
        assert!(node.children.is_empty());
        assert!(self.pop_at(id).value.is_none());
    }

    #[must_use]
    fn push_value(&mut self, value: T) -> NodeId {
        self.push(Node::new_value(value))
    }

    fn adopt(&mut self, parent: NodeId, first: u8, child: NodeId) {
        let child = &mut self[child];
        assert!(child.parent.is_none());
        child.parent = Some((first, parent));
        self.decrement_roots();
    }

    #[must_use]
    fn push_child(&mut self, first: u8, rest: Vec<u8>, child: NodeId) -> NodeId {
        let parent = self.push(Node::new_child(first, rest, child));
        self.adopt(parent, first, child);
        parent
    }

    fn add_child(&mut self, parent: NodeId, first: u8, rest: Vec<u8>, child: NodeId) {
        let children = &mut self[parent].children;
        assert!(!children.contains_key(&first));
        children.insert(first, (rest, child));
        self.adopt(parent, first, child);
    }

    #[must_use]
    fn add_value(&mut self, parent: NodeId, first: u8, rest: Vec<u8>, value: T) -> NodeId {
        let child = self.push_value(value);
        self.add_child(parent, first, rest, child);
        child
    }

    #[must_use]
    fn add_grandchild(
        &mut self,
        parent: NodeId,
        ofirst: u8,
        orest: Vec<u8>,
        ifirst: u8,
        irest: Vec<u8>,
        grandchild: NodeId,
    ) -> NodeId {
        let child = self.push_child(ifirst, irest, grandchild);
        self.add_child(parent, ofirst, orest, child);
        child
    }

    #[must_use]
    fn remove_child(&mut self, parent: NodeId, first: u8) -> (Vec<u8>, NodeId) {
        let (prefix, child) = self[parent].children.remove(&first).expect("unknown child");
        self.increment_roots();
        self[child].parent = None;
        (prefix, child)
    }

    #[must_use]
    fn disown(&mut self, child: NodeId) -> (NodeId, u8, Vec<u8>) {
        let (first, parent) = self[child].parent.expect("disowning without a parent");
        let (prefix, c) = self.remove_child(parent, first);
        assert_eq!(child, c);
        (parent, first, prefix)
    }

    #[must_use]
    fn locate(&self, mut id: NodeId, mut key: &[u8]) -> Option<NodeId> {
        while let Some((first, rest)) = key.split_first() {
            let (prefix, sub_id) = self[id].children.get(first)?;
            let sub_key = rest.strip_prefix(prefix.as_slice())?;
            assert!(sub_key.len() < key.len());
            id = *sub_id;
            key = sub_key;
        }
        assert!(key.is_empty());
        Some(id)
    }

    #[must_use]
    fn insert(&mut self, mut id: NodeId, mut key: &[u8], value: T) -> (NodeId, Option<T>) {
        let value = loop {
            let Some((first, rest)) = key.split_first() else {
                break self[id].value.replace(value);
            };
            let first = *first;
            let Some((prefix, sub_id)) = self[id].children.get_mut(&first) else {
                id = self.add_value(id, first, rest.into(), value);
                break None;
            };
            let sub_id = *sub_id;
            if let Some(sub_key) = rest.strip_prefix(prefix.as_slice()) {
                assert!(sub_key.len() < key.len());
                id = sub_id;
                key = sub_key;
                continue;
            }
            id = if let Some(old_sub) = prefix.strip_prefix(rest) {
                assert!(!old_sub.is_empty());
                let (old_first, old_rest) = old_sub.split_first().expect("not an empty string");
                let old_first = *old_first;
                let old_rest = old_rest.into();
                {
                    let (p, f, r) = self.disown(sub_id);
                    assert_eq!(p, id);
                    assert_eq!(f, first);
                    assert_eq!(r[..rest.len()], *rest);
                    assert_eq!(r[rest.len()], old_first);
                    assert_eq!(r[rest.len() + 1..], old_rest);
                }
                let middle = self.add_value(id, first, rest.into(), value);
                self.add_child(middle, old_first, old_rest, sub_id);
                middle
            } else {
                let (common, new, old) = common_prefix(rest, prefix);
                assert!(!new.is_empty());
                assert!(!old.is_empty());
                let (new_first, new_rest) = new.split_first().expect("not an empty string");
                let (old_first, old_rest) = old.split_first().expect("not an empty string");
                let old_first = *old_first;
                let old_rest = old_rest.into();
                {
                    let (p, f, r) = self.disown(sub_id);
                    assert_eq!(p, id);
                    assert_eq!(f, first);
                    assert_eq!(r[..common.len()], *common);
                    assert_eq!(r[common.len()], old_first);
                    assert_eq!(r[common.len() + 1..], old_rest);
                }
                let common =
                    self.add_grandchild(id, first, common.into(), old_first, old_rest, sub_id);
                self.add_value(common, *new_first, new_rest.into(), value)
            };
            break None;
        };
        (id, value)
    }

    fn collapse(&mut self, mut id: NodeId) {
        while self[id].is_collapsible() {
            let parent = if let Some(child) = self[id].only_child() {
                let (middle, ifirst, mut irest) = self.disown(child);
                assert_eq!(middle, id);
                let (parent, ofirst, mut orest) = self.disown(id);
                orest.push(ifirst);
                orest.append(&mut irest);
                self.add_child(parent, ofirst, orest, child);
                parent
            } else {
                let (parent, _, _) = self.disown(id);
                parent
            };
            self.pop(id);
            id = parent;
        }
    }

    #[must_use]
    fn remove_at(&mut self, id: NodeId) -> Option<T> {
        let value = self[id].value.take()?;
        self.collapse(id);
        Some(value)
    }

    #[must_use]
    fn remove(&mut self, mut id: NodeId, key: &[u8]) -> Option<(NodeId, T)> {
        id = self.locate(id, key)?;
        let value = self.remove_at(id)?;
        Some((id, value))
    }

    #[must_use]
    fn collect_key(&self, mut id: NodeId) -> Vec<u8> {
        let mut parts = Vec::new();
        while let Some((first, parent)) = &self[id].parent {
            let (prefix, child) = self[*parent].children.get(first).expect("missing child");
            assert_eq!(*child, id);
            parts.push(prefix.as_slice());
            parts.push(std::array::from_ref(first));
            id = *parent;
        }
        parts.reverse();
        parts.concat()
    }
}

/// Has `value` and/or one of its descendants has `value`
#[derive(Debug)]
#[must_use]
struct Node<T> {
    parent: Option<(u8, NodeId)>,
    value: Option<T>,
    children: BTreeMap<u8, (Vec<u8>, NodeId)>,
}

fn common_prefix<'a, 'b>(l: &'a [u8], r: &'b [u8]) -> (&'a [u8], &'a [u8], &'b [u8]) {
    let len = l.iter().zip(r).filter(|(l, r)| l == r).count();
    (&l[..len], &l[len..], &r[len..])
}

impl<T> Node<T> {
    fn new(value: Option<T>, children: BTreeMap<u8, (Vec<u8>, NodeId)>) -> Self {
        assert!(value.is_some() || !children.is_empty());
        Self {
            parent: None,
            value,
            children,
        }
    }

    fn new_value(value: T) -> Self {
        Self::new(Some(value), BTreeMap::new())
    }

    fn new_children(children: BTreeMap<u8, (Vec<u8>, NodeId)>) -> Self {
        assert!(!children.is_empty());
        Self::new(None, children)
    }

    fn new_child(first: u8, rest: Vec<u8>, child: NodeId) -> Self {
        Self::new_children([(first, (rest, child))].into())
    }

    #[must_use]
    fn is_collapsible(&self) -> bool {
        self.parent.is_some() && self.value.is_none() && self.children.len() < 2
    }

    #[must_use]
    fn only_child(&self) -> Option<NodeId> {
        assert!(self.is_collapsible());
        let l = self.children.first_key_value()?;
        let r = self.children.last_key_value()?;
        assert_eq!(l, r);
        Some(l.1.1)
    }
}

#[derive(Debug)]
pub struct Trie<T> {
    nodes: Nodes<T>,
    root: NodeId,
}

impl<T> Default for Trie<T> {
    fn default() -> Self {
        let mut nodes = Nodes::default();
        let root = nodes.push(Node {
            parent: None,
            value: None,
            children: BTreeMap::new(),
        });
        Self { nodes, root }
    }
}

impl<T> Trie<T> {
    #[must_use]
    pub fn try_index(&self, id: NodeId) -> Option<&T> {
        self.nodes.get(id)?.value.as_ref()
    }

    #[must_use]
    pub fn try_index_mut(&mut self, id: NodeId) -> Option<&mut T> {
        self.nodes.get_mut(id)?.value.as_mut()
    }

    #[must_use]
    pub fn contains_key(&self, key: &[u8]) -> bool {
        self.get(key).is_some()
    }

    #[must_use]
    pub fn get<'a>(&'a self, key: &[u8]) -> Option<(NodeId, &'a T)> {
        let id = self.nodes.locate(self.root, key)?;
        let value = self.nodes[id].value.as_ref()?;
        Some((id, value))
    }

    #[must_use]
    pub fn get_mut<'a>(&'a mut self, key: &[u8]) -> Option<(NodeId, &'a mut T)> {
        let id = self.nodes.locate(self.root, key)?;
        let value = self.nodes[id].value.as_mut()?;
        Some((id, value))
    }

    pub fn insert(&mut self, key: &[u8], value: T) -> (NodeId, Option<T>) {
        let result = self.nodes.insert(self.root, key, value);
        assert_eq!(self.nodes.roots(), 1);
        result
    }

    pub fn remove_at(&mut self, id: NodeId) -> Option<T> {
        if self.nodes.contains(id) {
            self.nodes.remove_at(id)
        } else {
            None
        }
    }

    pub fn remove(&mut self, key: &[u8]) -> Option<(NodeId, T)> {
        let result = self.nodes.remove(self.root, key);
        assert_eq!(self.nodes.roots(), 1);
        result
    }

    pub fn prefix_of<'a, 'b>(&'a self, suffix: &'b [u8]) -> PrefixOf<'a, 'b, T> {
        PrefixOf {
            nodes: &self.nodes,
            id: Some(self.root),
            suffix,
        }
    }

    #[must_use]
    pub fn collect_key(&self, id: NodeId) -> Option<Vec<u8>> {
        if self.nodes.contains(id) {
            Some(self.nodes.collect_key(id))
        } else {
            None
        }
    }
}

impl<T: ?Sized> Trie<Box<T>> {
    pub fn prefix_of_mut<'a, 'b>(&'a mut self, suffix: &'b [u8]) -> PrefixOfMut<'a, 'b, T> {
        PrefixOfMut {
            nodes: &mut self.nodes,
            id: Some(self.root),
            suffix,
        }
    }
}

#[must_use]
pub struct PrefixOf<'a, 'b, T> {
    nodes: &'a Nodes<T>,
    id: Option<NodeId>,
    suffix: &'b [u8],
}

impl<'a, 'b, T> Iterator for PrefixOf<'a, 'b, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let id = self.id.take()?;
            if let Some((first, rest)) = self.suffix.split_first()
                && let Some((prefix, id)) = self.nodes[id].children.get(first)
                && let Some(suffix) = rest.strip_prefix(prefix.as_slice())
            {
                assert!(suffix.len() < self.suffix.len());
                self.id = Some(*id);
                self.suffix = suffix;
            }
            if self.nodes[id].value.is_some() {
                break self.nodes[id].value.as_ref();
            }
        }
    }
}

#[must_use]
pub struct PrefixOfMut<'a, 'b, T: ?Sized> {
    nodes: &'a mut Nodes<Box<T>>,
    id: Option<NodeId>,
    suffix: &'b [u8],
}

impl<'a, 'b, T: ?Sized> Iterator for PrefixOfMut<'a, 'b, T> {
    type Item = &'a mut T;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let id = self.id.take()?;
            if let Some((first, rest)) = self.suffix.split_first()
                && let Some((prefix, id)) = self.nodes[id].children.get(first)
                && let Some(suffix) = rest.strip_prefix(prefix.as_slice())
            {
                assert!(suffix.len() < self.suffix.len());
                self.id = Some(*id);
                self.suffix = suffix;
            }
            if self.nodes[id].value.is_some() {
                let value = self.nodes[id]
                    .value
                    .as_deref_mut()
                    .map(|value| std::ptr::from_mut(value))?;
                break Some(unsafe { value.as_mut()? });
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
    assert_eq!(*trie.get(b"").unwrap().1, 123);
    assert!(trie.get(b"sub").is_none());
}

#[test]
fn insert_at_sub() {
    let mut trie = Trie::<i32>::default();
    trie.insert(b"sub", 123);
    assert_eq!(*trie.get(b"sub").unwrap().1, 123);
    assert!(trie.get(b"").is_none());
}

#[test]
fn insert_ab() {
    let mut trie = Trie::<i32>::default();
    trie.insert(b"a", 123);
    trie.insert(b"b", 456);
    assert_eq!(*trie.get(b"a").unwrap().1, 123);
    assert_eq!(*trie.get(b"b").unwrap().1, 456);
    assert!(trie.get(b"").is_none());
}

#[test]
fn insert_common_ab() {
    let mut trie = Trie::<i32>::default();
    trie.insert(b"common-a", 123);
    trie.insert(b"common-b", 456);
    assert_eq!(*trie.get(b"common-a").unwrap().1, 123);
    assert_eq!(*trie.get(b"common-b").unwrap().1, 456);
    assert!(trie.get(b"").is_none());
    assert!(trie.get(b"common-").is_none());
}

#[test]
fn insert_nested_abc() {
    let mut trie = Trie::<i32>::default();
    let (ab, _) = trie.insert(b"ab", 456);
    let (a, _) = trie.insert(b"a", 123);
    let (abc, _) = trie.insert(b"abc", 789);
    assert_eq!(*trie.get(b"a").unwrap().1, 123);
    assert_eq!(*trie.get(b"ab").unwrap().1, 456);
    assert_eq!(*trie.get(b"abc").unwrap().1, 789);
    assert!(trie.get(b"").is_none());
    assert!(trie.get(b"b").is_none());
    assert!(trie.get(b"c").is_none());
    assert_eq!(trie.collect_key(a).unwrap(), b"a");
    assert_eq!(trie.collect_key(ab).unwrap(), b"ab");
    assert_eq!(trie.collect_key(abc).unwrap(), b"abc");
    trie.remove(b"ab").unwrap();
    assert_eq!(trie.collect_key(abc).unwrap(), b"abc");
}

#[test]
fn insert_remove_a() {
    let mut trie = Trie::<i32>::default();
    trie.insert(b"a", 123);
    assert_eq!(trie.remove(b"a").unwrap().1, 123);
    assert!(trie.get(b"a").is_none());
}

#[test]
fn insert_remove_ab() {
    let mut trie = Trie::<i32>::default();
    trie.insert(b"+a", 123);
    trie.insert(b"+b", 456);
    assert_eq!(trie.remove(b"+a").unwrap().1, 123);
    assert_eq!(trie.remove(b"+b").unwrap().1, 456);
    assert!(trie.get(b"+a").is_none());
    assert!(trie.get(b"+b").is_none());
    assert_eq!(trie.nodes.len(), 1);
}

#[test]
fn prefix_of_mut() {
    let mut trie = Trie::<Box<i32>>::default();
    trie.insert(b"a", Box::new(123));
    trie.insert(b"ab", Box::new(456));
    trie.insert(b"abc", Box::new(789));
    trie.prefix_of_mut(b"abc")
        .collect::<Vec<_>>()
        .into_iter()
        .for_each(|value| *value = 426);
    assert_eq!(*trie.get(b"a").unwrap().1, Box::new(426));
    assert_eq!(*trie.get(b"ab").unwrap().1, Box::new(426));
    assert_eq!(*trie.get(b"abc").unwrap().1, Box::new(426));
    trie.prefix_of_mut(b"abc").for_each(|value| *value = 0);
    assert_eq!(*trie.get(b"a").unwrap().1, Box::new(0));
    assert_eq!(*trie.get(b"ab").unwrap().1, Box::new(0));
    assert_eq!(*trie.get(b"abc").unwrap().1, Box::new(0));
}

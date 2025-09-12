use std::collections::BTreeMap;

use slab::Slab;

#[derive(Debug)]
struct Nodes<T> {
    slab: Slab<(usize, Node<T>)>,
    roots: usize,
    ctr: usize,
}

impl<T> Default for Nodes<T> {
    fn default() -> Self {
        Self {
            slab: Default::default(),
            roots: Default::default(),
            ctr: Default::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NodeId {
    location: usize,
    ctr: usize,
}

impl<T> Nodes<T> {
    fn next_ctr(&mut self) -> usize {
        let next_ctr = self.ctr.wrapping_add(1);
        std::mem::replace(&mut self.ctr, next_ctr)
    }

    fn push(&mut self, node: Node<T>) -> NodeId {
        self.increment_roots();
        let ctr = self.next_ctr();
        NodeId {
            location: self.slab.insert((ctr, node)),
            ctr,
        }
    }

    fn pop(&mut self, id: NodeId) {
        let node = self.get(id);
        assert!(node.parent.is_none());
        assert!(node.value.is_none());
        assert!(node.children.is_empty());
        let (ctr, _) = self.slab.remove(id.location);
        assert_eq!(ctr, id.ctr);
        self.decrement_roots();
    }

    fn increment_roots(&mut self) {
        self.roots = self.roots.checked_add(1).expect("root count overflow");
    }

    fn decrement_roots(&mut self) {
        self.roots = self.roots.checked_sub(1).expect("root count underflow");
    }

    fn get(&self, id: NodeId) -> &Node<T> {
        let (ctr, node) = self.slab.get(id.location).expect("invalid node id");
        assert_eq!(*ctr, id.ctr);
        node
    }

    fn get_mut(&mut self, id: NodeId) -> &mut Node<T> {
        let (ctr, node) = self.slab.get_mut(id.location).expect("invalid node id");
        assert_eq!(*ctr, id.ctr);
        node
    }

    fn push_value(&mut self, value: T) -> NodeId {
        self.push(Node::new_value(value))
    }

    fn connect(&mut self, parent: NodeId, first: u8, child: NodeId) {
        let child = self.get_mut(child);
        assert!(child.parent.is_none());
        child.parent = Some((first, parent));
        self.decrement_roots();
    }

    fn push_child(&mut self, first: u8, rest: Vec<u8>, child: NodeId) -> NodeId {
        let parent = self.push(Node::new_child(first, rest, child));
        self.connect(parent, first, child);
        parent
    }

    fn add_child(&mut self, parent: NodeId, first: u8, rest: Vec<u8>, child: NodeId) {
        self.get_mut(parent).children.insert(first, (rest, child));
        self.connect(parent, first, child);
    }

    fn add_value(&mut self, parent: NodeId, first: u8, rest: Vec<u8>, value: T) -> NodeId {
        let child = self.push_value(value);
        self.add_child(parent, first, rest, child);
        child
    }

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

    fn remove_child(&mut self, parent: NodeId, first: u8) -> (Vec<u8>, NodeId) {
        let (prefix, child) = self
            .get_mut(parent)
            .children
            .remove(&first)
            .expect("unknown child");
        self.increment_roots();
        self.get_mut(child).parent = None;
        (prefix, child)
    }

    fn disown(&mut self, child: NodeId) -> (NodeId, u8, Vec<u8>) {
        let (first, parent) = self
            .get_mut(child)
            .parent
            .expect("disowning without a parent");
        let (prefix, c) = self.remove_child(parent, first);
        assert_eq!(child, c);
        (parent, first, prefix)
    }

    fn locate(&self, mut id: NodeId, mut key: &[u8]) -> Option<NodeId> {
        while let Some((first, rest)) = key.split_first() {
            let (prefix, sub_id) = self.get(id).children.get(first)?;
            let sub_key = rest.strip_prefix(prefix.as_slice())?;
            assert!(sub_key.len() < key.len());
            id = *sub_id;
            key = sub_key;
        }
        assert!(key.is_empty());
        Some(id)
    }
}

/// Has `value` and/or one of its descendants has `value`
#[derive(Debug)]
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

    fn is_collapsible(&self) -> bool {
        self.parent.is_some() && self.value.is_none() && self.children.len() < 2
    }

    fn only_child(&self) -> Option<NodeId> {
        assert!(self.is_collapsible());
        let l = self.children.first_key_value()?;
        let r = self.children.last_key_value()?;
        assert_eq!(l, r);
        Some(l.1.1)
    }
}

struct NodeMut<'a, T> {
    nodes: &'a mut Nodes<T>,
    id: NodeId,
}

impl<'a, T> NodeMut<'a, T> {
    fn as_mut(&mut self) -> &mut Node<T> {
        self.nodes.get_mut(self.id)
    }

    fn into_mut(self) -> &'a mut Node<T> {
        self.nodes.get_mut(self.id)
    }

    fn insert(mut self, mut key: &[u8], value: T) -> (NodeId, Option<T>) {
        loop {
            let Some((first, rest)) = key.split_first() else {
                break (self.id, self.into_mut().value.replace(value));
            };
            let Some((prefix, id)) = self.as_mut().children.get_mut(first) else {
                let id = self.nodes.add_value(self.id, *first, rest.into(), value);
                break (id, None);
            };
            let id = *id;
            if let Some(sub_key) = rest.strip_prefix(prefix.as_slice()) {
                assert!(sub_key.len() < key.len());
                self.id = id;
                key = sub_key;
                continue;
            }
            let id = if let Some(old_sub) = prefix.strip_prefix(rest) {
                assert!(!old_sub.is_empty());
                let (old_first, old_rest) = old_sub.split_first().expect("not an empty string");
                let old_first = *old_first;
                let old_rest = old_rest.into();
                self.nodes.disown(id);
                let middle = self.nodes.add_value(self.id, *first, rest.into(), value);
                self.nodes.add_child(middle, old_first, old_rest, id);
                middle
            } else {
                let (common, new, old) = common_prefix(rest, prefix);
                assert!(!new.is_empty());
                assert!(!old.is_empty());
                let (new_first, new_rest) = new.split_first().expect("not an empty string");
                let (old_first, old_rest) = old.split_first().expect("not an empty string");
                let old_first = *old_first;
                let old_rest = old_rest.into();
                self.nodes.disown(id);
                let common = self.nodes.add_grandchild(
                    self.id,
                    *first,
                    common.into(),
                    old_first,
                    old_rest,
                    id,
                );
                self.nodes
                    .add_value(common, *new_first, new_rest.into(), value)
            };
            break (id, None);
        }
    }

    fn remove(mut self, key: &[u8]) -> Option<T> {
        self.id = self.nodes.locate(self.id, key)?;
        let value = self.nodes.get_mut(self.id).value.take()?;
        while let node = self.as_mut()
            && node.is_collapsible()
        {
            if let Some(child) = node.only_child() {
                let (middle, ifirst, mut irest) = self.nodes.disown(child);
                assert_eq!(middle, self.id);
                let (parent, ofirst, mut orest) = self.nodes.disown(self.id);
                orest.push(ifirst);
                orest.append(&mut irest);
                self.nodes.add_child(parent, ofirst, orest, child);
                self.nodes.pop(self.id);
                self.id = parent;
            } else {
                let (parent, _, _) = self.nodes.disown(self.id);
                self.nodes.pop(self.id);
                self.id = parent;
            }
        }
        Some(value)
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
    pub fn get<'a>(&'a self, key: &[u8]) -> Option<&'a T> {
        self.nodes
            .get(self.nodes.locate(self.root, key)?)
            .value
            .as_ref()
    }

    pub fn get_mut<'a>(&'a mut self, key: &[u8]) -> Option<&'a mut T> {
        self.nodes
            .get_mut(self.nodes.locate(self.root, key)?)
            .value
            .as_mut()
    }

    pub fn insert(&mut self, key: &[u8], value: T) -> Option<T> {
        let (_, value) = NodeMut {
            nodes: &mut self.nodes,
            id: self.root,
        }
        .insert(key, value);
        assert_eq!(self.nodes.roots, 1);
        value
    }

    pub fn remove(&mut self, key: &[u8]) -> Option<T> {
        let value = NodeMut {
            nodes: &mut self.nodes,
            id: self.root,
        }
        .remove(key);
        assert_eq!(self.nodes.roots, 1);
        value
    }

    pub fn prefix_of<'a, 'b>(&'a self, suffix: &'b [u8]) -> PrefixOf<'a, 'b, T> {
        PrefixOf {
            nodes: &self.nodes,
            id: Some(self.root),
            suffix,
        }
    }

    pub fn prefix_of_mut<'a, 'b>(&'a mut self, suffix: &'b [u8]) -> PrefixOfMut<'a, 'b, T> {
        PrefixOfMut {
            nodes: &mut self.nodes,
            id: Some(self.root),
            suffix,
        }
    }
}

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
                && let Some((prefix, id)) = self.nodes.get(id).children.get(first)
                && let Some(suffix) = rest.strip_prefix(prefix.as_slice())
            {
                assert!(suffix.len() < self.suffix.len());
                self.id = Some(*id);
                self.suffix = suffix;
            }
            if self.nodes.get(id).value.is_some() {
                break self.nodes.get(id).value.as_ref();
            }
        }
    }
}

pub struct PrefixOfMut<'a, 'b, T> {
    nodes: &'a mut Nodes<T>,
    id: Option<NodeId>,
    suffix: &'b [u8],
}

impl<'a, 'b, T> PrefixOfMut<'a, 'b, T> {
    pub fn borrow_next(&mut self) -> Option<&mut T> {
        loop {
            let id = self.id.take()?;
            if let Some((first, rest)) = self.suffix.split_first()
                && let Some((prefix, id)) = self.nodes.get(id).children.get(first)
                && let Some(suffix) = rest.strip_prefix(prefix.as_slice())
            {
                assert!(suffix.len() < self.suffix.len());
                self.id = Some(*id);
                self.suffix = suffix;
            }
            if self.nodes.get(id).value.is_some() {
                break self.nodes.get_mut(id).value.as_mut();
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
    assert_eq!(trie.nodes.slab.len(), 1);
}

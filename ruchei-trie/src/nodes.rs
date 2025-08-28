use std::ops::{Index, IndexMut};

use slab::Slab;

#[derive(Debug)]
pub(crate) struct Nodes<Node> {
    slab: Slab<(usize, Node)>,
    roots: usize,
    ctr: usize,
}

impl<Node> Default for Nodes<Node> {
    fn default() -> Self {
        Self {
            slab: Default::default(),
            roots: Default::default(),
            ctr: Default::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct NodeId {
    location: usize,
    ctr: usize,
}

impl<Node> Nodes<Node> {
    pub(crate) fn get(&self, id: NodeId) -> Option<&Node> {
        let (ctr, node) = self.slab.get(id.location)?;
        (*ctr == id.ctr).then_some(node)
    }

    pub(crate) fn get_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        let (ctr, node) = self.slab.get_mut(id.location)?;
        (*ctr == id.ctr).then_some(node)
    }

    pub(crate) fn contains(&self, id: NodeId) -> bool {
        self.get(id).is_some()
    }

    fn next_ctr(&mut self) -> usize {
        let next_ctr = self.ctr.wrapping_add(1);
        std::mem::replace(&mut self.ctr, next_ctr)
    }

    pub(crate) fn push(&mut self, node: Node) -> NodeId {
        self.increment_roots();
        let ctr = self.next_ctr();
        NodeId {
            location: self.slab.insert((ctr, node)),
            ctr,
        }
    }

    pub(crate) fn pop_at(&mut self, id: NodeId) {
        let (ctr, _) = self.slab.remove(id.location);
        assert_eq!(ctr, id.ctr);
        self.decrement_roots();
    }

    pub(crate) fn increment_roots(&mut self) {
        self.roots = self.roots.checked_add(1).expect("root count overflow");
    }

    pub(crate) fn decrement_roots(&mut self) {
        self.roots = self.roots.checked_sub(1).expect("root count underflow");
    }

    pub(crate) fn roots(&self) -> usize {
        self.roots
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.slab.len()
    }
}

impl<Node> Index<NodeId> for Nodes<Node> {
    type Output = Node;

    fn index(&self, id: NodeId) -> &Self::Output {
        let (ctr, node) = self.slab.get(id.location).expect("invalid node id");
        assert_eq!(*ctr, id.ctr);
        node
    }
}

impl<Node> IndexMut<NodeId> for Nodes<Node> {
    fn index_mut(&mut self, id: NodeId) -> &mut Self::Output {
        let (ctr, node) = self.slab.get_mut(id.location).expect("invalid node id");
        assert_eq!(*ctr, id.ctr);
        node
    }
}

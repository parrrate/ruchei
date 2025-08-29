use std::{
    cmp::Ordering,
    ops::{BitXor, Not},
};

use crate::nodes::NodeId;

type Nodes<T> = crate::nodes::Nodes<Node<T>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Side {
    L,
    R,
}

impl Not for Side {
    type Output = Self;

    fn not(self) -> Self::Output {
        match self {
            Self::L => Self::R,
            Self::R => Self::L,
        }
    }
}

#[derive(Debug)]
enum Diff {
    Eq,
    Ne,
}

impl BitXor<Side> for Option<Diff> {
    type Output = Option<Side>;

    fn bitxor(self, rhs: Side) -> Self::Output {
        Some(match (self?, rhs) {
            (Diff::Eq, Side::L) => Side::L,
            (Diff::Eq, Side::R) => Side::R,
            (Diff::Ne, Side::L) => Side::R,
            (Diff::Ne, Side::R) => Side::L,
        })
    }
}

impl BitXor<Side> for Option<Side> {
    type Output = Option<Diff>;

    fn bitxor(self, rhs: Side) -> Self::Output {
        Some(match (self?, rhs) {
            (Side::L, Side::L) => Diff::Eq,
            (Side::L, Side::R) => Diff::Ne,
            (Side::R, Side::L) => Diff::Ne,
            (Side::R, Side::R) => Diff::Eq,
        })
    }
}

impl<T> Nodes<T> {
    fn pop(&mut self, id: NodeId) -> T {
        let node = &self[id];
        assert!(node.parent.is_none());
        assert!(node.bias.is_none());
        assert!(node.l.is_none());
        assert!(node.r.is_none());
        self.pop_at(id).value
    }

    fn push_value(&mut self, value: T) -> NodeId {
        self.push(Node::new(value))
    }

    fn adopt(&mut self, parent: NodeId, side: Side, child: NodeId) {
        let child = &mut self[child];
        assert!(child.parent.is_none());
        child.parent = Some((side, parent));
        self.decrement_roots();
    }

    fn add_child(&mut self, parent: NodeId, side: Side, child: NodeId) {
        let location = self[parent].side_mut(side);
        assert!(location.is_none());
        *location = Some(child);
        self.adopt(parent, side, child);
    }

    fn add_value(&mut self, parent: NodeId, side: Side, value: T) -> NodeId {
        let child = self.push_value(value);
        self.add_child(parent, side, child);
        child
    }

    fn remove_child(&mut self, parent: NodeId, side: Side) -> NodeId {
        let child = self[parent].side_mut(side).take().expect("unknown child");
        self.increment_roots();
        self[child].parent = None;
        child
    }

    fn disown(&mut self, child: NodeId) -> (Side, NodeId) {
        let (side, parent) = self[child].parent.expect("disowning without a parent");
        let c = self.remove_child(parent, side);
        assert_eq!(child, c);
        (side, parent)
    }

    fn replace(&mut self, id: NodeId, value: T) -> T {
        self[id].replace(value)
    }

    fn up(&self, mut node: NodeId, towards: Side) -> Option<NodeId> {
        while let Some((side, parent)) = self[node].parent {
            if !side == towards {
                return Some(parent);
            }
            node = parent;
        }
        None
    }

    fn down(&self, mut node: NodeId, towards: Side) -> NodeId {
        while let Some(next) = self[node].side(towards) {
            node = next;
        }
        node
    }
}

struct Node<T> {
    parent: Option<(Side, NodeId)>,
    value: T,
    bias: Option<Side>,
    l: Option<NodeId>,
    r: Option<NodeId>,
}

impl<T> Node<T> {
    fn new(value: T) -> Self {
        Self {
            parent: None,
            value,
            bias: None,
            l: None,
            r: None,
        }
    }

    fn side(&self, side: Side) -> Option<NodeId> {
        match side {
            Side::L => self.l,
            Side::R => self.r,
        }
    }

    fn side_mut(&mut self, side: Side) -> &mut Option<NodeId> {
        match side {
            Side::L => &mut self.l,
            Side::R => &mut self.r,
        }
    }

    fn replace(&mut self, value: T) -> T {
        std::mem::replace(&mut self.value, value)
    }
}

pub struct Avl<T> {
    nodes: Nodes<T>,
    root: Option<NodeId>,
    height: usize,
}

impl<T> Default for Avl<T> {
    fn default() -> Self {
        Self {
            nodes: Default::default(),
            root: Default::default(),
            height: Default::default(),
        }
    }
}

enum Mode {
    // Current node is balanced. May leave the node in an unbalanced state.
    Balanced,
    // Current node is unbalanced opposite the rotation. Makes it balanced.
    Unbalanced,
    // Current node and the deep node are both unbalanced opposite the rotation. Make both balanced.
    VeryUnbalanced,
}

impl<T> Avl<T> {
    fn pop(&mut self, id: NodeId) -> T {
        let node = &self.nodes[id];
        if node.parent.is_none() {
            assert_ne!(self.root, Some(id));
        }
        self.nodes.pop(id)
    }

    fn adopt_root(&mut self, child: NodeId) {
        let root = self.root.expect("not root");
        assert_eq!(root, child);
        assert!(self.nodes[child].parent.is_none());
        self.nodes.decrement_roots();
    }

    fn set_root(&mut self, child: NodeId) {
        assert!(self.root.is_none());
        self.root = Some(child);
        self.adopt_root(child);
    }

    fn set_child(&mut self, parent: NodeId, side: Side, child: Option<NodeId>) {
        match child {
            Some(child) => self.nodes.add_child(parent, side, child),
            None => {
                assert!(self.nodes[parent].side(side).is_none());
            }
        }
    }

    fn set_parent(&mut self, parent: Option<(Side, NodeId)>, child: NodeId) {
        match parent {
            Some((side, parent)) => self.nodes.add_child(parent, side, child),
            None => {
                assert!(self.root.is_none());
                self.set_root(child);
            }
        }
    }

    fn remove_root(&mut self) -> NodeId {
        let child = self.root.take().expect("no root");
        self.nodes.increment_roots();
        child
    }

    fn remove_child(&mut self, parent: Option<(Side, NodeId)>) -> NodeId {
        match parent {
            Some((side, parent)) => self.nodes.remove_child(parent, side),
            None => self.remove_root(),
        }
    }

    fn disown(&mut self, child: NodeId) -> Option<(Side, NodeId)> {
        if self.nodes[child].parent.is_some() {
            Some(self.nodes.disown(child))
        } else {
            let c = self.remove_root();
            assert_eq!(child, c);
            None
        }
    }

    fn take_child(&mut self, parent: NodeId, side: Side) -> Option<NodeId> {
        self.nodes[parent].side(side)?;
        Some(self.nodes.remove_child(parent, side))
    }

    fn rotate(&mut self, side: Side, n2: NodeId, mode: &mut Mode) {
        let p = self.disown(n2);
        let n1 = self.take_child(n2, side);
        let n4 = self.remove_child(Some((!side, n2)));
        let n3 = self.take_child(n4, side);
        let n5 = self.take_child(n4, !side);
        let a2 = self.nodes[n2].bias ^ side;
        let a4 = self.nodes[n4].bias ^ side;
        let (a2, a4) = match mode {
            Mode::Balanced => match (a2, a4) {
                (Some(Diff::Ne), None) => {
                    *mode = Mode::Unbalanced;
                    (None, Some(Diff::Eq))
                }
                (Some(Diff::Ne), Some(Diff::Eq)) => {
                    *mode = Mode::VeryUnbalanced;
                    (None, Some(Diff::Eq))
                }
                (Some(Diff::Ne), Some(Diff::Ne)) => {
                    *mode = Mode::Unbalanced;
                    (Some(Diff::Eq), Some(Diff::Eq))
                }
                _ => panic!("unbalanced"),
            },
            Mode::Unbalanced => match (a2, a4) {
                (None, None) => (None, Some(Diff::Eq)),
                (None, Some(Diff::Ne)) => (Some(Diff::Eq), Some(Diff::Eq)),
                (Some(Diff::Ne), None) => (Some(Diff::Ne), Some(Diff::Eq)),
                (Some(Diff::Ne), Some(Diff::Ne)) => (None, None),
                _ => panic!("unbalanced"),
            },
            Mode::VeryUnbalanced => match (a2, a4) {
                (Some(Diff::Ne), Some(Diff::Ne)) => (Some(Diff::Eq), None),
                _ => panic!("unbalanced"),
            },
        };
        self.nodes[n2].bias = a2 ^ side;
        self.nodes[n4].bias = a4 ^ side;
        self.set_child(n2, side, n1);
        self.set_child(n2, !side, n3);
        self.nodes.add_child(n4, side, n2);
        self.set_child(n4, !side, n5);
        self.set_parent(p, n4);
    }

    /// Assumes `parent[side].height == parent[!side].height + 2`
    fn balance(&mut self, parent: NodeId, side: Side) -> (NodeId, bool) {
        let node = self.nodes[parent].side(side).expect("unknown child");
        let mut mode = Mode::Unbalanced;
        let height_unchanged = self.nodes[node].bias.is_none();
        if self.nodes[node].bias == Some(!side) {
            mode = Mode::Balanced;
            self.rotate(side, node, &mut mode);
        }
        self.rotate(!side, parent, &mut mode);
        (node, height_unchanged)
    }

    fn increase_height(&mut self, mut node: NodeId) {
        while let Some((side, parent)) = self.nodes[node].parent {
            match self.nodes[parent].bias ^ side {
                Some(Diff::Eq) => {
                    self.balance(parent, side);
                    return;
                }
                Some(Diff::Ne) => {
                    self.nodes[parent].bias = None;
                    return;
                }
                None => {
                    self.nodes[parent].bias = Some(side);
                    node = parent;
                }
            }
        }
        self.height = self.height.checked_add(1).expect("height overflow");
    }

    fn insert_child(&mut self, parent: NodeId, side: Side, value: T) -> NodeId {
        assert!(self.nodes[parent].side(side).is_none());
        let child = self.nodes.add_value(parent, side, value);
        if self.nodes[parent].side(!side).is_some() {
            assert_eq!(self.nodes[parent].bias, Some(!side));
            self.nodes[parent].bias = None;
        } else {
            assert!(self.nodes[parent].bias.is_none());
            self.nodes[parent].bias = Some(side);
            self.increase_height(parent);
        }
        child
    }

    fn create_root(&mut self, value: T) -> NodeId {
        let node = self.nodes.push_value(value);
        self.set_root(node);
        self.height = 1;
        node
    }

    fn insert_at(&mut self, parent: Option<(Side, NodeId)>, value: T) -> NodeId {
        if let Some((side, parent)) = parent {
            self.insert_child(parent, side, value)
        } else {
            self.create_root(value)
        }
    }

    fn swap_locations(&mut self, a: NodeId, b: NodeId) {
        let ap = self.disown(a);
        let bp = self.disown(b);
        let al = self.take_child(a, Side::L);
        let bl = self.take_child(b, Side::L);
        let ar = self.take_child(a, Side::R);
        let br = self.take_child(b, Side::R);
        let ab = self.nodes[a].bias.take();
        let bb = self.nodes[b].bias.take();
        let (a, b) = (a, b);
        self.set_parent(ap, a);
        self.set_parent(bp, b);
        self.set_child(a, Side::L, al);
        self.set_child(b, Side::L, bl);
        self.set_child(a, Side::R, ar);
        self.set_child(b, Side::R, br);
        self.nodes[a].bias = ab;
        self.nodes[b].bias = bb;
    }

    fn skip(&mut self, parent: NodeId, child: NodeId, side: Side) -> T {
        assert_eq!(self.nodes[parent].side(side), Some(child));
        assert!(self.nodes[parent].side(!side).is_none());
        assert_eq!(self.nodes[parent].bias, Some(side));
        self.nodes[parent].bias = None;
        let (expected, p) = self.disown(child).expect("unknown child");
        assert_eq!(p, parent);
        assert_eq!(expected, side);
        let grandparent = self.disown(parent);
        self.set_parent(grandparent, child);
        self.decrease_height(child);
        self.pop(parent)
    }

    fn decrease_height(&mut self, mut node: NodeId) {
        while let Some((side, parent)) = self.nodes[node].parent {
            match self.nodes[parent].bias ^ side {
                Some(Diff::Eq) => {
                    self.nodes[parent].bias = None;
                    node = parent;
                }
                Some(Diff::Ne) => {
                    let (next, height_unchanged) = self.balance(parent, !side);
                    if height_unchanged {
                        return;
                    }
                    node = next;
                }
                None => {
                    self.nodes[parent].bias = Some(!side);
                    return;
                }
            }
        }
        self.height = self.height.checked_sub(1).expect("height underflow");
    }

    fn remove_leaf(&mut self, id: NodeId) -> T {
        if let Some((side, parent)) = self.nodes[id].parent {
            let c = self.nodes.remove_child(parent, side);
            let p = &mut self.nodes[parent];
            assert_eq!(c, id);
            match p.bias ^ side {
                Some(Diff::Eq) => {
                    assert!(p.side(!side).is_none());
                    p.bias = None;
                    self.decrease_height(parent);
                }
                Some(Diff::Ne) => {
                    let (parent, height_unchanged) = self.balance(parent, !side);
                    if !height_unchanged {
                        self.decrease_height(parent);
                    }
                }
                None => {
                    assert!(p.side(!side).is_some());
                    p.bias = Some(!side);
                }
            }
        } else {
            assert_eq!(self.root, Some(id));
            assert_eq!(self.height, 1);
            let parent = self.disown(id);
            assert!(parent.is_none());
            assert!(self.root.is_none());
            self.height = 0;
        }
        self.pop(id)
    }

    fn remove_without_left(&mut self, id: NodeId) -> T {
        let node = &self.nodes[id];
        assert!(node.l.is_none());
        match node.r {
            Some(child) => self.skip(id, child, Side::R),
            None => self.remove_leaf(id),
        }
    }

    fn remove_at(&mut self, id: NodeId) -> T {
        let node = &self.nodes[id];
        match node.l {
            Some(child) => match node.r {
                Some(r) => {
                    let r = self.nodes.down(r, Side::L);
                    self.swap_locations(id, r);
                    self.remove_without_left(id)
                }
                None => self.skip(id, child, Side::L),
            },
            None => self.remove_without_left(id),
        }
    }

    pub fn iter(&self) -> Iter<'_, T> {
        Iter {
            avl: self,
            id: self.root,
        }
    }
}

impl<T: Ord> Nodes<T> {
    fn locate(&self, mut id: NodeId, value: &T) -> Result<NodeId, (Side, NodeId)> {
        loop {
            let node = &self[id];
            match node.value.cmp(value) {
                Ordering::Greater => {
                    let Some(next) = node.l else {
                        return Err((Side::L, id));
                    };
                    id = next;
                }
                Ordering::Equal => break Ok(id),
                Ordering::Less => {
                    let Some(next) = node.r else {
                        return Err((Side::R, id));
                    };
                    id = next;
                }
            }
        }
    }
}

impl<T: Ord> Avl<T> {
    fn locate(&self, value: &T) -> Result<NodeId, Option<(Side, NodeId)>> {
        match self.root {
            Some(id) => self.nodes.locate(id, value).map_err(Some),
            None => Err(None),
        }
    }

    pub fn insert(&mut self, value: T) -> (NodeId, Option<T>) {
        let r = match self.locate(&value) {
            Ok(id) => (id, Some(self.nodes.replace(id, value))),
            Err(parent) => (self.insert_at(parent, value), None),
        };
        assert_eq!(self.nodes.roots(), 0);
        r
    }

    pub fn remove(&mut self, value: &T) -> Option<(NodeId, T)> {
        let id = self.locate(value).ok()?;
        let value = self.remove_at(id);
        assert_eq!(self.nodes.roots(), 0);
        Some((id, value))
    }

    pub fn get<'a>(&'a self, value: &T) -> Option<(NodeId, &'a T)> {
        let id = self.locate(value).ok()?;
        let value = &self.nodes[id].value;
        Some((id, value))
    }
}

pub struct Iter<'a, T> {
    avl: &'a Avl<T>,
    id: Option<NodeId>,
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        let id = self.id?;
        self.id = if let Some(r) = self.avl.nodes[id].r {
            Some(self.avl.nodes.down(r, Side::L))
        } else {
            self.avl.nodes.up(id, Side::R)
        };
        Some(&self.avl.nodes[id].value)
    }
}

#[cfg(test)]
impl<T: std::fmt::Display> Avl<T> {
    fn print_values(&self) {
        let Some(node) = self.root else {
            return;
        };
        let mut node = self.nodes.down(node, Side::L);
        loop {
            print!(
                "[ {} {{{:?}}} ]",
                self.nodes[node].value, self.nodes[node].bias
            );
            if let Some(r) = self.nodes[node].r {
                print!(" \\ ");
                node = self.nodes.down(r, Side::L);
            } else if let Some(u) = self.nodes.up(node, Side::R) {
                print!(" / ");
                node = u;
            } else {
                break;
            }
        }
        println!();
    }
}

#[test]
fn test() {
    let mut avl = Avl::<i32>::default();
    avl.insert(2);
    assert_eq!(avl.height, 1);
    avl.insert(1);
    assert_eq!(avl.height, 2);
    avl.insert(6);
    assert_eq!(avl.height, 2);
    avl.insert(4);
    assert_eq!(avl.height, 3);
    avl.insert(7);
    assert_eq!(avl.height, 3);
    avl.insert(5);
    assert_eq!(avl.height, 3);
    avl.insert(3);
    assert_eq!(avl.height, 3);
    avl.print_values();
    avl.remove(&1).unwrap();
    avl.print_values();
    avl.remove(&2).unwrap();
    avl.print_values();
    avl.remove(&3).unwrap();
    avl.print_values();
    avl.remove(&4).unwrap();
    avl.print_values();
    avl.remove(&5).unwrap();
    avl.print_values();
    avl.remove(&6).unwrap();
    avl.print_values();
    avl.remove(&7).unwrap();
    avl.print_values();
}

//! <https://www.1024cores.net/home/lock-free-algorithms/queues/intrusive-mpsc-node-based-queue>

#![deny(clippy::as_pointer_underscore)]
#![deny(clippy::borrow_as_ptr)]
#![deny(clippy::ptr_as_ptr)]
#![deny(clippy::ptr_cast_constness)]

use std::{
    cell::UnsafeCell,
    marker::PhantomData,
    mem::MaybeUninit,
    ops::{Index, IndexMut},
    sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, AtomicUsize, Ordering},
    task::{RawWaker, RawWakerVTable, Waker},
};

use atomic_waker::AtomicWaker;
use crossbeam_utils::CachePadded;

struct AtomicConst<T>(AtomicPtr<T>);

impl<T> AtomicConst<T> {
    fn new(ptr: *const T) -> Self {
        Self(AtomicPtr::new(ptr.cast_mut()))
    }

    fn get_mut(&mut self) -> &mut *const T {
        unsafe { std::mem::transmute(self.0.get_mut()) }
    }

    fn swap(&self, ptr: *const T, order: Ordering) -> *const T {
        self.0.swap(ptr.cast_mut(), order).cast_const()
    }

    fn store(&self, ptr: *const T, order: Ordering) {
        self.0.store(ptr.cast_mut(), order);
    }

    fn load(&self, order: Ordering) -> *const T {
        self.0.load(order).cast_const()
    }
}

struct OwnRoot<S, const W: usize, const L: usize = W> {
    wake_tail: [*const Node<S, W, L>; W],
}

struct Root<S, const W: usize, const L: usize = W> {
    own: UnsafeCell<OwnRoot<S, W, L>>,
    wakers: [AtomicWaker; W],
    wake_head: [CachePadded<AtomicConst<Node<S, W, L>>>; W],
    wake_stub: Node<S, W, L>,
}

struct OwnNode<S, const W: usize, const L: usize = W> {
    stream: MaybeUninit<S>,
    own_next: *const Node<S, W, L>,
    own_prev: *const Node<S, W, L>,
}

const STATE_CAN_WAKE: u8 = 1;
const STATE_QUEUEING: u8 = 2;
const STATE_IN_QUEUE: u8 = 3;
const STATE_DISOWNED: u8 = 4;
const STATE_NOT_NODE: u8 = 5;

struct Node<S, const W: usize, const L: usize = W> {
    root: *const Root<S, W, L>,
    ctr: AtomicUsize,
    own: UnsafeCell<OwnNode<S, W, L>>,
    has_value: AtomicBool,
    wake_next: [CachePadded<AtomicConst<Self>>; W],
    state: [CachePadded<AtomicU8>; W],
}

const MAX_REFCOUNT: usize = (isize::MAX) as usize;

impl<S, const W: usize, const L: usize> Node<S, W, L> {
    fn increase_ctr(&self) {
        let old_count = self.ctr.fetch_add(1, Ordering::AcqRel);
        if old_count > MAX_REFCOUNT {
            std::process::abort();
        }
    }

    fn decrease_ctr(&self) -> bool {
        let old_count = self.ctr.fetch_sub(1, Ordering::AcqRel);
        old_count == 1
    }

    unsafe fn drop_self(node: *const Self) {
        if unsafe { (*node).decrease_ctr() } {
            unsafe {
                (*node)
                    .state
                    .iter()
                    .for_each(|state| assert_eq!(state.load(Ordering::Acquire), STATE_DISOWNED))
            };
            unsafe { Root::drop_self((*node).root) };
            drop(unsafe { Box::from_raw(node.cast_mut()) });
        }
    }

    fn wake<const X: usize>(&self) {
        if unsafe { (*self.root).outer_push::<X>(self) } {
            unsafe { (*self.root).wake::<X>() };
        }
    }

    unsafe fn wake_drop<const X: usize>(node: *const Self) {
        unsafe { (*node).wake::<X>() };
        unsafe { Self::drop_self(node) };
    }

    fn from_void(p: *const ()) -> *const Self {
        p.cast()
    }

    const fn vtable<const X: usize>() -> &'static RawWakerVTable {
        &RawWakerVTable::new(
            |p| unsafe { (*Self::from_void(p)).raw_waker::<X>() },
            |p| unsafe { Self::wake_drop::<X>(Self::from_void(p)) },
            |p| unsafe { (*Self::from_void(p)).wake::<X>() },
            |p| unsafe { Self::drop_self(Self::from_void(p)) },
        )
    }

    fn raw_waker<const X: usize>(&self) -> RawWaker {
        self.increase_ctr();
        RawWaker::new(std::ptr::from_ref(self).cast(), Self::vtable::<X>())
    }

    fn waker<const X: usize>(&self) -> Waker {
        unsafe { Waker::from_raw(self.raw_waker::<X>()) }
    }
}

impl<S, const W: usize, const L: usize> Root<S, W, L> {
    unsafe fn drop_self(root: *const Self) {
        if unsafe { (*root).wake_stub.decrease_ctr() } {
            assert!(unsafe { (*root).is_empty() });
            drop(unsafe { Box::from_raw(root.cast_mut()) });
        }
    }

    unsafe fn is_empty(&self) -> bool {
        std::ptr::from_ref(&self.wake_stub) == unsafe { (*self.wake_stub.own.get()).own_next }
    }

    fn new() -> *const Self {
        let x = std::ptr::from_mut(Box::leak(Box::new(Self {
            own: UnsafeCell::new(OwnRoot {
                wake_tail: [std::ptr::null(); W],
            }),
            wakers: std::array::from_fn(|_| AtomicWaker::new()),
            wake_head: std::array::from_fn(|_| {
                CachePadded::new(AtomicConst::new(std::ptr::null()))
            }),
            wake_stub: Node {
                root: std::ptr::null(),
                ctr: AtomicUsize::new(1),
                own: UnsafeCell::new(OwnNode {
                    stream: MaybeUninit::uninit(),
                    own_next: std::ptr::null_mut(),
                    own_prev: std::ptr::null_mut(),
                }),
                has_value: AtomicBool::new(false),
                wake_next: std::array::from_fn(|_| {
                    CachePadded::new(AtomicConst::new(std::ptr::null()))
                }),
                state: std::array::from_fn(|_| CachePadded::new(AtomicU8::new(STATE_NOT_NODE))),
            },
        })));
        let stub_ptr = unsafe { &raw mut (*x).wake_stub };
        unsafe {
            (*x).own
                .get_mut()
                .wake_tail
                .iter_mut()
                .for_each(|p| *p = stub_ptr);
            (*x).wake_head
                .iter_mut()
                .for_each(|p| *p.get_mut() = stub_ptr);
            (*x).wake_stub.root = x;
            (*x).wake_stub.own.get_mut().own_next = stub_ptr;
            (*x).wake_stub.own.get_mut().own_prev = stub_ptr;
        }
        x
    }

    fn outer_push<const X: usize>(&self, n: &Node<S, W, L>) -> bool {
        let is_queueing = n.state[X]
            .compare_exchange(
                STATE_CAN_WAKE,
                STATE_QUEUEING,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok();
        if !is_queueing {
            return false;
        }
        n.increase_ctr();
        self.inner_push::<X>(n);
        n.state[X].store(STATE_IN_QUEUE, Ordering::Release);
        true
    }

    fn inner_push<const X: usize>(&self, n: &Node<S, W, L>) {
        let root_ptr = std::ptr::from_ref(self);
        assert_eq!(n.root, root_ptr);
        assert!(n.wake_next[X].load(Ordering::Acquire).is_null());
        let n = std::ptr::from_ref(n);
        let prev = self.wake_head[X].swap(n, Ordering::AcqRel);
        unsafe { (*prev).wake_next[X].store(n, Ordering::Release) };
    }

    fn wake<const X: usize>(&self) {
        self.wakers[X].wake();
    }

    unsafe fn pop_raw<const X: usize>(&self) -> *const Node<S, W, L> {
        let own = unsafe { &mut *self.own.get() };
        let mut tail = own.wake_tail[X];
        let mut next = unsafe { (*tail).wake_next[X].load(Ordering::Acquire) };
        if tail == &raw const self.wake_stub {
            if next.is_null() {
                return std::ptr::null();
            }
            own.wake_tail[X] = next;
            tail = next;
            next = unsafe { (*next).wake_next[X].load(Ordering::Acquire) };
        }
        if !next.is_null() {
            own.wake_tail[X] = next;
            return tail;
        }
        let head = self.wake_head[X].load(Ordering::Acquire);
        if tail != head {
            return std::ptr::null();
        }
        self.wake_stub.wake_next[X].store(std::ptr::null(), Ordering::Release);
        self.inner_push::<X>(&self.wake_stub);
        next = unsafe { (*tail).wake_next[X].load(Ordering::Acquire) };
        if !next.is_null() {
            own.wake_tail[X] = next;
            return tail;
        }
        std::ptr::null()
    }

    unsafe fn pop<const X: usize>(&self) -> *const Node<S, W, L> {
        'outer: loop {
            let n = unsafe { self.pop_raw::<X>() };
            if !n.is_null() {
                'inner: loop {
                    let r = unsafe {
                        (*n).state[X].compare_exchange(
                            STATE_IN_QUEUE,
                            STATE_CAN_WAKE,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                    };
                    match r {
                        Ok(_) => break 'inner,
                        Err(STATE_DISOWNED) => {
                            unsafe { Node::drop_self(n) };
                            continue 'outer;
                        }
                        Err(STATE_QUEUEING) => {}
                        Err(_) => unreachable!("{r:?}"),
                    }
                }
            }
            break n;
        }
    }

    unsafe fn remove(n: *const Node<S, W, L>) -> S {
        assert!(unsafe { (*n).has_value.load(Ordering::Acquire) });
        unsafe {
            (*n).state.iter().for_each(|state| {
                loop {
                    let r = state.compare_exchange(
                        STATE_CAN_WAKE,
                        STATE_DISOWNED,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    );
                    match r {
                        Ok(_) => break,
                        Err(STATE_IN_QUEUE) => {
                            state.store(STATE_DISOWNED, Ordering::Release);
                            (*n).decrease_ctr();
                            break;
                        }
                        Err(STATE_QUEUEING) => {}
                        Err(_) => unreachable!(),
                    }
                }
            })
        };
        let prev = unsafe { (*(*n).own.get()).own_prev };
        let next = unsafe { (*(*n).own.get()).own_next };
        unsafe { (*(*prev).own.get()).own_next = next };
        unsafe { (*(*next).own.get()).own_prev = prev };
        let stream = unsafe { (*(*n).own.get()).stream.assume_init_read() };
        unsafe { Node::drop_self(n) };
        stream
    }

    unsafe fn insert(root: *const Self, stream: S) -> *const Node<S, W, L> {
        unsafe { (*root).wake_stub.increase_ctr() };
        let stub_ptr = unsafe { &raw const (*root).wake_stub };
        let tail_ptr = unsafe { (*(*stub_ptr).own.get()).own_prev };
        let x = Box::leak(Box::new(Node {
            root,
            ctr: AtomicUsize::new(1),
            own: UnsafeCell::new(OwnNode {
                stream: MaybeUninit::new(stream),
                own_next: stub_ptr,
                own_prev: tail_ptr,
            }),
            has_value: AtomicBool::new(true),
            wake_next: std::array::from_fn(|_| {
                CachePadded::new(AtomicConst::new(std::ptr::null()))
            }),
            state: std::array::from_fn(|_| CachePadded::new(AtomicU8::new(STATE_CAN_WAKE))),
        }));
        let n = std::ptr::from_mut(x);
        unsafe { (*(*stub_ptr).own.get()).own_prev = n };
        unsafe { (*(*tail_ptr).own.get()).own_next = n };
        n
    }
}

pub struct Queue<S, const W: usize, const L: usize = W> {
    root: *const Root<S, W, L>,
    phantom: PhantomData<Root<S, W, L>>,
}

impl<S, const W: usize, const L: usize> Default for Queue<S, W, L> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S, const W: usize, const L: usize> Queue<S, W, L> {
    pub fn new() -> Self {
        Self {
            root: Root::new(),
            phantom: PhantomData,
        }
    }

    pub fn insert(&mut self, stream: S) -> Ref<S, W, L> {
        Ref::new(unsafe { Root::insert(self.root, stream) })
    }

    pub fn remove(&mut self, r: &Ref<S, W, L>) -> Option<S> {
        if r.get().has_value.load(Ordering::Acquire) {
            Some(unsafe { Root::remove(r.0) })
        } else {
            None
        }
    }

    pub fn queue_pop_front<const X: usize>(&mut self) -> Option<Ref<S, W, L>> {
        let n = unsafe { (*self.root).pop::<X>() };
        if n.is_null() { None } else { Some(Ref(n)) }
    }

    pub fn queue_push_back<const X: usize>(&self, r: &Ref<S, W, L>) {
        assert_eq!(r.get().root, self.root);
        unsafe { (*self.root).outer_push::<X>(r.get()) };
    }

    pub fn register<const X: usize>(&self, waker: &Waker) {
        unsafe { (*self.root).wakers[X].register(waker) };
    }

    pub fn wake<const X: usize>(&self) {
        unsafe { (*self.root).wake::<X>() };
    }
}

impl<S, const W: usize, const L: usize> Drop for Queue<S, W, L> {
    fn drop(&mut self) {
        {
            while let head_ptr = unsafe { (*(*self.root).wake_stub.own.get()).own_next }
                && head_ptr != unsafe { &raw const (*self.root).wake_stub }
            {
                unsafe { Root::remove(head_ptr) };
            }
        }
        unsafe { Root::drop_self(self.root) };
    }
}

impl<'a, S, const W: usize, const L: usize> Index<&'a Ref<S, W, L>> for Queue<S, W, L> {
    type Output = S;

    fn index(&self, r: &'a Ref<S, W, L>) -> &Self::Output {
        assert_eq!(r.get().root, self.root);
        unsafe { (*r.get().own.get().cast_const()).stream.assume_init_ref() }
    }
}

impl<'a, S, const W: usize, const L: usize> IndexMut<&'a Ref<S, W, L>> for Queue<S, W, L> {
    fn index_mut(&mut self, r: &'a Ref<S, W, L>) -> &mut Self::Output {
        assert_eq!(r.get().root, self.root);
        unsafe { (*r.get().own.get()).stream.assume_init_mut() }
    }
}

#[derive(PartialEq, Eq)]
pub struct Ref<S, const W: usize, const L: usize = W>(*const Node<S, W, L>);

impl<S, const W: usize, const L: usize> std::fmt::Debug for Ref<S, W, L> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Ref").field(&self.0).finish()
    }
}

impl<S, const W: usize, const L: usize> Ref<S, W, L> {
    fn get(&self) -> &Node<S, W, L> {
        unsafe { &*self.0 }
    }

    fn new(n: *const Node<S, W, L>) -> Self {
        unsafe { (*n).increase_ctr() };
        Self(n)
    }

    pub fn waker<const X: usize>(&self) -> Waker {
        self.get().waker::<X>()
    }
}

impl<S, const W: usize, const L: usize> Drop for Ref<S, W, L> {
    fn drop(&mut self) {
        unsafe { Node::drop_self(self.0) }
    }
}

impl<S, const W: usize, const L: usize> Clone for Ref<S, W, L> {
    fn clone(&self) -> Self {
        self.get().increase_ctr();
        Self(self.0)
    }
}

#[test]
fn can_create() {
    drop(Queue::<i32, 1>::new());
}

#[test]
fn can_insert() {
    Queue::<i32, 1>::new().insert(0);
}

#[test]
fn can_insert_2() {
    let mut queue = Queue::<i32, 1>::new();
    queue.insert(0);
    queue.insert(1);
}

#[test]
fn can_insert_3() {
    let mut queue = Queue::<i32, 1>::new();
    queue.insert(0);
    queue.insert(1);
    queue.insert(3);
}

#[test]
fn can_remove() {
    let mut queue = Queue::<i32, 1>::new();
    let r = queue.insert(0);
    assert_eq!(queue.remove(&r).unwrap(), 0);
}

#[test]
fn can_remove_middle() {
    let mut queue = Queue::<i32, 1>::new();
    queue.insert(0);
    let r = queue.insert(1);
    queue.insert(2);
    assert_eq!(queue.remove(&r).unwrap(), 1);
}

#[test]
fn can_clone() {
    let _ = Queue::<i32, 1>::new().insert(0).clone();
}

#[test]
fn can_drop_queue_ref() {
    let mut queue = Queue::<i32, 1>::new();
    let r = queue.insert(0);
    drop(queue);
    drop(r);
}

#[test]
fn can_drop_ref_queue() {
    let mut queue = Queue::<i32, 1>::new();
    let r = queue.insert(0);
    drop(r);
    drop(queue);
}

#[test]
fn can_waker() {
    Queue::<i32, 1>::new().insert(0).waker::<0>();
}

#[test]
fn can_clone_waker() {
    let _ = Queue::<i32, 1>::new().insert(0).waker::<0>().clone();
}

#[test]
fn can_wake() {
    Queue::<i32, 1>::new().insert(0).waker::<0>().wake();
}

#[test]
fn can_wake_by_ref() {
    Queue::<i32, 1>::new().insert(0).waker::<0>().wake_by_ref();
}

#[test]
fn can_pop() {
    let mut queue = Queue::<i32, 1>::new();
    let r = queue.insert(0);
    r.waker::<0>().wake();
    assert_eq!(queue.queue_pop_front::<0>(), Some(r));
    assert_eq!(queue.queue_pop_front::<0>(), None);
}

#[test]
fn can_get_mut() {
    let mut queue = Queue::<i32, 1>::new();
    let r = queue.insert(0);
    queue[&r] = 1;
}

#[test]
fn can_get() {
    let mut queue = Queue::<i32, 1>::new();
    let r = queue.insert(0);
    assert_eq!((&queue[&r], &queue[&r]), (&0, &0));
}

#[test]
fn can_push_back() {
    let mut queue = Queue::<i32, 1>::new();
    let r = queue.insert(0);
    queue.queue_push_back::<0>(&r);
    assert_eq!(queue.queue_pop_front::<0>(), Some(r));
    assert_eq!(queue.queue_pop_front::<0>(), None);
}

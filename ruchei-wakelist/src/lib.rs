//! <https://www.1024cores.net/home/lock-free-algorithms/queues/intrusive-mpsc-node-based-queue>

#![deny(clippy::as_pointer_underscore)]
#![deny(clippy::borrow_as_ptr)]
#![deny(clippy::ptr_as_ptr)]
#![deny(clippy::ptr_cast_constness)]

use std::{
    cell::UnsafeCell,
    marker::PhantomData,
    mem::MaybeUninit,
    ops::Index,
    pin::Pin,
    sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, AtomicUsize, Ordering},
    task::{Context, RawWaker, RawWakerVTable, Waker},
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
    lens: [usize; L],
}

struct Root<S, const W: usize, const L: usize = W> {
    own: UnsafeCell<OwnRoot<S, W, L>>,
    wakers: [AtomicWaker; W],
    wake_head: [CachePadded<AtomicConst<Node<S, W, L>>>; W],
    stub: Node<S, W, L>,
}

struct OwnNode<S, const W: usize, const L: usize = W> {
    up: *const Node<S, W, L>,
    stream: MaybeUninit<S>,
    own_next: *const Node<S, W, L>,
    own_prev: *const Node<S, W, L>,
    link_next: [*mut Self; L],
    link_prev: [*mut Self; L],
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
    unsafe fn from_own(own: *mut OwnNode<S, W, L>) -> *const Self {
        unsafe { (*own).up }
    }

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
        if unsafe { (*root).stub.decrease_ctr() } {
            assert!(unsafe { (*root).is_empty() });
            drop(unsafe { Box::from_raw(root.cast_mut()) });
        }
    }

    unsafe fn is_empty(&self) -> bool {
        std::ptr::from_ref(&self.stub) == unsafe { (*self.stub.own.get()).own_next }
    }

    fn new() -> *const Self {
        let x = std::ptr::from_mut(Box::leak(Box::new(Self {
            own: UnsafeCell::new(OwnRoot {
                wake_tail: [std::ptr::null(); W],
                lens: [0; L],
            }),
            wakers: std::array::from_fn(|_| AtomicWaker::new()),
            wake_head: std::array::from_fn(|_| {
                CachePadded::new(AtomicConst::new(std::ptr::null()))
            }),
            stub: Node {
                root: std::ptr::null(),
                ctr: AtomicUsize::new(1),
                own: UnsafeCell::new(OwnNode {
                    up: std::ptr::null(),
                    stream: MaybeUninit::uninit(),
                    own_next: std::ptr::null_mut(),
                    own_prev: std::ptr::null_mut(),
                    link_prev: [std::ptr::null_mut(); L],
                    link_next: [std::ptr::null_mut(); L],
                }),
                has_value: AtomicBool::new(false),
                wake_next: std::array::from_fn(|_| {
                    CachePadded::new(AtomicConst::new(std::ptr::null()))
                }),
                state: std::array::from_fn(|_| CachePadded::new(AtomicU8::new(STATE_NOT_NODE))),
            },
        })));
        let stub_ptr = unsafe { &raw mut (*x).stub };
        let stub_own_ptr = unsafe { (*stub_ptr).own.get() };
        unsafe {
            (*x).own
                .get_mut()
                .wake_tail
                .iter_mut()
                .for_each(|p| *p = stub_ptr);
            (*x).wake_head
                .iter_mut()
                .for_each(|p| *p.get_mut() = stub_ptr);
            (*x).stub.root = x;
            (*x).stub.own.get_mut().up = stub_ptr;
            (*x).stub.own.get_mut().own_next = stub_ptr;
            (*x).stub.own.get_mut().own_prev = stub_ptr;
            (*x).stub
                .own
                .get_mut()
                .link_next
                .iter_mut()
                .for_each(|p| *p = stub_own_ptr);
            (*x).stub
                .own
                .get_mut()
                .link_prev
                .iter_mut()
                .for_each(|p| *p = stub_own_ptr);
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
        if tail == &raw const self.stub {
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
        self.stub.wake_next[X].store(std::ptr::null(), Ordering::Release);
        self.inner_push::<X>(&self.stub);
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
        unsafe { (*n).has_value.store(false, Ordering::Relaxed) };
        unsafe { Node::drop_self(n) };
        stream
    }

    unsafe fn insert(root: *const Self, stream: S) -> *const Node<S, W, L> {
        unsafe { (*root).stub.increase_ctr() };
        let stub_ptr = unsafe { &raw const (*root).stub };
        let tail_ptr = unsafe { (*(*stub_ptr).own.get()).own_prev };
        let n = Box::leak(Box::new(Node {
            root,
            ctr: AtomicUsize::new(1),
            own: UnsafeCell::new(OwnNode {
                up: std::ptr::null(),
                stream: MaybeUninit::new(stream),
                own_next: stub_ptr,
                own_prev: tail_ptr,
                link_prev: [std::ptr::null_mut(); L],
                link_next: [std::ptr::null_mut(); L],
            }),
            has_value: AtomicBool::new(true),
            wake_next: std::array::from_fn(|_| {
                CachePadded::new(AtomicConst::new(std::ptr::null()))
            }),
            state: std::array::from_fn(|_| CachePadded::new(AtomicU8::new(STATE_CAN_WAKE))),
        }));
        let n = std::ptr::from_mut(n);
        unsafe { (*n).own.get_mut().up = n };
        unsafe { (*(*stub_ptr).own.get()).own_prev = n };
        unsafe { (*(*tail_ptr).own.get()).own_next = n };
        n
    }

    unsafe fn link_stub(root: *const Self) -> *mut OwnNode<S, W, L> {
        unsafe { (*root).stub.own.get() }
    }

    unsafe fn link_len<const X: usize>(root: *const Self) -> usize {
        unsafe { (*(*root).own.get()).lens[X] }
    }

    unsafe fn link_remove<const X: usize>(root: *const Self, n: *mut OwnNode<S, W, L>) {
        let prev = unsafe { (*n).link_prev[X] };
        let next = unsafe { (*n).link_next[X] };
        assert!(!prev.is_null());
        assert!(!next.is_null());
        unsafe { (*prev).link_next[X] = next };
        unsafe { (*next).link_prev[X] = prev };
        unsafe { (*n).link_prev[X] = std::ptr::null_mut() };
        unsafe { (*n).link_next[X] = std::ptr::null_mut() };
        unsafe { (*(*root).own.get()).lens[X] += 1 };
    }

    unsafe fn link_contains<const X: usize>(n: *const OwnNode<S, W, L>) -> bool {
        let prev_null = unsafe { (*n).link_prev[X].is_null() };
        let next_null = unsafe { (*n).link_next[X].is_null() };
        match (prev_null, next_null) {
            (true, true) => false,
            (false, false) => true,
            _ => panic!("inconsistent state"),
        }
    }

    unsafe fn link<const X: usize>(
        root: *const Self,
        n: *mut OwnNode<S, W, L>,
        prev: *mut OwnNode<S, W, L>,
        next: *mut OwnNode<S, W, L>,
    ) {
        assert_ne!(n, prev);
        assert_ne!(n, next);
        assert!(unsafe { !Self::link_contains::<X>(n) });
        assert!(unsafe { Self::link_contains::<X>(prev) });
        assert!(unsafe { Self::link_contains::<X>(next) });
        assert_eq!(unsafe { (*prev).link_next[X] }, next);
        assert_eq!(unsafe { (*next).link_prev[X] }, prev);
        unsafe { (*n).link_prev[X] = prev };
        unsafe { (*n).link_next[X] = next };
        unsafe { (*prev).link_next[X] = n };
        unsafe { (*next).link_prev[X] = n };
        unsafe { (*(*root).own.get()).lens[X] += 1 };
    }

    unsafe fn link_empty<const X: usize>(root: *const Self) -> bool {
        let stub = unsafe { Self::link_stub(root) };
        let prev = unsafe { (*stub).link_prev[X] };
        let next = unsafe { (*stub).link_next[X] };
        let prev_stub = prev == stub;
        let next_stub = next == stub;
        match (prev_stub, next_stub) {
            (true, true) => true,
            (false, false) => false,
            _ => panic!("inconsistent state"),
        }
    }

    unsafe fn link_front<const X: usize>(root: *const Self) -> *mut OwnNode<S, W, L> {
        unsafe { (*(*root).stub.own.get().cast_const()).link_next[X] }
    }

    unsafe fn link_back<const X: usize>(root: *const Self) -> *mut OwnNode<S, W, L> {
        unsafe { (*(*root).stub.own.get().cast_const()).link_prev[X] }
    }

    unsafe fn link_push_front<const X: usize>(root: *const Self, n: *mut OwnNode<S, W, L>) {
        unsafe { Self::link::<X>(root, n, Self::link_stub(root), Self::link_front::<X>(root)) }
    }

    unsafe fn link_push_back<const X: usize>(root: *const Self, n: *mut OwnNode<S, W, L>) {
        unsafe { Self::link::<X>(root, n, Self::link_back::<X>(root), Self::link_stub(root)) }
    }

    unsafe fn link_pop_front<const X: usize>(root: *const Self) -> *mut OwnNode<S, W, L> {
        let n = unsafe { Self::link_front::<X>(root) };
        unsafe { Self::link_remove::<X>(root, n) };
        n
    }

    unsafe fn link_pop_back<const X: usize>(root: *const Self) -> *mut OwnNode<S, W, L> {
        let n = unsafe { Self::link_back::<X>(root) };
        unsafe { Self::link_remove::<X>(root, n) };
        n
    }

    fn own_node(root: *const Self, node: &Node<S, W, L>) -> *mut OwnNode<S, W, L> {
        assert_eq!(root, node.root);
        assert!(node.has_value.load(Ordering::Acquire));
        node.own.get()
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

    pub fn link_len<const X: usize>(&self) -> usize {
        unsafe { Root::link_len::<X>(self.root) }
    }

    pub fn link_front<const X: usize>(&self) -> Option<Ref<S, W, L>> {
        unsafe {
            if Root::link_empty::<X>(self.root) {
                None
            } else {
                Some(Ref::from_own(Root::link_front::<X>(self.root)))
            }
        }
    }

    pub fn link_back<const X: usize>(&self) -> Option<Ref<S, W, L>> {
        unsafe {
            if Root::link_empty::<X>(self.root) {
                None
            } else {
                Some(Ref::from_own(Root::link_back::<X>(self.root)))
            }
        }
    }

    pub fn link_push_front<const X: usize>(&mut self, r: &Ref<S, W, L>) -> bool {
        unsafe {
            if Root::link_contains::<X>(r.own()) {
                false
            } else {
                Root::link_push_front::<X>(self.root, r.own());
                true
            }
        }
    }

    pub fn link_push_back<const X: usize>(&mut self, r: &Ref<S, W, L>) -> bool {
        unsafe {
            if Root::link_contains::<X>(r.own()) {
                false
            } else {
                Root::link_push_back::<X>(self.root, r.own());
                true
            }
        }
    }

    pub fn link_pop_front<const X: usize>(&mut self) -> Option<Ref<S, W, L>> {
        unsafe {
            if Root::link_empty::<X>(self.root) {
                None
            } else {
                Some(Ref::from_own(Root::link_pop_front::<X>(self.root)))
            }
        }
    }

    pub fn link_pop_back<const X: usize>(&mut self) -> Option<Ref<S, W, L>> {
        unsafe {
            if Root::link_empty::<X>(self.root) {
                None
            } else {
                Some(Ref::from_own(Root::link_pop_back::<X>(self.root)))
            }
        }
    }

    pub fn queue_pull<const X: usize>(&mut self) {
        while let Some(r) = self.queue_pop_front::<X>() {
            self.link_push_back::<X>(&r);
        }
    }

    pub fn queue_poll<const X: usize>(&mut self, cx: &mut Context<'_>) {
        self.register::<X>(cx.waker());
        self.queue_pull::<X>();
    }

    pub fn index_pin_mut(&mut self, r: &Ref<S, W, L>) -> Pin<&mut S> {
        unsafe {
            Pin::new_unchecked(
                (*Root::own_node(self.root, r.get()))
                    .stream
                    .assume_init_mut(),
            )
        }
    }
}

impl<S, const W: usize, const L: usize> Drop for Queue<S, W, L> {
    fn drop(&mut self) {
        {
            while let head_ptr = unsafe { (*(*self.root).stub.own.get()).own_next }
                && head_ptr != unsafe { &raw const (*self.root).stub }
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
        unsafe {
            (*Root::own_node(self.root, r.get()).cast_const())
                .stream
                .assume_init_ref()
        }
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

    unsafe fn from_own(n: *mut OwnNode<S, W, L>) -> Self {
        Self::new(unsafe { Node::from_own(n) })
    }

    fn own(&self) -> *mut OwnNode<S, W, L> {
        let n = self.get();
        Root::own_node(n.root, n)
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
    *queue.index_pin_mut(&r) = 1;
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

#[test]
fn can_link() {
    let mut queue = Queue::<i32, 0, 1>::new();
    let a = queue.insert(0);
    let b = queue.insert(1);
    let c = queue.insert(2);
    assert!(queue.link_push_back::<0>(&a));
    assert!(queue.link_push_back::<0>(&b));
    assert!(queue.link_push_back::<0>(&c));
    assert_eq!(queue.link_len::<0>(), 3);
    assert_eq!(queue.link_pop_front::<0>(), Some(a));
    assert_eq!(queue.link_pop_front::<0>(), Some(b));
    assert_eq!(queue.link_pop_front::<0>(), Some(c));
    assert_eq!(queue.link_pop_front::<0>(), None);
}

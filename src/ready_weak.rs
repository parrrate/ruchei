use std::{
    hash::Hash,
    sync::{Arc, Weak},
    task::{Context, Wake, Waker},
};

use futures_util::task::AtomicWaker;

use crate::{collections::linked_hash_set::LinkedHashSet, route::Key};

type Mutex<K> = std::sync::Mutex<LinkedHashSet<K>>;

type Guard<'a, K> = std::sync::MutexGuard<'a, LinkedHashSet<K>>;

pub(crate) struct Ready<K>(Arc<Mutex<K>>);

impl<K: Hash + Eq> Default for Ready<K> {
    fn default() -> Self {
        Self(Default::default())
    }
}

pub(crate) struct ReadyWeak<K>(Weak<Mutex<K>>);

impl<K> Default for ReadyWeak<K> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<K> Ready<K> {
    pub(crate) fn lock(&self) -> Guard<'_, K> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn downgrade(&self) -> ReadyWeak<K> {
        ReadyWeak(Arc::downgrade(&self.0))
    }
}

impl<K> ReadyWeak<K> {
    pub(crate) fn lock(&self, f: impl FnOnce(Guard<'_, K>)) {
        if let Some(ready) = self.0.upgrade() {
            f(Ready(ready).lock());
        }
    }
}

impl<K: Key> Ready<K> {
    pub(crate) fn remove(&self, key: &K) {
        self.lock().remove(key);
    }

    pub(crate) fn next(&self) -> Option<K> {
        self.lock().pop_front()
    }
}

impl<K: Key> ReadyWeak<K> {
    pub(crate) fn insert(&self, key: K) {
        self.lock(|mut ready| {
            ready.insert_if_absent(key);
        });
    }
}

impl<K: Key> Extend<K> for ReadyWeak<K> {
    fn extend<T: IntoIterator<Item = K>>(&mut self, iter: T) {
        self.lock(|mut ready| ready.extend(iter))
    }
}

pub(crate) struct ConnectionWaker<K> {
    pub(crate) waker: AtomicWaker,
    ready: ReadyWeak<K>,
    key: K,
}

impl<K> ConnectionWaker<K> {
    pub(crate) fn new(key: K, ready: ReadyWeak<K>) -> Arc<Self> {
        Arc::new(Self {
            waker: Default::default(),
            ready,
            key,
        })
    }
}

impl<K: Key> Wake for ConnectionWaker<K> {
    fn wake(self: Arc<Self>) {
        self.ready.insert(self.key.clone());
        self.waker.wake();
    }
}

impl<K: Key> ConnectionWaker<K> {
    pub(crate) fn poll<T>(
        self: &Arc<Self>,
        cx: &mut Context<'_>,
        f: impl FnOnce(&mut Context<'_>) -> T,
    ) -> T {
        self.waker.register(cx.waker());
        self.poll_detached(f)
    }

    pub(crate) fn poll_detached<T>(self: &Arc<Self>, f: impl FnOnce(&mut Context<'_>) -> T) -> T {
        f(&mut Context::from_waker(&Waker::from(self.clone())))
    }
}

pub(crate) struct Connection<K, S> {
    pub(crate) stream: S,
    pub(crate) next: Arc<ConnectionWaker<K>>,
    pub(crate) ready: Arc<ConnectionWaker<K>>,
    pub(crate) flush: Arc<ConnectionWaker<K>>,
    pub(crate) close: Arc<ConnectionWaker<K>>,
}

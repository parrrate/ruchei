use std::{
    hash::Hash,
    sync::{Arc, Weak},
    task::{Context, Wake, Waker},
};

use futures_util::task::AtomicWaker;

use crate::{collections::linked_hash_set::LinkedHashSet, route::Key};

pub(crate) struct Ready<K>(Arc<std::sync::Mutex<LinkedHashSet<K>>>);

impl<K: Hash + Eq> Default for Ready<K> {
    fn default() -> Self {
        Self(Default::default())
    }
}

pub(crate) struct ReadyWeak<K>(Weak<std::sync::Mutex<LinkedHashSet<K>>>);

impl<K> Default for ReadyWeak<K> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<K> Ready<K> {
    pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, LinkedHashSet<K>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn downgrade(&self) -> ReadyWeak<K> {
        ReadyWeak(Arc::downgrade(&self.0))
    }
}

impl<K> ReadyWeak<K> {
    pub(crate) fn lock(&self, f: impl FnOnce(std::sync::MutexGuard<'_, LinkedHashSet<K>>)) {
        if let Some(ready) = self.0.upgrade() {
            f(ready.lock().unwrap_or_else(|e| e.into_inner()));
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

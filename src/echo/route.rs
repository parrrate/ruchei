use std::{
    collections::{HashMap, HashSet, VecDeque},
    pin::Pin,
    sync::{Arc, Weak},
    task::{Context, Poll, Wake, Waker},
};

use futures_util::{task::AtomicWaker, Future, Stream};
use pin_project::pin_project;
use ruchei_route::RouteSink;

use crate::route::Key;

struct Ready<K>(Arc<std::sync::Mutex<HashSet<K>>>);

impl<K> Default for Ready<K> {
    fn default() -> Self {
        Self(Default::default())
    }
}

struct ReadyWeak<K>(Weak<std::sync::Mutex<HashSet<K>>>);

impl<K> Default for ReadyWeak<K> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<K> Ready<K> {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashSet<K>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn downgrade(&self) -> ReadyWeak<K> {
        ReadyWeak(Arc::downgrade(&self.0))
    }
}

impl<K> ReadyWeak<K> {
    fn lock(&self, f: impl FnOnce(std::sync::MutexGuard<'_, HashSet<K>>)) {
        if let Some(ready) = self.0.upgrade() {
            f(ready.lock().unwrap_or_else(|e| e.into_inner()));
        }
    }
}

impl<K: Key> Ready<K> {
    fn next(&self) -> Option<K> {
        let mut set = self.lock();
        let key = set.iter().next()?.clone();
        set.remove(&key);
        Some(key)
    }
}

impl<K: Key> ReadyWeak<K> {
    fn insert(&self, key: K) {
        self.lock(|mut ready| {
            ready.insert(key);
        });
    }
}

struct ConnectionWaker<K> {
    waker: AtomicWaker,
    ready: ReadyWeak<K>,
    key: K,
}

impl<K: Key> ConnectionWaker<K> {
    fn new(key: K, ready: ReadyWeak<K>) -> Arc<Self> {
        ready.insert(key.clone());
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
    fn poll<T>(self: &Arc<Self>, cx: &mut Context<'_>, f: impl FnOnce(&mut Context<'_>) -> T) -> T {
        self.waker.register(cx.waker());
        f(&mut Context::from_waker(&Waker::from(self.clone())))
    }
}

struct Connection<K, T> {
    waker: Arc<ConnectionWaker<K>>,
    msgs: VecDeque<T>,
}

impl<K: Key, T> Connection<K, T> {
    fn new(key: K, ready: ReadyWeak<K>) -> Self {
        Self {
            waker: ConnectionWaker::new(key, ready),
            msgs: Default::default(),
        }
    }
}

#[pin_project]
#[must_use = "futures must be awaited"]
pub struct Echo<S, K, T> {
    #[pin]
    router: S,
    connections: HashMap<K, Connection<K, T>>,
    ready: Ready<K>,
}

impl<K: Key, T, E, S: Stream<Item = Result<(K, T), E>> + RouteSink<K, T, Error = E>> Future
    for Echo<S, K, T>
{
    type Output = Result<(), E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        while let Poll::Ready(o) = this.router.as_mut().poll_next(cx)? {
            if let Some((key, msg)) = o {
                this.connections
                    .entry(key)
                    .or_insert_with_key(|key| Connection::new(key.clone(), this.ready.downgrade()))
                    .msgs
                    .push_back(msg);
            } else {
                return Poll::Ready(Ok(()));
            }
        }
        while let Some(key) = this.ready.next() {
            let mut ready = false;
            while let Some(connection) = this.connections.get_mut(&key) {
                if connection.msgs.is_empty() {
                    match connection
                        .waker
                        .poll(cx, |cx| this.router.as_mut().poll_flush(&key, cx))?
                    {
                        Poll::Ready(()) => {
                            ready = true;
                            this.connections.remove(&key);
                        }
                        Poll::Pending => {
                            break;
                        }
                    }
                } else {
                    match connection
                        .waker
                        .poll(cx, |cx| this.router.as_mut().poll_ready(&key, cx))?
                    {
                        Poll::Ready(()) => {
                            ready = true;
                            this.router
                                .as_mut()
                                .start_send(key.clone(), connection.msgs.pop_front().unwrap())?;
                        }
                        Poll::Pending => {
                            break;
                        }
                    }
                }
            }
            if ready && !this.router.is_routing() {
                let mut guard = this.ready.lock();
                if guard.is_empty() {
                    if let Some(key) = this.connections.keys().next() {
                        guard.insert(key.clone());
                    }
                }
            }
        }
        Poll::Pending
    }
}

pub trait EchoRoute: Sized {
    /// Per-connection unique key.
    type K;
    /// Item yielded and accepted by `self` as [`Stream`]/[`RouteSink`].
    type T;

    fn echo_route(self) -> Echo<Self, Self::K, Self::T> {
        Echo {
            router: self,
            connections: Default::default(),
            ready: Default::default(),
        }
    }
}

impl<K: Key, T, E, S: Stream<Item = Result<(K, T), E>> + RouteSink<K, T, Error = E>> EchoRoute
    for S
{
    type K = K;
    type T = T;
}

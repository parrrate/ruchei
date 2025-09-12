use std::{
    collections::{HashMap, HashSet},
    convert::Infallible,
    hash::Hash,
    pin::Pin,
    sync::{Arc, Weak},
    task::{Context, Poll, Wake, Waker},
};

use futures_util::{ready, task::AtomicWaker, Sink, SinkExt, Stream, StreamExt};
pub use ruchei_route::{RouteExt, RouteSink, WithRoute};

use crate::{
    callback::OnClose,
    pinned_extend::{AutoPinnedExtend, Extending, ExtendingRoute},
};

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

pub trait Key: 'static + Send + Sync + Clone + Hash + PartialEq + Eq {}

impl<K: 'static + Send + Sync + Clone + Hash + PartialEq + Eq> Key for K {}

impl<K: Key> Ready<K> {
    fn remove(&self, key: &K) {
        self.lock().remove(key);
    }

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

impl<K> ConnectionWaker<K> {
    fn new(key: K, ready: ReadyWeak<K>) -> Arc<Self> {
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

struct Connection<K, S> {
    stream: S,
    next: Arc<ConnectionWaker<K>>,
    ready: Arc<ConnectionWaker<K>>,
    flush: Arc<ConnectionWaker<K>>,
    close: Arc<ConnectionWaker<K>>,
}

pub struct Router<K, S, F> {
    connections: HashMap<K, Connection<K, S>>,
    next: Ready<K>,
    close: Ready<K>,
    callback: F,
}

impl<K, S, F> Unpin for Router<K, S, F> {}

impl<K, S, F> AutoPinnedExtend for Router<K, S, F> {}

impl<K: Key, S, F> Router<K, S, F> {
    fn remove<E>(&mut self, key: &K, error: Option<E>)
    where
        F: OnClose<E>,
    {
        self.callback.on_close(error);
        self.next.remove(key);
        self.close.remove(key);
        if let Some(connection) = self.connections.remove(key) {
            connection.next.waker.wake();
            connection.ready.waker.wake();
            connection.flush.waker.wake();
            connection.close.waker.wake();
        }
    }

    pub fn push(&mut self, key: K, stream: S) {
        let next = self.next.downgrade();
        let close = self.close.downgrade();
        next.insert(key.clone());
        close.insert(key.clone());
        let connection = Connection {
            stream,
            next: ConnectionWaker::new(key.clone(), next),
            ready: ConnectionWaker::new(key.clone(), Default::default()),
            flush: ConnectionWaker::new(key.clone(), Default::default()),
            close: ConnectionWaker::new(key.clone(), close),
        };
        self.connections.insert(key, connection);
    }
}

impl<In, K: Key, E, S: Unpin + Stream<Item = Result<In, E>>, F: OnClose<E>> Stream
    for Router<K, S, F>
{
    type Item = Result<(K, In), Infallible>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        while let Some(key) = this.next.next() {
            if let Some(connection) = this.connections.get_mut(&key) {
                if let Poll::Ready(o) = connection
                    .next
                    .poll(cx, |cx| connection.stream.poll_next_unpin(cx))
                {
                    match o {
                        Some(Ok(item)) => {
                            this.next.downgrade().insert(key.clone());
                            return Poll::Ready(Some(Ok((key, item))));
                        }
                        Some(Err(e)) => {
                            this.remove(&key, Some(e));
                        }
                        None => {
                            this.remove(&key, None);
                        }
                    }
                }
            }
        }
        if this.connections.is_empty() {
            Poll::Ready(None)
        } else {
            Poll::Pending
        }
    }
}

impl<Out, K: Key, E, S: Unpin + Sink<Out, Error = E>, F: OnClose<E>> RouteSink<K, Out>
    for Router<K, S, F>
{
    type Error = Infallible;

    fn poll_ready(
        self: Pin<&mut Self>,
        key: &K,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        if let Some(connection) = this.connections.get_mut(key) {
            if let Err(e) = ready!(connection
                .ready
                .poll(cx, |cx| connection.stream.poll_ready_unpin(cx)))
            {
                this.remove(key, Some(e));
            }
        }
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, key: K, msg: Out) -> Result<(), Self::Error> {
        let key = &key;
        let this = self.get_mut();
        if let Some(connection) = this.connections.get_mut(key) {
            if let Err(e) = connection.stream.start_send_unpin(msg) {
                this.remove(key, Some(e));
            }
        }
        Ok(())
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        key: &K,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        if let Some(connection) = this.connections.get_mut(key) {
            if let Err(e) = ready!(connection
                .flush
                .poll(cx, |cx| connection.stream.poll_flush_unpin(cx)))
            {
                this.remove(key, Some(e));
            }
        }
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        while let Some(key) = this.close.next() {
            if let Some(connection) = this.connections.get_mut(&key) {
                if let Poll::Ready(r) = connection
                    .close
                    .poll(cx, |cx| connection.stream.poll_close_unpin(cx))
                {
                    match r {
                        Ok(()) => {
                            this.remove(&key, None);
                        }
                        Err(e) => {
                            this.remove(&key, Some(e));
                        }
                    }
                }
            }
        }
        if this.connections.is_empty() {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }
}

impl<K: Key, S, F> Extend<(K, S)> for Router<K, S, F> {
    fn extend<T: IntoIterator<Item = (K, S)>>(&mut self, iter: T) {
        for (key, stream) in iter {
            self.push(key, stream)
        }
    }
}

pub type RouterExtending<F, R> =
    ExtendingRoute<Router<<R as RouterExt>::K, <R as RouterExt>::S, F>, R>;

pub trait RouterExt: Sized {
    type K;
    type S;
    type E;

    fn route<F: OnClose<Self::E>>(self, callback: F) -> RouterExtending<F, Self>;
}

impl<In, K, E, S: Stream<Item = Result<In, E>>, R: Stream<Item = (K, S)>> RouterExt for R {
    type K = K;
    type S = S;
    type E = E;

    fn route<F: OnClose<Self::E>>(self, callback: F) -> RouterExtending<F, Self> {
        ExtendingRoute(Extending::new(
            self,
            Router {
                connections: Default::default(),
                next: Default::default(),
                close: Default::default(),
                callback,
            },
        ))
    }
}

use std::{
    collections::HashSet,
    convert::Infallible,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{stream::FusedStream, Sink, SinkExt, Stream, StreamExt};
use linked_hash_map::LinkedHashMap;
use linked_hash_set::LinkedHashSet;
use ruchei_callback::OnClose;

use crate::{
    pinned_extend::{AutoPinnedExtend, Extending},
    ready_keyed::{Connection, ConnectionWaker, Ready},
    route::Key,
};

pub struct Multicast<K, S, F> {
    connections: LinkedHashMap<K, Connection<K, S>>,
    next: Ready<K>,
    ready: Ready<K>,
    readied: HashSet<K>,
    started: LinkedHashSet<K>,
    flushing: HashSet<K>,
    flush: Ready<K>,
    close: Ready<K>,
    callback: F,
}

impl<K, S, F> Unpin for Multicast<K, S, F> {}

impl<K, S, F> AutoPinnedExtend for Multicast<K, S, F> {}

impl<K: Key, S, F> Multicast<K, S, F> {
    fn remove<E>(&mut self, key: &K, error: Option<E>)
    where
        F: OnClose<E>,
    {
        self.callback.on_close(error);
        self.next.remove(key);
        self.ready.remove(key);
        self.readied.remove(key);
        self.started.remove(key);
        self.flushing.remove(key);
        self.flush.remove(key);
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
        let ready = self.ready.downgrade();
        let flush = self.flush.downgrade();
        let close = self.close.downgrade();
        next.insert(key.clone());
        ready.insert(key.clone());
        close.insert(key.clone());
        let connection = Connection {
            stream,
            next: ConnectionWaker::new(key.clone(), next),
            ready: ConnectionWaker::new(key.clone(), ready),
            flush: ConnectionWaker::new(key.clone(), flush),
            close: ConnectionWaker::new(key.clone(), close),
        };
        self.connections.insert(key, connection);
    }
}

impl<In, K: Key, E, S: Unpin + Stream<Item = Result<In, E>>, F: OnClose<E>> Stream
    for Multicast<K, S, F>
{
    type Item = Result<In, Infallible>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        while let Some(key) = this.next.next() {
            if let Some(connection) = this.connections.get_mut(&key) {
                if this.readied.remove(&key) {
                    this.ready.downgrade().insert(key.clone());
                }
                if let Poll::Ready(o) = connection
                    .next
                    .poll(cx, |cx| connection.stream.poll_next_unpin(cx))
                {
                    match o {
                        Some(Ok(item)) => {
                            this.next.downgrade().insert(key);
                            return Poll::Ready(Some(Ok(item)));
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

impl<In, K: Key, E, S: Unpin + Stream<Item = Result<In, E>>, F: OnClose<E>> FusedStream
    for Multicast<K, S, F>
{
    fn is_terminated(&self) -> bool {
        self.connections.is_empty()
    }
}

impl<Out: Clone, K: Key, E, S: Unpin + Sink<Out, Error = E>, F: OnClose<E>> Sink<Out>
    for Multicast<K, S, F>
{
    type Error = Infallible;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        while let Some(key) = this.ready.next() {
            if !this.readied.contains(&key) {
                if let Some(connection) = this.connections.get_mut(&key) {
                    if let Poll::Ready(r) = connection
                        .ready
                        .poll(cx, |cx| connection.stream.poll_ready_unpin(cx))
                    {
                        if let Err(e) = r {
                            this.remove(&key, Some(e));
                        } else {
                            this.readied.insert(key);
                        }
                    }
                }
            }
        }
        if this.readied.len() == this.connections.len() {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }

    fn start_send(self: Pin<&mut Self>, msg: Out) -> Result<(), Self::Error> {
        let this = self.get_mut();
        let mut readied = HashSet::new();
        std::mem::swap(&mut readied, &mut this.readied);
        for key in readied.drain() {
            if let Some(connection) = this.connections.get_mut(&key) {
                if let Err(e) = connection.stream.start_send_unpin(msg.clone()) {
                    this.remove(&key, Some(e));
                } else {
                    this.started.insert_if_absent(key.clone());
                    this.ready.downgrade().insert(key);
                }
            }
        }
        std::mem::swap(&mut readied, &mut this.readied);
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        this.flush.downgrade().extend(
            this.started
                .drain()
                .filter(|key| !this.flushing.contains(key)),
        );
        while let Some(key) = this.flush.next() {
            if let Some(connection) = this.connections.get_mut(&key) {
                if this.readied.remove(&key) {
                    this.ready.downgrade().insert(key.clone());
                }
                match connection
                    .flush
                    .poll(cx, |cx| connection.stream.poll_flush_unpin(cx))
                {
                    Poll::Ready(Ok(())) => {
                        this.flushing.remove(&key);
                    }
                    Poll::Ready(Err(e)) => {
                        this.remove(&key, Some(e));
                    }
                    Poll::Pending => {
                        this.flushing.insert(key);
                    }
                }
            }
        }
        if this.flushing.is_empty() {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
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

impl<K: Key, S, F> Extend<(K, S)> for Multicast<K, S, F> {
    fn extend<T: IntoIterator<Item = (K, S)>>(&mut self, iter: T) {
        for (key, stream) in iter {
            self.push(key, stream)
        }
    }
}

pub type MulticastExtending<F, R> = Extending<
    Multicast<<R as MulticastBufferlessKeyed>::K, <R as MulticastBufferlessKeyed>::S, F>,
    R,
>;

pub trait MulticastBufferlessKeyed: Sized {
    /// Key.
    type K;
    /// Single [`Stream`]/[`Sink`].
    type S;
    /// Error.
    type E;

    #[must_use]
    fn multicast_bufferless_keyed<F: OnClose<Self::E>>(
        self,
        callback: F,
    ) -> MulticastExtending<F, Self>;
}

impl<In, K: Key, E, S: Unpin + Stream<Item = Result<In, E>>, R: FusedStream<Item = (K, S)>>
    MulticastBufferlessKeyed for R
{
    type K = K;
    type S = S;
    type E = E;

    fn multicast_bufferless_keyed<F: OnClose<Self::E>>(
        self,
        callback: F,
    ) -> MulticastExtending<F, Self> {
        Extending::new(
            self,
            Multicast {
                connections: Default::default(),
                next: Default::default(),
                ready: Default::default(),
                readied: Default::default(),
                started: Default::default(),
                flushing: Default::default(),
                flush: Default::default(),
                close: Default::default(),
                callback,
            },
        )
    }
}

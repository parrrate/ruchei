use std::{
    collections::{HashSet, VecDeque},
    convert::Infallible,
    hash::Hash,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{
    Sink, SinkExt, Stream, TryStream, TryStreamExt, ready, stream::FusedStream, task::AtomicWaker,
};
use linked_hash_map::LinkedHashMap;
use linked_hash_set::LinkedHashSet;

use crate::{
    multi_item::MultiItem,
    pinned_extend::{AutoPinnedExtend, Extending},
    ready_keyed::{Connection, ConnectionWaker, Ready},
    route::Key,
};

/// [`Sink`]/[`Stream`] implemented over the stream of incoming [`Sink`]s/[`Stream`]s.
pub struct Dealer<K, S, E> {
    connections: LinkedHashMap<K, Connection<K, S>>,
    next: Ready<K>,
    wready: AtomicWaker,
    started: LinkedHashSet<K>,
    flushing: HashSet<K>,
    flush: Ready<K>,
    close: Ready<K>,
    closed: VecDeque<(S, Option<E>)>,
}

impl<K: Hash + Eq, S, E> Default for Dealer<K, S, E> {
    fn default() -> Self {
        Self {
            connections: Default::default(),
            next: Default::default(),
            wready: Default::default(),
            started: Default::default(),
            flushing: Default::default(),
            flush: Default::default(),
            close: Default::default(),
            closed: Default::default(),
        }
    }
}

impl<K, S, E> Unpin for Dealer<K, S, E> {}

impl<K, S, E> AutoPinnedExtend for Dealer<K, S, E> {}

impl<K: Key, S, E> Dealer<K, S, E> {
    fn remove(&mut self, key: &K, error: Option<E>) {
        self.next.remove(key);
        self.wready.wake();
        self.started.remove(key);
        self.flushing.remove(key);
        self.flush.remove(key);
        self.close.remove(key);
        let connection = self.connections.remove(key).expect("unknown key");
        connection.next.waker.wake();
        connection.ready.waker.wake();
        connection.flush.waker.wake();
        connection.close.waker.wake();
        self.closed.push_back((connection.stream, error));
    }

    /// Add new connection with its unique key.
    pub fn push(&mut self, key: K, stream: S) {
        let next = self.next.downgrade();
        let flush = self.flush.downgrade();
        let close = self.close.downgrade();
        next.insert(key.clone());
        self.wready.wake();
        close.insert(key.clone());
        let connection = Connection {
            stream,
            next: ConnectionWaker::new(key.clone(), next),
            ready: ConnectionWaker::new(key.clone(), Default::default()),
            flush: ConnectionWaker::new(key.clone(), flush),
            close: ConnectionWaker::new(key.clone(), close),
        };
        self.connections.insert(key, connection);
    }
}

impl<In, K: Key, E, S: Unpin + TryStream<Ok = In, Error = E>> Stream for Dealer<K, S, E> {
    type Item = MultiItem<In, S, E>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if let Some((stream, error)) = this.closed.pop_front() {
            return Poll::Ready(Some(MultiItem::Closed(stream, error)));
        }
        while let Some(key) = this.next.next() {
            if let Some(connection) = this.connections.get_mut(&key)
                && let Poll::Ready(o) = connection
                    .next
                    .poll(cx, |cx| connection.stream.try_poll_next_unpin(cx))
            {
                match o {
                    Some(Ok(item)) => {
                        this.next.downgrade().insert(key);
                        return Poll::Ready(Some(MultiItem::Item(item)));
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
        if this.connections.is_empty() {
            Poll::Ready(None)
        } else {
            Poll::Pending
        }
    }
}

impl<In, K: Key, E, S: Unpin + TryStream<Ok = In, Error = E>> FusedStream for Dealer<K, S, E> {
    fn is_terminated(&self) -> bool {
        self.connections.is_empty()
    }
}

impl<Out, K: Key, E, S: Unpin + Sink<Out, Error = E>> Sink<Out> for Dealer<K, S, E> {
    type Error = Infallible;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        this.wready.register(cx.waker());
        if let Some((key, _)) = this.connections.front() {
            let key = key.clone();
            if let Some(connection) = this.connections.get_mut(&key) {
                if let Err(e) = ready!(
                    connection
                        .ready
                        .poll(cx, |cx| connection.stream.poll_ready_unpin(cx))
                ) {
                    this.remove(&key, Some(e));
                } else {
                    return Poll::Ready(Ok(()));
                }
            }
        }
        Poll::Pending
    }

    fn start_send(self: Pin<&mut Self>, msg: Out) -> Result<(), Self::Error> {
        let this = self.get_mut();
        if let Some((key, _)) = this.connections.front() {
            let key = key.clone();
            let connection = this
                .connections
                .get_refresh(&key)
                .expect("first key must point to an existing entry");
            if let Err(e) = connection.stream.start_send_unpin(msg) {
                this.remove(&key, Some(e));
            } else {
                this.started.insert_if_absent(key);
            }
        };
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
            if let Some(connection) = this.connections.get_mut(&key)
                && let Poll::Ready(r) = connection
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
        if this.connections.is_empty() {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }
}

impl<K: Key, S, F> Extend<(K, S)> for Dealer<K, S, F> {
    fn extend<T: IntoIterator<Item = (K, S)>>(&mut self, iter: T) {
        for (key, stream) in iter {
            self.push(key, stream)
        }
    }
}

/// [`Sink`]/[`Stream`] Returned by [`DealerKeyedExt::deal_keyed`].
pub type DealerExtending<R> = Extending<
    Dealer<<R as DealerKeyedExt>::K, <R as DealerKeyedExt>::S, <R as DealerKeyedExt>::E>,
    R,
>;

/// Extension trait to auto-extend a [`Dealer`] from a stream of connections.
pub trait DealerKeyedExt: Sized {
    /// Key.
    type K;
    /// Single [`Stream`]/[`Sink`].
    type S;
    /// Error.
    type E;

    /// Extend the stream of connections (`self`) into a [`Dealer`].
    #[must_use]
    fn deal_keyed(self) -> DealerExtending<Self>;
}

impl<In, K: Key, E, S: Unpin + TryStream<Ok = In, Error = E>, R: FusedStream<Item = (K, S)>>
    DealerKeyedExt for R
{
    type K = K;
    type S = S;
    type E = E;

    fn deal_keyed(self) -> DealerExtending<Self> {
        Extending::new(self, Default::default())
    }
}

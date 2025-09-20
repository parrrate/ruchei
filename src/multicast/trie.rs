use std::{
    borrow::Borrow,
    collections::VecDeque,
    convert::Infallible,
    pin::Pin,
    task::{Context, Poll},
};

use extend_pinned::ExtendPinned;
use futures_util::{Sink, SinkExt, Stream, TryStream, TryStreamExt, stream::FusedStream};
use pin_project::pin_project;
use ruchei_collections::{
    as_linked_slab::{AsLinkedSlab, SlabKey},
    linked_slab_multi_trie::LinkedSlabMultiTrie,
    multi_trie::{MultiTrie, MultiTrieAddOwned, MultiTriePrefix},
};
use ruchei_connection::{Connection, ConnectionWaker, Ready};
use ruchei_extend::{Extending, ExtendingExt};

use crate::multi_item::MultiItem;

const OP_WAKE_NEXT: usize = 0;
const OP_WAKE_READY: usize = 1;
const OP_WAKE_FLUSH: usize = 2;
const OP_WAKE_CLOSE: usize = 3;
const OP_IS_STARTED: usize = 4;
const OP_IS_READIED: usize = 5;
const OP_IS_FLUSHING: usize = 6;
const OP_COUNT: usize = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SubRequest<K, O> {
    Sub(K),
    Unsub(K),
    Other(O),
}

#[pin_project]
#[derive(Debug)]
pub struct Multicast<S, E = <S as TryStream>::Error> {
    connections: LinkedSlabMultiTrie<Connection<S>, OP_COUNT>,
    #[pin]
    next: Ready,
    #[pin]
    ready: Ready,
    #[pin]
    flush: Ready,
    #[pin]
    close: Ready,
    closed: VecDeque<(S, Option<E>)>,
}

impl<S, E> Default for Multicast<S, E> {
    fn default() -> Self {
        Self {
            connections: Default::default(),
            next: Default::default(),
            ready: Default::default(),
            flush: Default::default(),
            close: Default::default(),
            closed: Default::default(),
        }
    }
}

impl<S, E> Multicast<S, E> {
    fn remove(self: Pin<&mut Self>, key: SlabKey, error: Option<E>) {
        let this = self.project();
        let connection = this.connections.remove(key);
        connection.next.wake();
        connection.ready.wake();
        connection.flush.wake();
        connection.close.wake();
        this.closed.push_back((connection.stream, error));
        this.next.wake();
    }

    pub fn push(self: Pin<&mut Self>, stream: S) {
        let this = self.project();
        let key = this.connections.vacant_key();
        let next = this.next.downgrade();
        let ready = this.ready.downgrade();
        let flush = this.flush.downgrade();
        let close = this.close.downgrade();
        let connection = Connection {
            stream,
            next: ConnectionWaker::new(key, next),
            ready: ConnectionWaker::new(key, ready),
            flush: ConnectionWaker::new(key, flush),
            close: ConnectionWaker::new(key, close),
        };
        assert_eq!(this.connections.insert(connection), key);
        this.connections.link_push_back::<OP_WAKE_NEXT>(key);
        this.connections.link_push_back::<OP_WAKE_READY>(key);
        this.connections.link_push_back::<OP_WAKE_CLOSE>(key);
        this.next.wake();
        this.ready.wake();
        this.close.wake();
    }
}

impl<K: AsRef<[u8]>, O, E, S: Unpin + TryStream<Ok = SubRequest<K, O>, Error = E>> Stream
    for Multicast<S, E>
{
    type Item = MultiItem<S, O>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.as_mut().project();
        if let Some((stream, error)) = this.closed.pop_front() {
            return Poll::Ready(Some(MultiItem::Closed(stream, error)));
        }
        this.next.register(cx);
        while let Some(key) = this.next.as_mut().next::<OP_WAKE_NEXT>(this.connections) {
            if this.connections.link_pop_at::<OP_IS_READIED>(key) {
                this.ready.downgrade().insert(key);
            }
            if this.connections.link_pop_at::<OP_IS_FLUSHING>(key) {
                this.flush.downgrade().insert(key);
            }
            if let Some(connection) = this.connections.get_mut(key)
                && let Poll::Ready(o) = connection
                    .next
                    .poll(cx, |cx| connection.stream.try_poll_next_unpin(cx))
            {
                match o {
                    Some(Ok(item)) => {
                        this.next.downgrade().insert(key);
                        match item {
                            SubRequest::Sub(route) => {
                                this.connections.mt_add_owned(key, route.as_ref())
                            }
                            SubRequest::Unsub(route) => {
                                this.connections.mt_remove(&key, route.as_ref())
                            }
                            SubRequest::Other(item) => {
                                return Poll::Ready(Some(MultiItem::Item(item)));
                            }
                        }
                    }
                    Some(Err(e)) => {
                        self.as_mut().remove(key, Some(e));
                    }
                    None => {
                        self.as_mut().remove(key, None);
                    }
                }
            }
            this = self.as_mut().project();
        }
        if this.connections.is_empty() {
            Poll::Ready(None)
        } else {
            Poll::Pending
        }
    }
}

impl<K: AsRef<[u8]>, O, E, S: Unpin + TryStream<Ok = SubRequest<K, O>, Error = E>> FusedStream
    for Multicast<S, E>
{
    fn is_terminated(&self) -> bool {
        self.connections.is_empty()
    }
}

impl<K: Clone + AsRef<[u8]>, O: Clone, E, S: Unpin + Sink<(K, O), Error = E>> Sink<(K, O)>
    for Multicast<S, E>
{
    type Error = Infallible;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let mut this = self.as_mut().project();
        this.ready.register(cx);
        while let Some(key) = this.ready.as_mut().next::<OP_WAKE_READY>(this.connections) {
            if !this.connections.link_contains::<OP_IS_READIED>(key)
                && let Some(connection) = this.connections.get_mut(key)
                && let Poll::Ready(r) = connection
                    .ready
                    .poll(cx, |cx| connection.stream.poll_ready_unpin(cx))
            {
                if let Err(e) = r {
                    self.as_mut().remove(key, Some(e));
                } else {
                    this.connections.link_push_back::<OP_IS_READIED>(key);
                }
            }
            this = self.as_mut().project();
        }
        if this.connections.link_len::<OP_IS_READIED>() == this.connections.len() {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }

    fn start_send(mut self: Pin<&mut Self>, msg: (K, O)) -> Result<(), Self::Error> {
        let mut this = self.as_mut().project();
        let keys = this
            .connections
            .mt_prefix_collect(msg.0.as_ref())
            .into_iter()
            .map(|item| *item.borrow())
            .collect::<Vec<_>>();
        for key in keys {
            if this.connections.link_pop_at::<OP_IS_READIED>(key)
                && let Some(connection) = this.connections.get_mut(key)
            {
                if let Err(e) = connection.stream.start_send_unpin(msg.clone()) {
                    self.as_mut().remove(key, Some(e));
                } else {
                    this.connections.link_push_back::<OP_IS_STARTED>(key);
                    if this.connections.link_contains::<OP_IS_FLUSHING>(key) {
                        this.connections.link_pop_at::<OP_IS_FLUSHING>(key);
                        this.flush.wake();
                    }
                    this.ready.downgrade().insert(key);
                }
            }
            this = self.as_mut().project();
        }
        Ok(())
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let mut this = self.as_mut().project();
        this.flush.register(cx);
        this.flush.downgrade().extend(
            this.connections
                .link_pops::<OP_IS_STARTED, _, _>(|key, _| key),
        );
        while let Some(key) = this.flush.as_mut().next::<OP_WAKE_FLUSH>(this.connections) {
            if let Some(connection) = this.connections.get_mut(key) {
                this.ready.downgrade().insert(key);
                match connection
                    .flush
                    .poll(cx, |cx| connection.stream.poll_flush_unpin(cx))
                {
                    Poll::Ready(Ok(())) => {
                        this.connections.link_pop_at::<OP_IS_FLUSHING>(key);
                    }
                    Poll::Ready(Err(e)) => {
                        self.as_mut().remove(key, Some(e));
                    }
                    Poll::Pending => {
                        this.connections.link_push_back::<OP_IS_FLUSHING>(key);
                    }
                }
            }
            this = self.as_mut().project();
        }
        if this.connections.link_empty::<OP_IS_FLUSHING>() {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let mut this = self.as_mut().project();
        this.close.register(cx);
        while let Some(key) = this.close.as_mut().next::<OP_WAKE_CLOSE>(this.connections) {
            if let Some(connection) = this.connections.get_mut(key)
                && let Poll::Ready(r) = connection
                    .close
                    .poll(cx, |cx| connection.stream.poll_close_unpin(cx))
            {
                match r {
                    Ok(()) => {
                        self.as_mut().remove(key, None);
                    }
                    Err(e) => {
                        self.as_mut().remove(key, Some(e));
                    }
                }
            }
            this = self.as_mut().project();
        }
        if this.connections.is_empty() {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }
}

impl<S, E> ExtendPinned<S> for Multicast<S, E> {
    fn extend_pinned<T: IntoIterator<Item = S>>(mut self: Pin<&mut Self>, iter: T) {
        for stream in iter {
            self.as_mut().push(stream);
        }
    }
}

pub type MulticastExtending<R> = Extending<Multicast<<R as Stream>::Item>, R>;

pub trait MulticastTrie: Sized + FusedStream<Item: TryStream> {
    #[must_use]
    fn multicast_trie(self) -> MulticastExtending<Self> {
        self.extending_default()
    }
}

impl<
    O,
    K: AsRef<[u8]>,
    E,
    S: Unpin + TryStream<Ok = SubRequest<K, O>, Error = E>,
    R: FusedStream<Item = S>,
> MulticastTrie for R
{
}

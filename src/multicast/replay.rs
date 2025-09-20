use std::{
    collections::VecDeque,
    convert::Infallible,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Wake},
};

use extend_pinned::ExtendPinned;
use futures_util::{
    Sink, SinkExt, Stream, TryStream, TryStreamExt, ready, stream::FusedStream, task::AtomicWaker,
};
use pin_project::pin_project;
use ruchei_collections::{
    as_linked_slab::{AsLinkedSlab, SlabKey},
    linked_slab::LinkedSlab,
};
use ruchei_connection::{ConnectionWaker, Ready};
use ruchei_extend::{Extending, ExtendingExt};

use crate::connection_item::ConnectionItem;

const OP_WAKE_NEXT: usize = 0;
const OP_WAKE_READY: usize = 1;
const OP_WAKE_FLUSH: usize = 1;
const OP_WAKE_CLOSE: usize = 2;
/// `start`ed, haven't yet reached the `flush_target`
const OP_IS_S_PRE_F: usize = 3;
/// `start`ed, already reached the `flush_target`
const OP_IS_S_POST_F: usize = 4;
const OP_IS_BEHIND: usize = 5;
const OP_IS_NOT_BEHIND: usize = 6;
/// `OP_IS_S_PRE_F` and `OP_IS_NOT_BEHIND`
const OP_IS_FLUSHING: usize = 7;
const OP_COUNT: usize = 8;

#[derive(Debug)]
pub(crate) struct Connection<S> {
    pub(crate) stream: S,
    pub(crate) next: Arc<ConnectionWaker>,
    pub(crate) ready: Arc<ConnectionWaker>,
    pub(crate) flush: Arc<ConnectionWaker>,
    pub(crate) close: Arc<ConnectionWaker>,
    sent: usize,
    flushed: usize,
}

#[derive(Debug, Default)]
struct NextFlush {
    next: AtomicWaker,
    flush: AtomicWaker,
}

impl Wake for NextFlush {
    fn wake(self: Arc<Self>) {
        self.next.wake();
        self.flush.wake();
    }
}

#[pin_project]
#[derive(Debug)]
pub struct Multicast<S, T, E = <S as TryStream>::Error> {
    connections: LinkedSlab<Connection<S>, OP_COUNT>,
    #[pin]
    next: Ready,
    #[pin]
    ready: Ready,
    #[pin]
    flush: Ready,
    #[pin]
    close: Ready,
    items: Vec<T>,
    flush_target: usize,
    next_flush: Arc<NextFlush>,
    closed: VecDeque<(S, Option<E>)>,
}

impl<S, T, E> Default for Multicast<S, T, E> {
    fn default() -> Self {
        Self {
            connections: Default::default(),
            next: Default::default(),
            ready: Default::default(),
            flush: Default::default(),
            close: Default::default(),
            items: Default::default(),
            flush_target: Default::default(),
            next_flush: Default::default(),
            closed: Default::default(),
        }
    }
}

impl<S: Unpin + Sink<T, Error = E>, T: Clone, E> Multicast<S, T, E> {
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
            sent: 0,
            flushed: 0,
        };
        this.connections.insert_at(key, connection);
        this.connections.link_push_back::<OP_WAKE_NEXT>(key);
        this.connections.link_push_back::<OP_WAKE_READY>(key);
        this.connections.link_push_back::<OP_WAKE_CLOSE>(key);
        this.next.wake();
        this.ready.wake();
        this.close.wake();
        if this.items.is_empty() {
            this.connections.link_push_back::<OP_IS_NOT_BEHIND>(key);
        } else {
            this.connections.link_push_back::<OP_IS_BEHIND>(key);
        }
    }

    fn start_flush_one(self: Pin<&mut Self>, key: SlabKey) {
        let this = self.project();
        assert!(this.connections.link_contains::<OP_IS_NOT_BEHIND>(key));
        assert!(this.connections[key].sent == this.items.len());
        assert!(this.connections.link_contains::<OP_IS_S_PRE_F>(key));
        assert!(this.connections[key].flushed < *this.flush_target);
        assert!(this.connections.link_push_back::<OP_IS_FLUSHING>(key));
        this.flush.downgrade().insert(key);
    }

    /// wait until `sent` reaches `items.len()`
    fn poll_send_one(
        mut self: Pin<&mut Self>,
        key: SlabKey,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), S::Error>> {
        let this = self.as_mut().project();
        assert!(this.connections.link_contains::<OP_IS_BEHIND>(key));
        while this.connections[key].sent < this.items.len() {
            ready!(this.connections[key].stream.poll_ready_unpin(cx))?;
            let item = this.items[this.connections[key].sent].clone();
            this.connections[key].stream.start_send_unpin(item)?;
            this.connections[key].sent += 1;
            if !this.connections.link_contains::<OP_IS_S_PRE_F>(key) {
                if this.connections[key].flushed < *this.flush_target {
                    this.connections.link_push_back::<OP_IS_S_PRE_F>(key);
                } else {
                    this.connections.link_push_back::<OP_IS_S_POST_F>(key);
                }
            }
        }
        assert!(this.connections.link_pop_at::<OP_IS_BEHIND>(key));
        assert!(this.connections.link_push_back::<OP_IS_NOT_BEHIND>(key));
        if this.connections.link_contains::<OP_IS_S_PRE_F>(key) {
            self.as_mut().start_flush_one(key);
        }
        Poll::Ready(Ok(()))
    }

    /// wait until `flushed` reaches `flush_target`
    fn poll_flush_one(
        self: Pin<&mut Self>,
        key: SlabKey,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), S::Error>> {
        let this = self.project();
        assert!(this.connections.link_contains::<OP_IS_FLUSHING>(key));
        assert!(this.connections.link_contains::<OP_IS_NOT_BEHIND>(key));
        assert!(this.connections.link_contains::<OP_IS_S_PRE_F>(key));
        ready!(this.connections[key].stream.poll_flush_unpin(cx))?;
        this.connections[key].flushed = this.connections[key].sent;
        assert!(this.connections.link_pop_at::<OP_IS_FLUSHING>(key));
        assert!(this.connections.link_pop_at::<OP_IS_S_PRE_F>(key));
        Poll::Ready(Ok(()))
    }

    fn poll_send_all(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut this = self.as_mut().project();
        this.ready.register(cx);
        while let Some(key) = this.ready.as_mut().next::<OP_WAKE_READY>(this.connections) {
            if this.connections.link_contains::<OP_IS_BEHIND>(key)
                && let Some(connection) = this.connections.get_mut(key)
                && let Poll::Ready(Err(e)) = connection
                    .ready
                    .clone()
                    .poll(cx, |cx| self.as_mut().poll_send_one(key, cx))
            {
                self.as_mut().remove(key, Some(e));
            }
            this = self.as_mut().project();
        }
        if this.connections.link_empty::<OP_IS_BEHIND>() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }

    fn poll_flush_all(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut this = self.as_mut().project();
        this.flush.register(cx);
        while let Some(key) = this.flush.as_mut().next::<OP_WAKE_FLUSH>(this.connections) {
            if this.connections.link_contains::<OP_IS_FLUSHING>(key)
                && let Some(connection) = this.connections.get_mut(key)
                && let Poll::Ready(Err(e)) = connection
                    .flush
                    .clone()
                    .poll(cx, |cx| self.as_mut().poll_flush_one(key, cx))
            {
                self.as_mut().remove(key, Some(e));
            }
            this = self.as_mut().project();
        }
        if this.connections.link_empty::<OP_IS_FLUSHING>() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }

    fn poll_send_flush(mut self: Pin<&mut Self>) -> Poll<()> {
        let waker = self.next_flush.clone().into();
        let cx = &mut Context::from_waker(&waker);
        let sent = self.as_mut().poll_send_all(cx);
        ready!(self.as_mut().poll_flush_all(cx));
        sent
    }

    fn start_flush(mut self: Pin<&mut Self>) {
        let mut this = self.as_mut().project();
        assert!(*this.flush_target < this.items.len());
        *this.flush_target = this.items.len();
        while let Some(key) = this.connections.link_pop_front::<OP_IS_S_POST_F>() {
            assert!(this.connections[key].flushed < *this.flush_target);
            this.connections.link_push_back::<OP_IS_S_PRE_F>(key);
            if this.connections.link_contains::<OP_IS_NOT_BEHIND>(key) {
                self.as_mut().start_flush_one(key);
                this = self.as_mut().project();
            }
        }
    }
}

impl<S: Unpin + TryStream<Error = E> + Sink<T, Error = E>, T: Clone, E> Stream
    for Multicast<S, T, E>
{
    type Item = ConnectionItem<S>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.next_flush.next.register(cx.waker());
        let _ = self.as_mut().poll_send_flush();
        let mut this = self.as_mut().project();
        if let Some((stream, error)) = this.closed.pop_front() {
            return Poll::Ready(Some(ConnectionItem::Closed(stream, error)));
        }
        while let Some(key) = this.next.as_mut().next::<OP_WAKE_NEXT>(this.connections) {
            if let Some(connection) = this.connections.get_mut(key)
                && let Poll::Ready(o) = connection
                    .next
                    .poll(cx, |cx| connection.stream.try_poll_next_unpin(cx))
            {
                match o {
                    Some(Ok(item)) => {
                        this.next.downgrade().insert(key);
                        return Poll::Ready(Some(ConnectionItem::Item(item)));
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

impl<S: Unpin + TryStream<Error = E> + Sink<T, Error = E>, T: Clone, E> FusedStream
    for Multicast<S, T, E>
{
    fn is_terminated(&self) -> bool {
        self.connections.is_empty()
    }
}

impl<S: Unpin + Sink<T, Error = E>, T: Clone, E> Sink<T> for Multicast<S, T, E> {
    type Error = Infallible;

    fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, item: T) -> Result<(), Self::Error> {
        let this = self.project();
        this.items.push(item);
        while let Some(key) = this.connections.link_pop_front::<OP_IS_NOT_BEHIND>() {
            this.connections.link_pop_at::<OP_IS_FLUSHING>(key);
            this.connections.link_push_back::<OP_IS_BEHIND>(key);
            this.ready.downgrade().insert(key);
        }
        Ok(())
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.next_flush.flush.register(cx.waker());
        if self.flush_target < self.items.len() {
            self.as_mut().start_flush();
        }
        ready!(self.poll_send_flush());
        Poll::Ready(Ok(()))
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        ready!(self.as_mut().poll_send_all(cx));
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

impl<S: Unpin + Sink<T, Error = E>, T: Clone, E> ExtendPinned<S> for Multicast<S, T, E> {
    fn extend_pinned<I: IntoIterator<Item = S>>(mut self: Pin<&mut Self>, iter: I) {
        for stream in iter {
            self.as_mut().push(stream);
        }
    }
}

pub type MulticastExtending<T, R> = Extending<Multicast<<R as MulticastReplay<T>>::S, T>, R>;

pub trait MulticastReplay<T: Clone>: Sized + FusedStream<Item = Self::S> {
    /// Single [`Stream`]/[`Sink`].
    type S: Unpin + TryStream<Error = Self::E> + Sink<T, Error = Self::E>;
    /// Error.
    type E;

    #[must_use]
    fn multicast_replay(self) -> MulticastExtending<T, Self> {
        self.extending_default()
    }
}

impl<S: Unpin + TryStream<Error = E> + Sink<T, Error = E>, T: Clone, E, R: FusedStream<Item = S>>
    MulticastReplay<T> for R
{
    type S = S;
    type E = E;
}

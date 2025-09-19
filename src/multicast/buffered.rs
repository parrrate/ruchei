use std::{
    collections::VecDeque,
    convert::Infallible,
    ops::Index,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Wake},
};

use futures_util::{
    Sink, SinkExt, Stream, TryStream, TryStreamExt, ready, stream::FusedStream, task::AtomicWaker,
};
use pin_project::pin_project;
use ruchei_collections::{
    as_linked_slab::{AsLinkedSlab, SlabKey},
    linked_slab::LinkedSlab,
};

use crate::{
    multi_item::MultiItem,
    pinned_extend::{Extending, PinnedExtend},
    ready_slab::{ConnectionWaker, Ready},
};

const OP_WAKE_NEXT: usize = 0;
const OP_WAKE_READY: usize = 1;
const OP_WAKE_FLUSH: usize = 1;
const OP_WAKE_CLOSE: usize = 2;
/// `start`ed, haven't yet reached the `flush_target`
const OP_IS_S_PRE_F: usize = 3;
/// `start`ed, already reached the `flush_target`
const OP_IS_S_POST_F: usize = 4;
/// `OP_IS_S_PRE_F` and `sent == items.len()`
const OP_IS_FLUSHING: usize = 5;
/// ordered by `sent`
const OP_SENT_COUNT: usize = 6;
/// first representative of `OP_SENT_COUNT` per `sent`
const OP_SENT_FIRST: usize = 7;
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

#[derive(Debug)]
struct Item<T> {
    item: T,
    first: Option<SlabKey>,
}

#[derive(Debug)]
struct Items<T> {
    items: VecDeque<Item<T>>,
    offset: usize,
}

impl<T> Default for Items<T> {
    fn default() -> Self {
        Self {
            items: Default::default(),
            offset: Default::default(),
        }
    }
}

impl<T> Items<T> {
    fn len(&self) -> usize {
        self.items.len() + self.offset
    }

    fn push(&mut self, item: T, first: Option<SlabKey>) {
        self.items.push_back(Item { item, first });
    }
}

impl<T> Index<usize> for Items<T> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        &self.items[index.checked_sub(self.offset).expect("early index")].item
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
    items: Items<T>,
    first_sent_all: Option<SlabKey>,
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
            first_sent_all: Default::default(),
            flush_target: Default::default(),
            next_flush: Default::default(),
            closed: Default::default(),
        }
    }
}

impl<S: Unpin + Sink<T, Error = E>, T: Clone, E> Multicast<S, T, E> {
    fn first_for(self: Pin<&mut Self>, sent: usize) -> &mut Option<SlabKey> {
        let this = self.project();
        if sent == this.items.len() {
            this.first_sent_all
        } else {
            &mut this.items.items[sent.checked_sub(this.items.offset).expect("early index")].first
        }
    }

    fn uncount_first(
        mut self: Pin<&mut Self>,
        key: SlabKey,
        sent: usize,
    ) -> (Option<SlabKey>, Option<SlabKey>) {
        let mut this = self.as_mut().project();
        assert!(this.connections.link_contains::<OP_SENT_FIRST>(key));
        assert_eq!(this.connections[key].sent, sent);
        assert_eq!(self.as_mut().first_for(sent).take(), Some(key));
        this = self.as_mut().project();
        let (long_prev, long_next) = this.connections.link_of::<OP_SENT_FIRST>(Some(key));
        assert!(this.connections.link_pop_at::<OP_SENT_FIRST>(key));
        let (_, short_next) = this.connections.link_of::<OP_SENT_COUNT>(Some(key));
        assert!(this.connections.link_pop_at::<OP_SENT_COUNT>(key));
        if let Some(short_next) = short_next
            && this.connections[short_next].sent == sent
        {
            assert!(!this.connections.link_contains::<OP_SENT_FIRST>(short_next));
            this.connections
                .link_insert::<OP_SENT_FIRST>(long_prev, short_next, long_next);
            *self.first_for(sent) = Some(short_next);
            (Some(short_next), long_next)
        } else {
            (long_prev, long_next)
        }
    }

    fn uncount_non_first(
        mut self: Pin<&mut Self>,
        key: SlabKey,
        sent: usize,
    ) -> (SlabKey, Option<SlabKey>) {
        let mut this = self.as_mut().project();
        assert!(!this.connections.link_contains::<OP_SENT_FIRST>(key));
        assert_eq!(this.connections[key].sent, sent);
        let first = self
            .as_mut()
            .first_for(sent)
            .as_ref()
            .copied()
            .expect("first not found");
        assert_ne!(first, key);
        this = self.project();
        let (_, long_next) = this.connections.link_of::<OP_SENT_FIRST>(Some(first));
        assert!(this.connections.link_pop_at::<OP_SENT_COUNT>(key));
        (first, long_next)
    }

    fn uncount(
        mut self: Pin<&mut Self>,
        key: SlabKey,
        sent: usize,
    ) -> (Option<SlabKey>, Option<SlabKey>) {
        let mut this = self.as_mut().project();
        assert_eq!(this.connections[key].sent, sent);
        let (prev, next) = if this.connections.link_contains::<OP_SENT_FIRST>(key) {
            self.as_mut().uncount_first(key, sent)
        } else {
            let (prev, next) = self.as_mut().uncount_non_first(key, sent);
            (Some(prev), next)
        };
        this = self.as_mut().project();
        if let Some(prev) = prev {
            assert!(this.connections.link_contains::<OP_SENT_FIRST>(prev));
            let prev_sent = this.connections[prev].sent;
            assert!(prev_sent <= sent);
            assert_eq!(*self.as_mut().first_for(prev_sent), Some(prev));
            this = self.as_mut().project();
        }
        if let Some(next) = next {
            assert!(this.connections.link_contains::<OP_SENT_FIRST>(next));
            let next_sent = this.connections[next].sent;
            assert!(sent < next_sent);
            assert_eq!(*self.as_mut().first_for(next_sent), Some(next));
            this = self.as_mut().project();
        }
        assert!(!this.connections.link_contains::<OP_SENT_FIRST>(key));
        assert!(!this.connections.link_contains::<OP_SENT_COUNT>(key));
        (prev, next)
    }

    fn count(
        mut self: Pin<&mut Self>,
        prev: Option<SlabKey>,
        next: Option<SlabKey>,
        key: SlabKey,
        sent: usize,
    ) {
        let mut this = self.as_mut().project();
        assert_eq!(this.connections[key].sent, sent);
        assert!(!this.connections.link_contains::<OP_SENT_FIRST>(key));
        assert!(!this.connections.link_contains::<OP_SENT_COUNT>(key));
        if let Some(prev) = prev {
            assert!(this.connections.link_contains::<OP_SENT_FIRST>(prev));
            let prev_sent = this.connections[prev].sent;
            assert!(prev_sent < sent);
            assert_eq!(*self.as_mut().first_for(prev_sent), Some(prev));
            this = self.as_mut().project();
        }
        if let Some(next) = next {
            assert!(this.connections.link_contains::<OP_SENT_FIRST>(next));
            let next_sent = this.connections[next].sent;
            assert!(sent <= next_sent);
            assert_eq!(*self.as_mut().first_for(next_sent), Some(next));
            this = self.as_mut().project();
        }
        let (_, prev_next) = this.connections.link_of::<OP_SENT_FIRST>(prev);
        assert_eq!(prev_next, next);
        let (next_prev, long_next) = this.connections.link_of::<OP_SENT_FIRST>(next);
        assert_eq!(next_prev, prev);
        if let Some(next) = next
            && sent == this.connections[next].sent
        {
            let (short_prev, _) = this.connections.link_of::<OP_SENT_COUNT>(long_next);
            let short_prev = short_prev.expect("should at least be what next is");
            assert_eq!(this.connections[short_prev].sent, sent);
            this.connections
                .link_insert::<OP_SENT_COUNT>(Some(short_prev), key, long_next);
        } else {
            this.connections
                .link_insert::<OP_SENT_FIRST>(prev, key, next);
            let (short_prev, _) = this.connections.link_of::<OP_SENT_COUNT>(next);
            match (prev, short_prev) {
                (None, None) => {}
                (Some(prev), Some(short_prev)) => {
                    let sent = this.connections[short_prev].sent;
                    assert_eq!(this.connections[prev].sent, sent);
                    assert_eq!(*self.as_mut().first_for(sent), Some(prev));
                    this = self.as_mut().project();
                }
                _ => panic!("inconsistent state"),
            }
            this.connections
                .link_insert::<OP_SENT_COUNT>(short_prev, key, next);
            *self.first_for(sent) = Some(key);
        }
    }

    fn increment_sent(mut self: Pin<&mut Self>, key: SlabKey, sent: usize) {
        let (prev, next) = self.as_mut().uncount(key, sent);
        let this = self.as_mut().project();
        this.connections[key].sent += 1;
        let sent = this.connections[key].sent;
        self.count(prev, next, key, sent);
    }

    fn remove(mut self: Pin<&mut Self>, key: SlabKey, error: Option<E>) {
        let mut this = self.as_mut().project();
        if this.connections.link_contains::<OP_SENT_FIRST>(key) {
            let sent = this.connections[key].sent;
            self.as_mut().uncount_first(key, sent);
            this = self.project();
        }
        let connection = this.connections.remove(key);
        connection.next.waker.wake();
        connection.ready.waker.wake();
        connection.flush.waker.wake();
        connection.close.waker.wake();
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
        let sent = this.items.len();
        let connection = Connection {
            stream,
            next: ConnectionWaker::new(key, next),
            ready: ConnectionWaker::new(key, ready),
            flush: ConnectionWaker::new(key, flush),
            close: ConnectionWaker::new(key, close),
            sent,
            flushed: sent,
        };
        assert_eq!(this.connections.insert(connection), key);
        assert!(this.connections.link_push_back::<OP_WAKE_NEXT>(key));
        assert!(this.connections.link_push_back::<OP_WAKE_READY>(key));
        assert!(this.connections.link_push_back::<OP_WAKE_CLOSE>(key));
        this.next.wake();
        this.ready.wake();
        this.close.wake();
        assert!(this.connections.link_push_back::<OP_SENT_COUNT>(key));
        if this.first_sent_all.is_none() {
            assert!(this.connections.link_push_back::<OP_SENT_FIRST>(key));
            *this.first_sent_all = Some(key);
        }
    }

    fn start_flush_one(self: Pin<&mut Self>, key: SlabKey) {
        let this = self.project();
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
        let mut this = self.as_mut().project();
        assert!(this.connections[key].sent < this.items.len());
        while this.connections[key].sent < this.items.len() {
            ready!(this.connections[key].stream.poll_ready_unpin(cx))?;
            let sent = this.connections[key].sent;
            let item = this.items[sent].clone();
            this.connections[key].stream.start_send_unpin(item)?;
            if !this.connections.link_contains::<OP_IS_S_PRE_F>(key) {
                if this.connections[key].flushed < *this.flush_target {
                    this.connections.link_push_back::<OP_IS_S_PRE_F>(key);
                } else {
                    this.connections.link_push_back::<OP_IS_S_POST_F>(key);
                }
            }
            self.as_mut().increment_sent(key, sent);
            this = self.as_mut().project();
        }
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
        assert!(this.connections[key].sent == this.items.len());
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
            if this.connections[key].sent < this.items.len()
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
        while let Some(item) = this.items.items.front()
            && item.first.is_none()
        {
            this.items.items.pop_front();
            this.items.offset += 1;
        }
        if this.items.items.is_empty() {
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
            if this.connections[key].sent == this.items.len() {
                self.as_mut().start_flush_one(key);
                this = self.as_mut().project();
            }
        }
    }
}

impl<S: Unpin + TryStream<Error = E> + Sink<T, Error = E>, T: Clone, E> Stream
    for Multicast<S, T, E>
{
    type Item = MultiItem<S>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.next_flush.next.register(cx.waker());
        let _ = self.as_mut().poll_send_flush();
        let mut this = self.as_mut().project();
        if let Some((stream, error)) = this.closed.pop_front() {
            return Poll::Ready(Some(MultiItem::Closed(stream, error)));
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
                        return Poll::Ready(Some(MultiItem::Item(item)));
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
        self.closed.is_empty() && self.connections.is_empty()
    }
}

impl<S: Unpin + Sink<T, Error = E>, T: Clone, E> Sink<T> for Multicast<S, T, E> {
    type Error = Infallible;

    fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, item: T) -> Result<(), Self::Error> {
        let this = self.project();
        let mut key = this.first_sent_all.as_ref().copied();
        while let Some(k) = key {
            this.connections.link_pop_at::<OP_IS_FLUSHING>(k);
            this.ready.downgrade().insert(k);
            (_, key) = this.connections.link_of::<OP_SENT_COUNT>(key);
        }
        this.items.push(item, this.first_sent_all.take());
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

impl<S: Unpin + Sink<T, Error = E>, T: Clone, E> PinnedExtend<S> for Multicast<S, T, E> {
    fn extend_pinned<I: IntoIterator<Item = S>>(mut self: Pin<&mut Self>, iter: I) {
        for stream in iter {
            self.as_mut().push(stream);
        }
    }
}

pub type MulticastExtending<T, R> = Extending<Multicast<<R as MulticastBuffered<T>>::S, T>, R>;

pub trait MulticastBuffered<T: Clone>: Sized + FusedStream<Item = Self::S> {
    /// Single [`Stream`]/[`Sink`].
    type S: Unpin + TryStream<Error = Self::E> + Sink<T, Error = Self::E>;
    /// Error.
    type E;

    #[must_use]
    fn multicast_buffered(self) -> MulticastExtending<T, Self> {
        Extending::new(self, Default::default())
    }
}

impl<S: Unpin + TryStream<Error = E> + Sink<T, Error = E>, T: Clone, E, R: FusedStream<Item = S>>
    MulticastBuffered<T> for R
{
    type S = S;
    type E = E;
}

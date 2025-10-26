use std::{
    collections::VecDeque,
    convert::Infallible,
    ops::Index,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Wake},
};

use extend_pinned::ExtendPinned;
use futures_util::{Sink, Stream, TryStream, ready, stream::FusedStream, task::AtomicWaker};
use pin_project::pin_project;
use ruchei_extend::{Extending, ExtendingExt};
use ruchei_wakelist::{Queue, Ref};

use crate::connection_item::ConnectionItem;

const OP_WAKE_NEXT: usize = 0;
const OP_WAKE_READY: usize = 1;
const OP_WAKE_FLUSH: usize = 2;
const OP_WAKE_CLOSE: usize = 3;
const OP_WAKE: usize = 4;
/// `start`ed, haven't yet reached the `flush_target`
const OP_IS_S_PRE_F: usize = 4;
/// `start`ed, already reached the `flush_target`
const OP_IS_S_POST_F: usize = 5;
/// `OP_IS_S_PRE_F` and `sent == items.len()`
const OP_IS_FLUSHING: usize = 6;
/// ordered by `sent`
const OP_SENT_COUNT: usize = 7;
/// first representative of `OP_SENT_COUNT` per `sent`
const OP_SENT_FIRST: usize = 8;
const OP_COUNT: usize = 9;

type Key<S> = Ref<Connection<S>, OP_WAKE, OP_COUNT>;

#[pin_project]
struct Connection<S> {
    #[pin]
    stream: S,
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
struct Item<S, T> {
    item: T,
    first: Option<Key<S>>,
}

#[derive(Debug)]
struct Items<S, T> {
    items: VecDeque<Item<S, T>>,
    offset: usize,
}

impl<S, T> Default for Items<S, T> {
    fn default() -> Self {
        Self {
            items: Default::default(),
            offset: Default::default(),
        }
    }
}

impl<S, T> Items<S, T> {
    #[must_use]
    fn len(&self) -> usize {
        self.items.len() + self.offset
    }

    fn push(&mut self, item: T, first: Option<Key<S>>) {
        self.items.push_back(Item { item, first });
    }
}

impl<S, T> Index<usize> for Items<S, T> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        &self.items[index.checked_sub(self.offset).expect("early index")].item
    }
}

pub struct Multicast<S, T, E = <S as TryStream>::Error> {
    connections: Queue<Connection<S>, OP_WAKE, OP_COUNT>,
    items: Items<S, T>,
    first_sent_all: Option<Key<S>>,
    flush_target: usize,
    next_flush: Arc<NextFlush>,
    closed: VecDeque<(S, Option<E>)>,
}

impl<S, T, E> Unpin for Multicast<S, T, E> {}

impl<S, T, E> Default for Multicast<S, T, E> {
    fn default() -> Self {
        Self {
            connections: Default::default(),
            items: Default::default(),
            first_sent_all: Default::default(),
            flush_target: Default::default(),
            next_flush: Default::default(),
            closed: Default::default(),
        }
    }
}

impl<S: Sink<T, Error = E>, T: Clone, E> Multicast<S, T, E> {
    #[must_use]
    fn first_for(&mut self, sent: usize) -> &mut Option<Key<S>> {
        if sent == self.items.len() {
            &mut self.first_sent_all
        } else {
            &mut self.items.items[sent.checked_sub(self.items.offset).expect("early index")].first
        }
    }

    #[must_use]
    fn uncount_first(&mut self, key: &Key<S>, sent: usize) -> (Option<Key<S>>, Option<Key<S>>) {
        assert!(self.connections.link_contains::<OP_SENT_FIRST>(key));
        assert_eq!(self.connections[key].sent, sent);
        assert_eq!(self.first_for(sent).take().as_ref(), Some(key));
        let (long_prev, long_next) = self.connections.link_of::<OP_SENT_FIRST>(Some(key));
        assert!(self.connections.link_pop_at::<OP_SENT_FIRST>(key));
        let (_, short_next) = self.connections.link_of::<OP_SENT_COUNT>(Some(key));
        assert!(self.connections.link_pop_at::<OP_SENT_COUNT>(key));
        if let Some(short_next) = short_next
            && self.connections[&short_next].sent == sent
        {
            assert!(!self.connections.link_contains::<OP_SENT_FIRST>(&short_next));
            self.connections.link_insert::<OP_SENT_FIRST>(
                long_prev.as_ref(),
                &short_next,
                long_next.as_ref(),
            );
            *self.first_for(sent) = Some(short_next.clone());
            (Some(short_next), long_next)
        } else {
            (long_prev, long_next)
        }
    }

    #[must_use]
    fn uncount_non_first(&mut self, key: &Key<S>, sent: usize) -> (Key<S>, Option<Key<S>>) {
        assert!(!self.connections.link_contains::<OP_SENT_FIRST>(key));
        assert_eq!(self.connections[key].sent, sent);
        let first = self
            .first_for(sent)
            .as_ref()
            .cloned()
            .expect("first not found");
        assert_ne!(first, *key);
        let (_, long_next) = self.connections.link_of::<OP_SENT_FIRST>(Some(&first));
        assert!(self.connections.link_pop_at::<OP_SENT_COUNT>(key));
        (first, long_next)
    }

    #[must_use]
    fn uncount(&mut self, key: &Key<S>, sent: usize) -> (Option<Key<S>>, Option<Key<S>>) {
        assert_eq!(self.connections[key].sent, sent);
        let (prev, next) = if self.connections.link_contains::<OP_SENT_FIRST>(key) {
            self.uncount_first(key, sent)
        } else {
            let (prev, next) = self.uncount_non_first(key, sent);
            (Some(prev), next)
        };
        if let Some(prev) = prev.as_ref() {
            assert!(self.connections.link_contains::<OP_SENT_FIRST>(prev));
            let prev_sent = self.connections[prev].sent;
            assert!(prev_sent <= sent);
            assert_eq!(self.first_for(prev_sent).as_ref(), Some(prev));
        }
        if let Some(next) = next.as_ref() {
            assert!(self.connections.link_contains::<OP_SENT_FIRST>(next));
            let next_sent = self.connections[next].sent;
            assert!(sent < next_sent);
            assert_eq!(self.first_for(next_sent).as_ref(), Some(next));
        }
        assert!(!self.connections.link_contains::<OP_SENT_FIRST>(key));
        assert!(!self.connections.link_contains::<OP_SENT_COUNT>(key));
        (prev, next)
    }

    fn count(&mut self, prev: Option<&Key<S>>, next: Option<&Key<S>>, key: &Key<S>, sent: usize) {
        assert_eq!(self.connections[key].sent, sent);
        assert!(!self.connections.link_contains::<OP_SENT_FIRST>(key));
        assert!(!self.connections.link_contains::<OP_SENT_COUNT>(key));
        if let Some(prev) = prev {
            assert!(self.connections.link_contains::<OP_SENT_FIRST>(prev));
            let prev_sent = self.connections[prev].sent;
            assert!(prev_sent < sent);
            assert_eq!(self.first_for(prev_sent).as_ref(), Some(prev));
        }
        if let Some(next) = next {
            assert!(self.connections.link_contains::<OP_SENT_FIRST>(next));
            let next_sent = self.connections[next].sent;
            assert!(sent <= next_sent);
            assert_eq!(self.first_for(next_sent).as_ref(), Some(next));
        }
        let (_, prev_next) = self.connections.link_of::<OP_SENT_FIRST>(prev);
        assert_eq!(prev_next.as_ref(), next);
        let (next_prev, long_next) = self.connections.link_of::<OP_SENT_FIRST>(next);
        assert_eq!(next_prev.as_ref(), prev);
        if let Some(next) = next
            && sent == self.connections[next].sent
        {
            let (short_prev, _) = self
                .connections
                .link_of::<OP_SENT_COUNT>(long_next.as_ref());
            let short_prev = short_prev.expect("should at least be what next is");
            assert_eq!(self.connections[&short_prev].sent, sent);
            self.connections.link_insert::<OP_SENT_COUNT>(
                Some(&short_prev),
                key,
                long_next.as_ref(),
            );
        } else {
            self.connections
                .link_insert::<OP_SENT_FIRST>(prev, key, next);
            let (short_prev, _) = self.connections.link_of::<OP_SENT_COUNT>(next);
            match (prev, &short_prev) {
                (None, None) => {}
                (Some(prev), Some(short_prev)) => {
                    let sent = self.connections[short_prev].sent;
                    assert_eq!(self.connections[prev].sent, sent);
                    assert_eq!(self.first_for(sent).as_ref(), Some(prev));
                }
                _ => panic!("inconsistent state"),
            }
            self.connections
                .link_insert::<OP_SENT_COUNT>(short_prev.as_ref(), key, next);
            *self.first_for(sent) = Some(key.clone());
        }
    }

    fn increment_sent(&mut self, key: &Key<S>, sent: usize) {
        let (prev, next) = self.uncount(key, sent);
        *self.connections.index_pin_mut(key).project().sent += 1;
        let sent = self.connections[key].sent;
        self.count(prev.as_ref(), next.as_ref(), key, sent);
    }

    fn remove(&mut self, key: &Key<S>, error: Option<E>) {
        if self.connections.link_contains::<OP_SENT_FIRST>(key) {
            let sent = self.connections[key].sent;
            let _ = self.uncount_first(key, sent);
        }
        let connection = self.connections.remove(key).expect("unknown connection");
        self.closed.push_back((connection.stream, error));
        self.connections.wake::<OP_WAKE_NEXT>();
        self.connections.wake::<OP_WAKE_READY>();
        self.connections.wake::<OP_WAKE_FLUSH>();
        self.connections.wake::<OP_WAKE_CLOSE>();
    }

    pub fn push(&mut self, stream: S) {
        let sent = self.items.len();
        let key = self.connections.insert(Connection {
            stream,
            sent,
            flushed: sent,
        });
        self.connections.wake_push::<OP_WAKE_NEXT>(&key);
        self.connections.wake_push::<OP_WAKE_READY>(&key);
        self.connections.wake_push::<OP_WAKE_CLOSE>(&key);
        assert!(self.connections.link_push_back::<OP_SENT_COUNT>(&key));
        if self.first_sent_all.is_none() {
            assert!(self.connections.link_push_back::<OP_SENT_FIRST>(&key));
            self.first_sent_all = Some(key);
        }
    }

    fn start_flush_one(&mut self, key: &Key<S>) {
        assert!(self.connections[key].sent == self.items.len());
        assert!(self.connections.link_contains::<OP_IS_S_PRE_F>(key));
        assert!(self.connections[key].flushed < self.flush_target);
        assert!(self.connections.link_push_back::<OP_IS_FLUSHING>(key));
        self.connections.wake_push::<OP_WAKE_FLUSH>(key);
    }

    /// wait until `sent` reaches `items.len()`
    fn poll_send_one(&mut self, key: &Key<S>, cx: &mut Context<'_>) -> Poll<Result<(), S::Error>> {
        assert!(self.connections[key].sent < self.items.len());
        while self.connections[key].sent < self.items.len() {
            ready!(
                self.connections
                    .index_pin_mut(key)
                    .project()
                    .stream
                    .poll_ready(cx),
            )?;
            let sent = self.connections[key].sent;
            let item = self.items[sent].clone();
            self.connections
                .index_pin_mut(key)
                .project()
                .stream
                .start_send(item)?;
            if !self.connections.link_contains::<OP_IS_S_PRE_F>(key) {
                if self.connections[key].flushed < self.flush_target {
                    self.connections.link_push_back::<OP_IS_S_PRE_F>(key);
                } else {
                    self.connections.link_push_back::<OP_IS_S_POST_F>(key);
                }
            }
            self.increment_sent(key, sent);
        }
        if self.connections.link_contains::<OP_IS_S_PRE_F>(key) {
            self.start_flush_one(key);
        }
        Poll::Ready(Ok(()))
    }

    /// wait until `flushed` reaches `flush_target`
    fn poll_flush_one(&mut self, key: &Key<S>, cx: &mut Context<'_>) -> Poll<Result<(), S::Error>> {
        assert!(self.connections.link_contains::<OP_IS_FLUSHING>(key));
        assert!(self.connections[key].sent == self.items.len());
        assert!(self.connections.link_contains::<OP_IS_S_PRE_F>(key));
        ready!(
            self.connections
                .index_pin_mut(key)
                .project()
                .stream
                .poll_flush(cx),
        )?;
        *self.connections.index_pin_mut(key).project().flushed = self.connections[key].sent;
        assert!(self.connections.link_pop_at::<OP_IS_FLUSHING>(key));
        assert!(self.connections.link_pop_at::<OP_IS_S_PRE_F>(key));
        Poll::Ready(Ok(()))
    }

    fn poll_send_all(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        self.connections.queue_poll::<OP_WAKE_READY>(cx);
        while let Some(key) = self.connections.link_pop_front::<OP_WAKE_READY>() {
            if self.connections[&key].sent < self.items.len()
                && let waker = key.waker::<OP_WAKE_READY>()
                && let Poll::Ready(Err(e)) =
                    self.poll_send_one(&key, &mut Context::from_waker(&waker))
            {
                self.remove(&key, Some(e));
            }
        }
        while let Some(item) = self.items.items.front()
            && item.first.is_none()
        {
            self.items.items.pop_front();
            self.items.offset += 1;
        }
        if self.items.items.is_empty() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }

    fn poll_flush_all(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        self.connections.queue_poll::<OP_WAKE_FLUSH>(cx);
        while let Some(key) = self.connections.link_pop_front::<OP_WAKE_FLUSH>() {
            if self.connections.link_contains::<OP_IS_FLUSHING>(&key)
                && let waker = key.waker::<OP_WAKE_FLUSH>()
                && let Poll::Ready(Err(e)) =
                    self.poll_flush_one(&key, &mut Context::from_waker(&waker))
            {
                self.remove(&key, Some(e));
            }
        }
        if self.connections.link_empty::<OP_IS_FLUSHING>() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }

    fn poll_send_flush(&mut self) -> Poll<()> {
        let waker = self.next_flush.clone().into();
        let cx = &mut Context::from_waker(&waker);
        let sent = self.poll_send_all(cx);
        ready!(self.poll_flush_all(cx));
        sent
    }

    fn start_flush(&mut self) {
        assert!(self.flush_target < self.items.len());
        self.flush_target = self.items.len();
        while let Some(key) = self.connections.link_pop_front::<OP_IS_S_POST_F>() {
            assert!(self.connections[&key].flushed < self.flush_target);
            self.connections.link_push_back::<OP_IS_S_PRE_F>(&key);
            if self.connections[&key].sent == self.items.len() {
                self.start_flush_one(&key);
            }
        }
    }
}

impl<S: TryStream<Error = E> + Sink<T, Error = E>, T: Clone, E> Stream for Multicast<S, T, E> {
    type Item = ConnectionItem<S>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        this.next_flush.next.register(cx.waker());
        let _ = this.poll_send_flush();
        if let Some((stream, error)) = this.closed.pop_front() {
            return Poll::Ready(Some(ConnectionItem::Closed(stream, error)));
        }
        this.connections.queue_poll::<OP_WAKE_NEXT>(cx);
        while let Some(key) = this.connections.link_pop_front::<OP_WAKE_NEXT>() {
            let (connection, waker) = this.connections.context::<OP_WAKE_NEXT>(&key);
            if let Poll::Ready(o) = connection
                .project()
                .stream
                .try_poll_next(&mut Context::from_waker(&waker))
            {
                match o {
                    Some(Ok(item)) => {
                        this.connections.wake_push::<OP_WAKE_NEXT>(&key);
                        return Poll::Ready(Some(ConnectionItem::Item(item)));
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

impl<S: TryStream<Error = E> + Sink<T, Error = E>, T: Clone, E> FusedStream for Multicast<S, T, E> {
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
        let this = self.get_mut();
        let mut key = this.first_sent_all.as_ref().cloned();
        while let Some(k) = key.as_ref() {
            this.connections.link_pop_at::<OP_IS_FLUSHING>(k);
            this.connections.wake_push::<OP_WAKE_READY>(k);
            (_, key) = this.connections.link_of::<OP_SENT_COUNT>(key.as_ref());
        }
        this.items.push(item, this.first_sent_all.take());
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        this.next_flush.flush.register(cx.waker());
        if this.flush_target < this.items.len() {
            this.start_flush();
        }
        ready!(this.poll_send_flush());
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        ready!(this.poll_send_all(cx));
        this.connections.queue_poll::<OP_WAKE_CLOSE>(cx);
        while let Some(key) = this.connections.link_pop_front::<OP_WAKE_CLOSE>() {
            let (connection, waker) = this.connections.context::<OP_WAKE_CLOSE>(&key);
            if let Poll::Ready(r) = connection
                .project()
                .stream
                .poll_close(&mut Context::from_waker(&waker))
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

impl<S: Sink<T, Error = E>, T: Clone, E> Extend<S> for Multicast<S, T, E> {
    fn extend<I: IntoIterator<Item = S>>(&mut self, iter: I) {
        for stream in iter {
            self.push(stream);
        }
    }
}

impl<S: Sink<T, Error = E>, T: Clone, E> ExtendPinned<S> for Multicast<S, T, E> {
    fn extend_pinned<I: IntoIterator<Item = S>>(self: Pin<&mut Self>, iter: I) {
        self.get_mut().extend(iter)
    }
}

pub type MulticastExtending<T, R> = Extending<Multicast<<R as MulticastBufferedWl<T>>::S, T>, R>;

pub trait MulticastBufferedWl<T: Clone>: Sized + Stream<Item = Self::S> {
    /// Single [`Stream`]/[`Sink`].
    type S: TryStream<Error = Self::E> + Sink<T, Error = Self::E>;
    /// Error.
    type E;

    #[must_use]
    fn multicast_buffered_wakelist(self) -> MulticastExtending<T, Self> {
        self.extending_default()
    }
}

impl<S: Unpin + TryStream<Error = E> + Sink<T, Error = E>, T: Clone, E, R: Stream<Item = S>>
    MulticastBufferedWl<T> for R
{
    type S = S;
    type E = E;
}

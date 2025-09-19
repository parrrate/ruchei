use std::{
    collections::VecDeque,
    convert::Infallible,
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
    ready_slab::{Connection, ConnectionWaker, Ready},
};

const OP_WAKE_NEXT: usize = 0;
const OP_DEAL: usize = 1;
const OP_WAKE_FLUSH: usize = 2;
const OP_WAKE_CLOSE: usize = 3;
const OP_IS_STARTED: usize = 4;
const OP_IS_FLUSHING: usize = 5;
const OP_COUNT: usize = 6;

#[derive(Debug, Default)]
struct Wakers {
    next: AtomicWaker,
    ready: AtomicWaker,
    flush: AtomicWaker,
}

impl Wake for Wakers {
    fn wake(self: Arc<Self>) {
        self.next.wake();
        self.ready.wake();
        self.flush.wake();
    }
}

/// [`Sink`]/[`Stream`] implemented over the stream of incoming [`Sink`]s/[`Stream`]s.
#[pin_project]
#[derive(Debug)]
pub struct Dealer<S, Out, E = <S as TryStream>::Error> {
    connections: LinkedSlab<Connection<S>, OP_COUNT>,
    #[pin]
    next: Ready,
    wready: AtomicWaker,
    #[pin]
    flush: Ready,
    #[pin]
    close: Ready,
    wakers: Arc<Wakers>,
    buffer: Option<Out>,
    closed: VecDeque<(S, Option<E>)>,
}

impl<S, Out, E> Default for Dealer<S, Out, E> {
    fn default() -> Self {
        Self {
            connections: Default::default(),
            next: Default::default(),
            wready: Default::default(),
            flush: Default::default(),
            close: Default::default(),
            wakers: Default::default(),
            buffer: Default::default(),
            closed: Default::default(),
        }
    }
}

impl<S, Out, E> Dealer<S, Out, E> {
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

    /// Add new connection with its unique key.
    pub fn push(self: Pin<&mut Self>, stream: S) {
        let this = self.project();
        let key = this.connections.vacant_key();
        let next = this.next.downgrade();
        let flush = this.flush.downgrade();
        let close = this.close.downgrade();
        this.wready.wake();
        let connection = Connection {
            stream,
            next: ConnectionWaker::new(key, next),
            ready: ConnectionWaker::new(key, Default::default()),
            flush: ConnectionWaker::new(key, flush),
            close: ConnectionWaker::new(key, close),
        };
        assert_eq!(this.connections.insert(connection), key);
        this.connections.link_push_back::<OP_WAKE_NEXT>(key);
        this.connections.link_push_back::<OP_DEAL>(key);
        this.connections.link_push_back::<OP_WAKE_CLOSE>(key);
        this.next.wake();
        this.close.wake();
    }
}

impl<E, Out, S: Unpin + Sink<Out, Error = E>> Dealer<S, Out, E> {
    fn poll_ready_first(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.as_mut().project();
        this.wready.register(cx.waker());
        if let Some(key) = this.connections.front::<OP_DEAL>()
            && let Some(connection) = this.connections.get_mut(key)
        {
            if let Err(e) = ready!(
                connection
                    .ready
                    .poll(cx, |cx| connection.stream.poll_ready_unpin(cx))
            ) {
                self.remove(key, Some(e));
            } else {
                return Poll::Ready(());
            }
        }
        Poll::Pending
    }

    fn start_send_first(mut self: Pin<&mut Self>, msg: Out) {
        let this = self.as_mut().project();
        if let Some(key) = this.connections.front::<OP_DEAL>() {
            let connection = this
                .connections
                .get_refresh::<OP_DEAL>(key)
                .expect("first key must point to an existing entry");
            if let Err(e) = connection.stream.start_send_unpin(msg) {
                self.remove(key, Some(e));
            } else {
                this.connections.link_push_back::<OP_IS_STARTED>(key);
                if this.connections.link_contains::<OP_IS_FLUSHING>(key) {
                    this.connections.link_pop_at::<OP_IS_FLUSHING>(key);
                    this.flush.wake();
                }
            }
        };
    }

    fn poll(mut self: Pin<&mut Self>) -> Poll<()> {
        let mut this = self.as_mut().project();
        let waker = this.wakers.clone().into();
        let cx = &mut Context::from_waker(&waker);
        if this.buffer.is_some() {
            ready!(self.as_mut().poll_ready_first(cx));
            this = self.as_mut().project();
        }
        if let Some(msg) = this.buffer.take() {
            self.as_mut().start_send_first(msg);
            self.wakers.wake_by_ref();
        }
        Poll::Ready(())
    }
}

impl<In, Out, E, S: Unpin + TryStream<Ok = In, Error = E> + Sink<Out, Error = E>> Stream
    for Dealer<S, Out, E>
{
    type Item = MultiItem<S>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.wakers.next.register(cx.waker());
        let _ = self.as_mut().poll();
        let mut this = self.as_mut().project();
        if let Some((stream, error)) = this.closed.pop_front() {
            return Poll::Ready(Some(MultiItem::Closed(stream, error)));
        }
        this.next.register(cx);
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

impl<In, Out, E, S: Unpin + TryStream<Ok = In, Error = E> + Sink<Out, Error = E>> FusedStream
    for Dealer<S, Out, E>
{
    fn is_terminated(&self) -> bool {
        self.closed.is_empty() && self.connections.is_empty()
    }
}

impl<Out, E, S: Unpin + Sink<Out, Error = E>> Sink<Out> for Dealer<S, Out, E> {
    type Error = Infallible;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.wakers.ready.register(cx.waker());
        ready!(self.as_mut().poll());
        self.poll_ready_first(cx).map(Ok)
    }

    fn start_send(self: Pin<&mut Self>, msg: Out) -> Result<(), Self::Error> {
        assert!(self.buffer.is_none());
        *self.project().buffer = Some(msg);
        Ok(())
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.wakers.flush.register(cx.waker());
        ready!(self.as_mut().poll());
        let mut this = self.as_mut().project();
        this.flush.register(cx);
        this.flush.downgrade().extend(
            this.connections
                .link_pops::<OP_IS_STARTED, _, _>(|key, connection| {
                    (!connection.link_contains::<OP_IS_FLUSHING>(key)).then_some(key)
                })
                .flatten(),
        );
        while let Some(key) = this.flush.as_mut().next::<OP_WAKE_FLUSH>(this.connections) {
            if let Some(connection) = this.connections.get_mut(key) {
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

impl<S, Out, E> PinnedExtend<S> for Dealer<S, Out, E> {
    fn extend_pinned<T: IntoIterator<Item = S>>(mut self: Pin<&mut Self>, iter: T) {
        for stream in iter {
            self.as_mut().push(stream)
        }
    }
}

/// [`Sink`]/[`Stream`] Returned by [`DealWithoutMulticast::deal_without_multicast`].
pub type DealerExtending<R, Out> = Extending<Dealer<<R as Stream>::Item, Out>, R>;

/// Extension trait to auto-extend a [`Dealer`] from a stream of connections.
pub trait DealWithoutMulticast<Out>:
    Sized + FusedStream<Item: TryStream<Error = Self::E> + Sink<Out, Error = Self::E>>
{
    type E;

    /// Extend the stream of connections (`self`) into a [`Dealer`].
    #[must_use]
    fn deal_without_multicast(self) -> DealerExtending<Self, Out> {
        Extending::new(self, Default::default())
    }
}

impl<
    In,
    Out,
    E,
    S: Unpin + TryStream<Ok = In, Error = E> + Sink<Out, Error = E>,
    R: FusedStream<Item = S>,
> DealWithoutMulticast<Out> for R
{
    type E = E;
}

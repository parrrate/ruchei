use std::{
    collections::VecDeque,
    convert::Infallible,
    pin::Pin,
    task::{Context, Poll, Wake},
};

use futures_util::{Sink, SinkExt, Stream, TryStream, TryStreamExt, ready, stream::FusedStream};
use pin_project::pin_project;
use route_sink::{FlushRoute, ReadyRoute};
use ruchei_collections::{as_linked_slab::AsLinkedSlab, linked_slab::LinkedSlab};

use crate::{
    multi_item::MultiItem,
    pinned_extend::{Extending, PinnedExtend},
    ready_slab::{Connection, ConnectionWaker, Ready},
};

const OP_WAKE_NEXT: usize = 0;
const OP_WAKE_READY: usize = 1;
const OP_WAKE_FLUSH: usize = 2;
const OP_WAKE_CLOSE: usize = 3;
const OP_IS_STARTED: usize = 4;
const OP_IS_READIED: usize = 5;
const OP_IS_FLUSHING: usize = 6;
const OP_COUNT: usize = 7;

#[pin_project]
pub struct Router<S, E> {
    connections: LinkedSlab<Connection<S>, OP_COUNT>,
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

impl<S, E> Default for Router<S, E> {
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

impl<S, E> Router<S, E> {
    fn remove(self: Pin<&mut Self>, key: usize, error: Option<E>) {
        let this = self.project();
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

    pub fn poll_ready<Out>(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()>
    where
        S: Unpin + Sink<Out, Error = E>,
    {
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
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }

    pub fn poll_flush<Out>(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()>
    where
        S: Unpin + Sink<Out, Error = E>,
    {
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
                this = self.as_mut().project();
            }
        }
        if this.connections.link_empty::<OP_IS_FLUSHING>() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }

    pub fn poll_close<Out>(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()>
    where
        S: Unpin + Sink<Out, Error = E>,
    {
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
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

impl<In, E, S: Unpin + TryStream<Ok = In, Error = E>> Stream for Router<S, E> {
    type Item = MultiItem<(usize, In), S, E>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.as_mut().project();
        if let Some((stream, error)) = this.closed.pop_front() {
            return Poll::Ready(Some(MultiItem::Closed(stream, error)));
        }
        this.next.register(cx);
        while let Some(key) = this.next.as_mut().next::<OP_WAKE_NEXT>(this.connections) {
            if !this.connections.contains(key) {
                continue;
            }
            if this.connections.link_pop_at::<OP_IS_READIED>(key)
                && let Some(connection) = this.connections.get_mut(key)
            {
                connection.ready.wake_by_ref();
            }
            if this.connections.link_pop_at::<OP_IS_FLUSHING>(key)
                && let Some(connection) = this.connections.get_mut(key)
            {
                connection.flush.wake_by_ref();
            }
            if let Some(connection) = this.connections.get_mut(key)
                && let Poll::Ready(o) = connection
                    .next
                    .poll(cx, |cx| connection.stream.try_poll_next_unpin(cx))
            {
                match o {
                    Some(Ok(item)) => {
                        this.next.downgrade().insert(key);
                        return Poll::Ready(Some(MultiItem::Item((key, item))));
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

impl<In, E, S: Unpin + TryStream<Ok = In, Error = E>> FusedStream for Router<S, E> {
    fn is_terminated(&self) -> bool {
        self.connections.is_empty()
    }
}

impl<Out: Clone, E, S: Unpin + Sink<Out, Error = E>> Sink<(usize, Out)> for Router<S, E> {
    type Error = Infallible;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.poll_ready(cx).map(Ok)
    }

    fn start_send(mut self: Pin<&mut Self>, (key, msg): (usize, Out)) -> Result<(), Self::Error> {
        let this = self.as_mut().project();
        if this.connections.contains(key)
            && this.connections.link_pop_at::<OP_IS_READIED>(key)
            && let Some(connection) = this.connections.get_mut(key)
        {
            if let Err(e) = connection.stream.start_send_unpin(msg.clone()) {
                self.remove(key, Some(e));
            } else {
                this.connections.link_push_back::<OP_IS_STARTED>(key);
                this.ready.downgrade().insert(key);
            }
        }
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.poll_flush(cx).map(Ok)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.poll_close(cx).map(Ok)
    }
}

impl<Out: Clone, E, S: Unpin + Sink<Out, Error = E>> FlushRoute<usize, Out> for Router<S, E> {
    fn poll_flush_route(
        mut self: Pin<&mut Self>,
        &key: &usize,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        let mut this = self.as_mut().project();
        this.flush.downgrade().extend(
            this.connections
                .link_pops::<OP_IS_STARTED, _, _>(|key, _| key),
        );
        this.flush.compact::<OP_WAKE_FLUSH>(this.connections);
        if this.connections.contains(key)
            && this.connections.link_pop_at::<OP_WAKE_FLUSH>(key)
            && let Some(connection) = this.connections.get_mut(key)
        {
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
            this = self.as_mut().project();
        }
        if this.connections.link_contains::<OP_IS_FLUSHING>(key) {
            Poll::Pending
        } else {
            Poll::Ready(Ok(()))
        }
    }
}

impl<Out: Clone, E, S: Unpin + Sink<Out, Error = E>> ReadyRoute<usize, Out> for Router<S, E> {
    fn poll_ready_route(
        mut self: Pin<&mut Self>,
        &key: &usize,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        let this = self.as_mut().project();
        this.ready.compact::<OP_WAKE_READY>(this.connections);
        if let Some(connection) = this.connections.get_mut(key) {
            if let Err(e) = ready!(
                connection
                    .ready
                    .poll(cx, |cx| connection.stream.poll_ready_unpin(cx))
            ) {
                self.remove(key, Some(e));
            } else {
                this.connections.link_push_back::<OP_IS_READIED>(key);
            }
        }
        Poll::Ready(Ok(()))
    }
}

impl<S, E> PinnedExtend<S> for Router<S, E> {
    fn extend_pinned<T: IntoIterator<Item = S>>(mut self: Pin<&mut Self>, iter: T) {
        for stream in iter {
            self.as_mut().push(stream);
        }
    }
}

pub type RouterExtending<R> =
    Extending<Router<<R as RouteSlabMulticastExt>::S, <R as RouteSlabMulticastExt>::E>, R>;

pub trait RouteSlabMulticastExt: Sized + FusedStream<Item = Self::S> {
    /// Single [`Stream`]/[`Sink`].
    type S: Unpin + TryStream<Ok = Self::In, Error = Self::E>;
    type In;
    /// Error.
    type E;

    #[must_use]
    fn route_slab_multicast(self) -> RouterExtending<Self> {
        Extending::new(self, Router::<Self::S, Self::E>::default())
    }
}

impl<In, E, S: Unpin + TryStream<Ok = In, Error = E>, R: FusedStream<Item = S>>
    RouteSlabMulticastExt for R
{
    type S = S;
    type In = In;
    type E = E;
}

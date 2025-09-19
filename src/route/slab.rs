use std::{
    collections::VecDeque,
    convert::Infallible,
    pin::Pin,
    task::{Context, Poll},
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
const OP_WAKE_CLOSE: usize = 1;
const OP_COUNT: usize = 2;

/// [`ReadyRoute`]/[`Stream`] implemented over the stream of incoming [`Sink`]s/[`Stream`]s.
#[pin_project]
pub struct Router<S, E = <S as TryStream>::Error> {
    connections: LinkedSlab<Connection<S>, OP_COUNT>,
    #[pin]
    next: Ready,
    #[pin]
    close: Ready,
    closed: VecDeque<(S, Option<E>)>,
}

impl<S, E> Default for Router<S, E> {
    fn default() -> Self {
        Self {
            connections: Default::default(),
            next: Default::default(),
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

    /// Add new connection.
    pub fn push(self: Pin<&mut Self>, stream: S) {
        let this = self.project();
        let key = this.connections.vacant_key();
        let next = this.next.downgrade();
        let close = this.close.downgrade();
        next.insert(key);
        close.insert(key);
        let connection = Connection {
            stream,
            next: ConnectionWaker::new(key, next),
            ready: ConnectionWaker::new(key, Default::default()),
            flush: ConnectionWaker::new(key, Default::default()),
            close: ConnectionWaker::new(key, close),
        };
        this.connections.insert(connection);
    }
}

impl<In, E, S: Unpin + TryStream<Ok = In, Error = E>> Stream for Router<S, E> {
    type Item = MultiItem<S, (usize, In)>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
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
        self.closed.is_empty() && self.connections.is_empty()
    }
}

impl<Out, E, S: Unpin + Sink<Out, Error = E>> Sink<(usize, Out)> for Router<S, E> {
    type Error = Infallible;

    fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Pending
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Pending
    }

    fn start_send(mut self: Pin<&mut Self>, (key, msg): (usize, Out)) -> Result<(), Self::Error> {
        let this = self.as_mut().project();
        if let Some(connection) = this.connections.get_mut(key)
            && let Err(e) = connection.stream.start_send_unpin(msg)
        {
            self.remove(key, Some(e));
        }
        Ok(())
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

impl<Out, E, S: Unpin + Sink<Out, Error = E>> FlushRoute<usize, Out> for Router<S, E> {
    fn poll_flush_route(
        mut self: Pin<&mut Self>,
        key: &usize,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        let this = self.as_mut().project();
        if let Some(connection) = this.connections.get_mut(*key)
            && let Err(e) = ready!(
                connection
                    .flush
                    .poll(cx, |cx| connection.stream.poll_flush_unpin(cx))
            )
        {
            self.remove(*key, Some(e));
        }
        Poll::Ready(Ok(()))
    }
}

impl<Out, E, S: Unpin + Sink<Out, Error = E>> ReadyRoute<usize, Out> for Router<S, E> {
    fn poll_ready_route(
        mut self: Pin<&mut Self>,
        key: &usize,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        let this = self.as_mut().project();
        if let Some(connection) = this.connections.get_mut(*key)
            && let Err(e) = ready!(
                connection
                    .ready
                    .poll(cx, |cx| connection.stream.poll_ready_unpin(cx))
            )
        {
            self.remove(*key, Some(e));
        }
        Poll::Ready(Ok(()))
    }
}

impl<S, E> PinnedExtend<S> for Router<S, E> {
    fn extend_pinned<T: IntoIterator<Item = S>>(mut self: Pin<&mut Self>, iter: T) {
        for stream in iter {
            self.as_mut().push(stream)
        }
    }
}

/// [`ReadyRoute`]/[`Stream`] Returned by [`RouterSlabExt::route_slab`].
pub type RouterExtending<R> = Extending<Router<<R as Stream>::Item>, R>;

/// Extension trait to auto-extend a [`Router`] from a stream of connections.
pub trait RouterSlabExt: Sized + FusedStream<Item: TryStream> {
    /// Extend the stream of connections (`self`) into a [`Router`].
    #[must_use]
    fn route_slab(self) -> RouterExtending<Self> {
        Extending::new(self, Default::default())
    }
}

impl<In, E, S: Unpin + TryStream<Ok = In, Error = E>, R: FusedStream<Item = S>> RouterSlabExt
    for R
{
}

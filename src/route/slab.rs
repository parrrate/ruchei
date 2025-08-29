use std::{
    convert::Infallible,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{Sink, SinkExt, Stream, StreamExt, ready, stream::FusedStream};
use pin_project::pin_project;
use ruchei_collections::linked_slab::LinkedSlab;
pub use ruchei_route::{RouteExt, RouteSink, Unroute, WithRoute};

use crate::{
    callback::OnClose,
    pinned_extend::{Extending, ExtendingRoute, PinnedExtend},
    ready_slab::{Connection, ConnectionWaker, Ready},
};

const OP_WAKE_NEXT: usize = 0;
const OP_WAKE_CLOSE: usize = 1;
const OP_COUNT: usize = 2;

/// [`RouteSink`]/[`Stream`] implemented over the stream of incoming [`Sink`]s/[`Stream`]s.
#[pin_project]
pub struct Router<S, F> {
    connections: LinkedSlab<Connection<S>, OP_COUNT>,
    #[pin]
    next: Ready,
    #[pin]
    close: Ready,
    callback: F,
}

impl<S, F> Router<S, F> {
    fn remove<E>(self: Pin<&mut Self>, key: usize, error: Option<E>)
    where
        F: OnClose<E>,
    {
        let this = self.project();
        this.callback.on_close(error);
        if let Some(connection) = this.connections.remove(key) {
            connection.next.waker.wake();
            connection.ready.waker.wake();
            connection.flush.waker.wake();
            connection.close.waker.wake();
        }
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

impl<In, E, S: Unpin + Stream<Item = Result<In, E>>, F: OnClose<E>> Stream for Router<S, F> {
    type Item = Result<(usize, In), Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.as_mut().project();
        this.next.register(cx);
        while let Some(key) = this
            .next
            .as_mut()
            .next::<OP_WAKE_NEXT, _, OP_COUNT>(this.connections)
        {
            if let Some(connection) = this.connections.get_mut(key)
                && let Poll::Ready(o) = connection
                    .next
                    .poll(cx, |cx| connection.stream.poll_next_unpin(cx))
            {
                match o {
                    Some(Ok(item)) => {
                        this.next.downgrade().insert(key);
                        return Poll::Ready(Some(Ok((key, item))));
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

impl<Out, E, S: Unpin + Sink<Out, Error = E>, F: OnClose<E>> RouteSink<usize, Out>
    for Router<S, F>
{
    type Error = Infallible;

    fn poll_ready(
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

    fn start_send(mut self: Pin<&mut Self>, key: usize, msg: Out) -> Result<(), Self::Error> {
        let this = self.as_mut().project();
        if let Some(connection) = this.connections.get_mut(key)
            && let Err(e) = connection.stream.start_send_unpin(msg)
        {
            self.remove(key, Some(e));
        }
        Ok(())
    }

    fn poll_flush(
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

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let mut this = self.as_mut().project();
        this.close.register(cx);
        while let Some(key) = this
            .close
            .as_mut()
            .next::<OP_WAKE_CLOSE, _, OP_COUNT>(this.connections)
        {
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

impl<S, F> PinnedExtend<S> for Router<S, F> {
    fn extend_pinned<T: IntoIterator<Item = S>>(mut self: Pin<&mut Self>, iter: T) {
        for stream in iter {
            self.as_mut().push(stream)
        }
    }
}

/// [`RouteSink`]/[`Stream`] Returned by [`RouterSlabExt::route_slab`].
pub type RouterExtending<F, R> = ExtendingRoute<Router<<R as RouterSlabExt>::S, F>, R>;

/// Extension trait to auto-extend a [`Router`] from a stream of connections.
pub trait RouterSlabExt: Sized {
    /// Single [`Stream`]/[`Sink`].
    type S;
    /// Error.
    type E;

    /// Extend the stream of connections (`self`) into a [`Router`].
    #[must_use]
    fn route_slab<F: OnClose<Self::E>>(self, callback: F) -> RouterExtending<F, Self>;
}

impl<In, E, S: Unpin + Stream<Item = Result<In, E>>, R: FusedStream<Item = S>> RouterSlabExt for R {
    type S = S;
    type E = E;

    fn route_slab<F: OnClose<Self::E>>(self, callback: F) -> RouterExtending<F, Self> {
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

use std::{
    collections::HashMap,
    convert::Infallible,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{Sink, SinkExt, Stream, StreamExt, ready, stream::FusedStream};
pub use ruchei_route::{RouteExt, RouteSink, Unroute, WithRoute};

use crate::{
    callback::OnClose,
    pinned_extend::{AutoPinnedExtend, Extending, ExtendingRoute},
    ready_keyed::{Connection, ConnectionWaker, Ready},
};

use super::Key;

/// [`RouteSink`]/[`Stream`] implemented over the stream of incoming [`Sink`]s/[`Stream`]s.
pub struct Router<K, S, F> {
    connections: HashMap<K, Connection<K, S>>,
    next: Ready<K>,
    close: Ready<K>,
    callback: F,
}

impl<K, S, F> Unpin for Router<K, S, F> {}

impl<K, S, F> AutoPinnedExtend for Router<K, S, F> {}

impl<K: Key, S, F> Router<K, S, F> {
    fn remove<E>(&mut self, key: &K, error: Option<E>)
    where
        F: OnClose<E>,
    {
        self.callback.on_close(error);
        self.next.remove(key);
        self.close.remove(key);
        if let Some(connection) = self.connections.remove(key) {
            connection.next.waker.wake();
            connection.ready.waker.wake();
            connection.flush.waker.wake();
            connection.close.waker.wake();
        }
    }

    /// Add new connection with its unique key.
    pub fn push(&mut self, key: K, stream: S) {
        let next = self.next.downgrade();
        let close = self.close.downgrade();
        next.insert(key.clone());
        close.insert(key.clone());
        let connection = Connection {
            stream,
            next: ConnectionWaker::new(key.clone(), next),
            ready: ConnectionWaker::new(key.clone(), Default::default()),
            flush: ConnectionWaker::new(key.clone(), Default::default()),
            close: ConnectionWaker::new(key.clone(), close),
        };
        self.connections.insert(key, connection);
    }
}

impl<In, K: Key, E, S: Unpin + Stream<Item = Result<In, E>>, F: OnClose<E>> Stream
    for Router<K, S, F>
{
    type Item = Result<(K, In), Infallible>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        while let Some(key) = this.next.next() {
            if let Some(connection) = this.connections.get_mut(&key)
                && let Poll::Ready(o) = connection
                    .next
                    .poll(cx, |cx| connection.stream.poll_next_unpin(cx))
            {
                match o {
                    Some(Ok(item)) => {
                        this.next.downgrade().insert(key.clone());
                        return Poll::Ready(Some(Ok((key, item))));
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

impl<Out, K: Key, E, S: Unpin + Sink<Out, Error = E>, F: OnClose<E>> RouteSink<K, Out>
    for Router<K, S, F>
{
    type Error = Infallible;

    fn poll_ready(
        self: Pin<&mut Self>,
        key: &K,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        if let Some(connection) = this.connections.get_mut(key)
            && let Err(e) = ready!(
                connection
                    .ready
                    .poll(cx, |cx| connection.stream.poll_ready_unpin(cx))
            )
        {
            this.remove(key, Some(e));
        }
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, key: K, msg: Out) -> Result<(), Self::Error> {
        let key = &key;
        let this = self.get_mut();
        if let Some(connection) = this.connections.get_mut(key)
            && let Err(e) = connection.stream.start_send_unpin(msg)
        {
            this.remove(key, Some(e));
        }
        Ok(())
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        key: &K,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        if let Some(connection) = this.connections.get_mut(key)
            && let Err(e) = ready!(
                connection
                    .flush
                    .poll(cx, |cx| connection.stream.poll_flush_unpin(cx))
            )
        {
            this.remove(key, Some(e));
        }
        Poll::Ready(Ok(()))
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

impl<K: Key, S, F> Extend<(K, S)> for Router<K, S, F> {
    fn extend<T: IntoIterator<Item = (K, S)>>(&mut self, iter: T) {
        for (key, stream) in iter {
            self.push(key, stream)
        }
    }
}

/// [`RouteSink`]/[`Stream`] Returned by [`RouterKeyedExt::route_keyed`].
pub type RouterExtending<F, R> =
    ExtendingRoute<Router<<R as RouterKeyedExt>::K, <R as RouterKeyedExt>::S, F>, R>;

/// Extension trait to auto-extend a [`Router`] from a stream of connections.
pub trait RouterKeyedExt: Sized {
    /// Key.
    type K;
    /// Single [`Stream`]/[`Sink`].
    type S;
    /// Error.
    type E;

    /// Extend the stream of connections (`self`) into a [`Router`].
    #[must_use]
    fn route_keyed<F: OnClose<Self::E>>(self, callback: F) -> RouterExtending<F, Self>;
}

impl<In, K: Key, E, S: Unpin + Stream<Item = Result<In, E>>, R: FusedStream<Item = (K, S)>>
    RouterKeyedExt for R
{
    type K = K;
    type S = S;
    type E = E;

    fn route_keyed<F: OnClose<Self::E>>(self, callback: F) -> RouterExtending<F, Self> {
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

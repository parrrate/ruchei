use std::{
    convert::Infallible,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{
    ready, stream::FusedStream, task::AtomicWaker, Sink, SinkExt, Stream, StreamExt,
};
use pin_project::pin_project;
use ruchei_callback::OnClose;

use crate::{
    collections::linked_slab::LinkedSlab,
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

/// [`Sink`]/[`Stream`] implemented over the stream of incoming [`Sink`]s/[`Stream`]s.
#[pin_project]
pub struct Dealer<S, F> {
    connections: LinkedSlab<Connection<S>, OP_COUNT>,
    #[pin]
    next: Ready,
    wready: AtomicWaker,
    #[pin]
    flush: Ready,
    #[pin]
    close: Ready,
    callback: F,
}

impl<S, F> Dealer<S, F> {
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

impl<In, E, S: Unpin + Stream<Item = Result<In, E>>, F: OnClose<E>> Stream for Dealer<S, F> {
    type Item = Result<In, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.as_mut().project();
        this.next.register(cx);
        while let Some(key) = this
            .next
            .as_mut()
            .next::<OP_WAKE_NEXT, _, OP_COUNT>(this.connections)
        {
            if let Some(connection) = this.connections.get_mut(key) {
                if let Poll::Ready(o) = connection
                    .next
                    .poll(cx, |cx| connection.stream.poll_next_unpin(cx))
                {
                    match o {
                        Some(Ok(item)) => {
                            this.next.downgrade().insert(key);
                            return Poll::Ready(Some(Ok(item)));
                        }
                        Some(Err(e)) => {
                            self.as_mut().remove(key, Some(e));
                        }
                        None => {
                            self.as_mut().remove(key, None);
                        }
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

impl<In, E, S: Unpin + Stream<Item = Result<In, E>>, F: OnClose<E>> FusedStream for Dealer<S, F> {
    fn is_terminated(&self) -> bool {
        self.connections.is_empty()
    }
}

impl<Out, E, S: Unpin + Sink<Out, Error = E>, F: OnClose<E>> Sink<Out> for Dealer<S, F> {
    type Error = Infallible;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.as_mut().project();
        this.wready.register(cx.waker());
        if let Some(key) = this.connections.front::<OP_DEAL>() {
            if let Some(connection) = this.connections.get_mut(key) {
                if let Err(e) = ready!(connection
                    .ready
                    .poll(cx, |cx| connection.stream.poll_ready_unpin(cx)))
                {
                    self.remove(key, Some(e));
                } else {
                    return Poll::Ready(Ok(()));
                }
            }
        }
        Poll::Pending
    }

    fn start_send(mut self: Pin<&mut Self>, msg: Out) -> Result<(), Self::Error> {
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
            }
        };
        Ok(())
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let mut this = self.as_mut().project();
        this.flush.register(cx);
        this.flush.downgrade().extend(
            this.connections
                .link_pops::<OP_IS_STARTED, _, _>(|key, connection| {
                    (!connection.link_contains::<OP_IS_FLUSHING>(key)).then_some(key)
                })
                .flatten(),
        );
        while let Some(key) = this
            .flush
            .as_mut()
            .next::<OP_WAKE_FLUSH, _, OP_COUNT>(this.connections)
        {
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
        while let Some(key) = this
            .close
            .as_mut()
            .next::<OP_WAKE_CLOSE, _, OP_COUNT>(this.connections)
        {
            if let Some(connection) = this.connections.get_mut(key) {
                if let Poll::Ready(r) = connection
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

impl<S, F> PinnedExtend<S> for Dealer<S, F> {
    fn extend_pinned<T: IntoIterator<Item = S>>(mut self: Pin<&mut Self>, iter: T) {
        for stream in iter {
            self.as_mut().push(stream)
        }
    }
}

/// [`Sink`]/[`Stream`] Returned by [`DealerSlabExt::deal_slab`].
pub type DealerExtending<F, R> = Extending<Dealer<<R as DealerSlabExt>::S, F>, R>;

/// Extension trait to auto-extend a [`Dealer`] from a stream of connections.
pub trait DealerSlabExt: Sized {
    /// Single [`Stream`]/[`Sink`].
    type S;
    /// Error.
    type E;

    /// Extend the stream of connections (`self`) into a [`Dealer`].
    #[must_use]
    fn deal_slab<F: OnClose<Self::E>>(self, callback: F) -> DealerExtending<F, Self>;
}

impl<In, E, S: Unpin + Stream<Item = Result<In, E>>, R: FusedStream<Item = S>> DealerSlabExt for R {
    type S = S;
    type E = E;

    fn deal_slab<F: OnClose<Self::E>>(self, callback: F) -> DealerExtending<F, Self> {
        Extending::new(
            self,
            Dealer {
                connections: Default::default(),
                next: Default::default(),
                wready: Default::default(),
                flush: Default::default(),
                close: Default::default(),
                callback,
            },
        )
    }
}

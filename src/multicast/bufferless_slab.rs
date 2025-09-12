use std::{
    convert::Infallible,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{Sink, SinkExt, Stream, StreamExt, stream::FusedStream};
use pin_project::pin_project;
use ruchei_callback::OnClose;
use ruchei_collections::{as_linked_slab::AsLinkedSlab, linked_slab::LinkedSlab};

use crate::{
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
pub struct Multicast<S, F> {
    connections: LinkedSlab<Connection<S>, OP_COUNT>,
    #[pin]
    next: Ready,
    #[pin]
    ready: Ready,
    #[pin]
    flush: Ready,
    #[pin]
    close: Ready,
    callback: F,
}

impl<S, F> Multicast<S, F> {
    fn remove<E>(self: Pin<&mut Self>, key: usize, error: Option<E>)
    where
        F: OnClose<E>,
    {
        let this = self.project();
        this.callback.on_close(error);
        if let Some(connection) = this.connections.try_remove(key) {
            connection.next.waker.wake();
            connection.ready.waker.wake();
            connection.flush.waker.wake();
            connection.close.waker.wake();
        }
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

impl<In, E, S: Unpin + Stream<Item = Result<In, E>>, F: OnClose<E>> Stream for Multicast<S, F> {
    type Item = Result<In, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.as_mut().project();
        this.next.register(cx);
        while let Some(key) = this
            .next
            .as_mut()
            .next::<OP_WAKE_NEXT, _, OP_COUNT>(this.connections)
        {
            if this.connections.link_pop_at::<OP_IS_READIED>(key) {
                this.ready.downgrade().insert(key);
            }
            if this.connections.link_pop_at::<OP_IS_FLUSHING>(key) {
                this.flush.downgrade().insert(key);
            }
            if let Some(connection) = this.connections.get_mut(key)
                && let Poll::Ready(o) = connection
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
            this = self.as_mut().project();
        }
        if this.connections.is_empty() {
            Poll::Ready(None)
        } else {
            Poll::Pending
        }
    }
}

impl<In, E, S: Unpin + Stream<Item = Result<In, E>>, F: OnClose<E>> FusedStream
    for Multicast<S, F>
{
    fn is_terminated(&self) -> bool {
        self.connections.is_empty()
    }
}

impl<Out: Clone, E, S: Unpin + Sink<Out, Error = E>, F: OnClose<E>> Sink<Out> for Multicast<S, F> {
    type Error = Infallible;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let mut this = self.as_mut().project();
        this.ready.register(cx);
        while let Some(key) = this
            .ready
            .as_mut()
            .next::<OP_WAKE_READY, _, OP_COUNT>(this.connections)
        {
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

    fn start_send(mut self: Pin<&mut Self>, msg: Out) -> Result<(), Self::Error> {
        let mut this = self.as_mut().project();
        while let Some(key) = this.connections.link_pop_front::<OP_IS_READIED>() {
            if let Some(connection) = this.connections.get_mut(key) {
                if let Err(e) = connection.stream.start_send_unpin(msg.clone()) {
                    self.as_mut().remove(key, Some(e));
                } else {
                    this.connections.link_push_back::<OP_IS_STARTED>(key);
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
        while let Some(key) = this
            .flush
            .as_mut()
            .next::<OP_WAKE_FLUSH, _, OP_COUNT>(this.connections)
        {
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

impl<S, F> PinnedExtend<S> for Multicast<S, F> {
    fn extend_pinned<T: IntoIterator<Item = S>>(mut self: Pin<&mut Self>, iter: T) {
        for stream in iter {
            self.as_mut().push(stream);
        }
    }
}

pub type MulticastExtending<F, R> = Extending<Multicast<<R as MulticastBufferlessSlab>::S, F>, R>;

pub trait MulticastBufferlessSlab: Sized {
    /// Single [`Stream`]/[`Sink`].
    type S;
    /// Error.
    type E;

    #[must_use]
    fn multicast_bufferless_slab<F: OnClose<Self::E>>(
        self,
        callback: F,
    ) -> MulticastExtending<F, Self>;
}

impl<In, E, S: Unpin + Stream<Item = Result<In, E>>, R: FusedStream<Item = S>>
    MulticastBufferlessSlab for R
{
    type S = S;
    type E = E;

    fn multicast_bufferless_slab<F: OnClose<Self::E>>(
        self,
        callback: F,
    ) -> MulticastExtending<F, Self> {
        Extending::new(
            self,
            Multicast {
                connections: Default::default(),
                next: Default::default(),
                ready: Default::default(),
                flush: Default::default(),
                close: Default::default(),
                callback,
            },
        )
    }
}

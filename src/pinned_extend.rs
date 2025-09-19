//! [`Extend`] for [`Pin`]ned values.
//!
//! Many of [`ruchei`] combinator traits return [`Extending`] wrapper around something that
//! implements [`PinnedExtend`] extending from `self`.

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{Sink, Stream, stream::FusedStream};
use pin_project::pin_project;
use route_sink::{FlushRoute, ReadyRoute, ReadySome};

/// Auto-derive [`PinnedExtend`] from [`Extend`].
pub trait AutoPinnedExtend {}

/// [`Extend`] equivalent for [`Pin<&mut I>`].
pub trait PinnedExtend<A> {
    /// [`Extend::extend`] equivalent for [`Pin<&mut I>`].
    fn extend_pinned<T: IntoIterator<Item = A>>(self: Pin<&mut Self>, iter: T);
}

impl<A, S: AutoPinnedExtend + Extend<A> + Unpin> PinnedExtend<A> for S {
    fn extend_pinned<T: IntoIterator<Item = A>>(self: Pin<&mut Self>, iter: T) {
        self.get_mut().extend(iter)
    }
}

/// Type extending a [`PinnedExtend`] value from a fused stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[pin_project]
pub struct Extending<S, R> {
    #[pin]
    incoming: R,
    #[pin]
    inner: S,
}

impl<S, R> Extending<S, R> {
    #[must_use]
    pub fn new(incoming: R, inner: S) -> Self {
        Self { incoming, inner }
    }

    /// Pinned mutable reference to the inner stream/sink.
    #[must_use]
    pub fn as_pin_mut(self: Pin<&mut Self>) -> Pin<&mut S> {
        self.project().inner
    }

    /// Convert into the inner stream/sink.
    #[must_use]
    pub fn into_inner(self) -> S {
        self.inner
    }

    #[must_use]
    pub fn incoming_pin_mut(self: Pin<&mut Self>) -> Pin<&mut R> {
        self.project().incoming
    }

    #[must_use]
    pub fn incoming(&self) -> &R {
        &self.incoming
    }

    #[must_use]
    pub fn incoming_mut(&mut self) -> &mut R {
        &mut self.incoming
    }

    #[must_use]
    pub fn into_incoming(self) -> R {
        self.incoming
    }
}

impl<S, R> AsRef<S> for Extending<S, R> {
    fn as_ref(&self) -> &S {
        &self.inner
    }
}

impl<S, R> AsMut<S> for Extending<S, R> {
    fn as_mut(&mut self) -> &mut S {
        &mut self.inner
    }
}

struct PollIter<'a, 'cx, R> {
    cx: &'a mut Context<'cx>,
    incoming: Pin<&'a mut R>,
}

impl<R: Stream> Iterator for PollIter<'_, '_, R> {
    type Item = R::Item;

    fn next(&mut self) -> Option<Self::Item> {
        match self.incoming.as_mut().poll_next(self.cx) {
            Poll::Ready(o) => o,
            Poll::Pending => None,
        }
    }
}

impl<A, S: Stream + PinnedExtend<A>, R: FusedStream<Item = A>> Stream for Extending<S, R> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        if !this.incoming.is_terminated() {
            this.inner.as_mut().extend_pinned(PollIter {
                cx,
                incoming: this.incoming.as_mut(),
            })
        }
        match this.inner.poll_next(cx) {
            Poll::Ready(None) if !this.incoming.is_terminated() => Poll::Pending,
            poll => poll,
        }
    }
}

impl<A, S: FusedStream + PinnedExtend<A>, R: FusedStream<Item = A>> FusedStream
    for Extending<S, R>
{
    fn is_terminated(&self) -> bool {
        self.inner.is_terminated() && self.incoming.is_terminated()
    }
}

impl<Item, S: Sink<Item>, R> Sink<Item> for Extending<S, R> {
    type Error = S::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().inner.poll_ready(cx)
    }

    fn start_send(self: Pin<&mut Self>, item: Item) -> Result<(), Self::Error> {
        self.project().inner.start_send(item)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().inner.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().inner.poll_close(cx)
    }
}

impl<Route, Msg, S: FlushRoute<Route, Msg>, R> FlushRoute<Route, Msg> for Extending<S, R> {
    fn poll_flush_route(
        self: Pin<&mut Self>,
        route: &Route,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.project().inner.poll_flush_route(route, cx)
    }

    fn poll_close_route(
        self: Pin<&mut Self>,
        route: &Route,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.project().inner.poll_close_route(route, cx)
    }
}

impl<Route, Msg, S: ReadyRoute<Route, Msg>, R> ReadyRoute<Route, Msg> for Extending<S, R> {
    fn poll_ready_route(
        self: Pin<&mut Self>,
        route: &Route,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.project().inner.poll_ready_route(route, cx)
    }
}

impl<Route, Msg, S: ReadySome<Route, Msg>, R> ReadySome<Route, Msg> for Extending<S, R> {
    fn poll_ready_some(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Route, Self::Error>> {
        self.project().inner.poll_ready_some(cx)
    }
}

pub trait ExtendingExt: Sized + FusedStream {
    #[must_use]
    fn extending<S: PinnedExtend<Self::Item>>(self, inner: S) -> Extending<S, Self> {
        Extending::new(self, inner)
    }
}

impl<R: FusedStream> ExtendingExt for R {}

impl<S: Default, R> From<R> for Extending<S, R> {
    fn from(incoming: R) -> Self {
        Self {
            incoming,
            inner: Default::default(),
        }
    }
}

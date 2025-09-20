//! Store extra data alongside a [`Future`]/[`Stream`]/[`Sink`].
//!
//! Commonly used for storing types with [`Drop`] logic.

#![no_std]

use core::{
    pin::Pin,
    task::{Context, Poll},
};

use futures_core::{FusedFuture, FusedStream, Future, Stream};
use futures_sink::Sink;
use pin_project::pin_project;
use route_sink::{FlushRoute, ReadyRoute, ReadySome};

/// [`Future`]/[`Stream`]/[`Sink`] with extra data attached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[pin_project]
pub struct WithExtra<T, Ex> {
    #[pin]
    inner: T,
    extra: Ex,
}

impl<T, Ex> WithExtra<T, Ex> {
    /// Attach extra data to a value.
    #[must_use]
    pub const fn new(inner: T, extra: Ex) -> Self {
        Self { inner, extra }
    }

    /// Unwrap into parts.
    #[must_use]
    pub fn into_inner(self) -> (T, Ex) {
        (self.inner, self.extra)
    }

    /// Get a pinned mutable reference to the inner value.
    #[must_use]
    pub fn as_pin_mut(self: Pin<&mut Self>) -> Pin<&mut T> {
        self.project().inner
    }
}

impl<T, Ex> From<(T, Ex)> for WithExtra<T, Ex> {
    fn from((inner, extra): (T, Ex)) -> Self {
        Self::new(inner, extra)
    }
}

impl<T, Ex> From<WithExtra<T, Ex>> for (T, Ex) {
    fn from(value: WithExtra<T, Ex>) -> Self {
        value.into_inner()
    }
}

impl<T, Ex: Default> From<T> for WithExtra<T, Ex> {
    fn from(inner: T) -> Self {
        Self::new(inner, Ex::default())
    }
}

impl<T, Ex> AsRef<T> for WithExtra<T, Ex> {
    fn as_ref(&self) -> &T {
        &self.inner
    }
}

impl<T, Ex> AsMut<T> for WithExtra<T, Ex> {
    fn as_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}

impl<T: Future, Ex> Future for WithExtra<T, Ex> {
    type Output = T::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project().inner.poll(cx)
    }
}

impl<T: FusedFuture, Ex> FusedFuture for WithExtra<T, Ex> {
    fn is_terminated(&self) -> bool {
        self.inner.is_terminated()
    }
}

impl<T: Stream, Ex> Stream for WithExtra<T, Ex> {
    type Item = T::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.project().inner.poll_next(cx)
    }
}

impl<T: FusedStream, Ex> FusedStream for WithExtra<T, Ex> {
    fn is_terminated(&self) -> bool {
        self.inner.is_terminated()
    }
}

impl<Item, T: Sink<Item>, Ex> Sink<Item> for WithExtra<T, Ex> {
    type Error = T::Error;

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

impl<Route, Msg, T: FlushRoute<Route, Msg>, Ex> FlushRoute<Route, Msg> for WithExtra<T, Ex> {
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

impl<Route, Msg, T: ReadyRoute<Route, Msg>, Ex> ReadyRoute<Route, Msg> for WithExtra<T, Ex> {
    fn poll_ready_route(
        self: Pin<&mut Self>,
        route: &Route,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.project().inner.poll_ready_route(route, cx)
    }
}

impl<Route, Msg, T: ReadySome<Route, Msg>, Ex> ReadySome<Route, Msg> for WithExtra<T, Ex> {
    fn poll_ready_some(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Route, Self::Error>> {
        self.project().inner.poll_ready_some(cx)
    }
}

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{future::FusedFuture, stream::FusedStream, Future, Sink, Stream};
use pin_project::pin_project;

#[pin_project]
pub struct WithExtra<T, Ex> {
    #[pin]
    inner: T,
    extra: Ex,
}

impl<T, Ex> WithExtra<T, Ex> {
    pub fn new(inner: T, extra: Ex) -> Self {
        Self { inner, extra }
    }

    pub fn as_mut(&mut self) -> &mut T {
        &mut self.inner
    }

    pub fn as_pin_mut(self: Pin<&mut Self>) -> Pin<&mut T> {
        self.project().inner
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

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{stream::FusedStream, Sink, Stream};
use pin_project::pin_project;

pub trait AutoPinnedExtend {}

pub trait PinnedExtend<A> {
    fn extend_pinned<T: IntoIterator<Item = A>>(self: Pin<&mut Self>, iter: T);
}

impl<A, S: AutoPinnedExtend + Extend<A> + Unpin> PinnedExtend<A> for S {
    fn extend_pinned<T: IntoIterator<Item = A>>(self: Pin<&mut Self>, iter: T) {
        self.get_mut().extend(iter)
    }
}

#[pin_project]
pub struct Extending<S, R> {
    #[pin]
    incoming: R,
    #[pin]
    inner: S,
}

impl<S, R> Extending<S, R> {
    pub fn new(incoming: R, inner: S) -> Self {
        Self { incoming, inner }
    }
}

struct PollIter<'a, 'cx, R> {
    cx: &'a mut Context<'cx>,
    incoming: Pin<&'a mut R>,
}

impl<'a, 'cx, R: Stream> Iterator for PollIter<'a, 'cx, R> {
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

pub trait ExteningExt: Sized {
    fn extending<S>(self, inner: S) -> Extending<S, Self>;
}

impl<R> ExteningExt for R {
    fn extending<S>(self, inner: S) -> Extending<S, Self> {
        Extending::new(self, inner)
    }
}

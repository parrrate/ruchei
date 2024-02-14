use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{
    stream::{FusedStream, FuturesUnordered},
    Future, Stream,
};
use pin_project::pin_project;

#[pin_project]
pub struct Concurrent<R, Fut> {
    #[pin]
    stream: R,
    #[pin]
    futures: FuturesUnordered<Fut>,
}

impl<Fut: Future, R: FusedStream<Item = Fut>> Stream for Concurrent<R, Fut> {
    type Item = Fut::Output;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        if !this.stream.is_terminated() {
            while let Poll::Ready(Some(future)) = this.stream.as_mut().poll_next(cx) {
                this.futures.push(future)
            }
        }
        match this.futures.poll_next(cx) {
            Poll::Ready(None) if !this.stream.is_terminated() => Poll::Pending,
            poll => poll,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let (lo, hi) = self.stream.size_hint();
        let extra = self.futures.len();
        (
            lo.saturating_add(extra),
            hi.and_then(|hi| hi.checked_add(extra)),
        )
    }
}

impl<Fut: Future, R: FusedStream<Item = Fut>> FusedStream for Concurrent<R, Fut> {
    fn is_terminated(&self) -> bool {
        self.stream.is_terminated() && self.futures.is_terminated()
    }
}

impl<Fut, R> From<R> for Concurrent<R, Fut> {
    fn from(stream: R) -> Self {
        Self {
            stream,
            futures: Default::default(),
        }
    }
}

pub trait ConcurrentExt: Sized {
    type Fut;

    fn concurrent(self) -> Concurrent<Self, Self::Fut>;
}

impl<Fut, R: Stream<Item = Fut>> ConcurrentExt for R {
    type Fut = Fut;

    fn concurrent(self) -> Concurrent<Self, Self::Fut> {
        self.into()
    }
}

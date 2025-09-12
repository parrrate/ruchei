use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{
    stream::{Fuse, FuturesUnordered},
    Future, Stream, StreamExt,
};

#[pin_project::pin_project]
pub struct Concurrent<R, Fut> {
    #[pin]
    stream: Fuse<R>,
    #[pin]
    futures: FuturesUnordered<Fut>,
}

impl<Fut: Future, R: Stream<Item = Fut>> Stream for Concurrent<R, Fut> {
    type Item = Fut::Output;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        while let Poll::Ready(Some(future)) = this.stream.as_mut().poll_next(cx) {
            this.futures.push(future)
        }
        match this.futures.poll_next(cx) {
            Poll::Ready(None) if !this.stream.is_done() => Poll::Pending,
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

impl<Fut, R: Stream<Item = Fut>> From<R> for Concurrent<R, Fut> {
    fn from(streams: R) -> Self {
        Self {
            stream: streams.fuse(),
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

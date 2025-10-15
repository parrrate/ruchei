use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures_core::{Stream, ready, stream::FusedStream};
use pin_project::pin_project;

#[pin_project]
#[derive(Debug)]
pub struct DedupEager<S, T = <S as Stream>::Item> {
    #[pin]
    inner: S,
    last: Option<T>,
}

impl<S, T> DedupEager<S, T> {
    pub(crate) fn new(inner: S) -> Self {
        Self { inner, last: None }
    }
}

impl<S: Stream<Item = T>, T: PartialEq + Clone> Stream for DedupEager<S, T> {
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        while let Some(item) = ready!(this.inner.as_mut().poll_next(cx)) {
            if this.last.as_ref().is_none_or(|last| *last != item) {
                *this.last = Some(item.clone());
                return Poll::Ready(Some(item));
            }
        }
        *this.last = None;
        Poll::Ready(None)
    }
}

impl<S: FusedStream<Item: PartialEq + Clone>> FusedStream for DedupEager<S> {
    fn is_terminated(&self) -> bool {
        self.inner.is_terminated()
    }
}

crate::macros::forward!(DedupEager, T);

#[test]
fn unique() {
    use crate::AsyncItertools;
    let x = futures_util::stream::iter([1, 2, 3]);
    let x = futures_executor::block_on_stream(x.dedup_eager()).collect::<Vec<_>>();
    assert_eq!(x, [1, 2, 3]);
}

#[test]
fn all_duplicates() {
    use crate::AsyncItertools;
    let x = futures_util::stream::iter([1, 1, 2, 2, 3, 3]);
    let x = futures_executor::block_on_stream(x.dedup_eager()).collect::<Vec<_>>();
    assert_eq!(x, [1, 2, 3]);
}

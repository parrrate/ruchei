use core::{
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{Stream, ready};
use pin_project::pin_project;

#[pin_project]
pub struct AllEqual<S, T = <S as Stream>::Item> {
    #[pin]
    pub(crate) stream: S,
    pub(crate) first: Option<T>,
}

impl<S: Stream<Item = T>, T: PartialEq> Future for AllEqual<S, T> {
    type Output = bool;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        if this.first.is_none() {
            *this.first = ready!(this.stream.as_mut().poll_next(cx));
        }
        let Some(first) = this.first.as_ref() else {
            return Poll::Ready(true);
        };
        while let Some(ref item) = ready!(this.stream.as_mut().poll_next(cx)) {
            if first != item {
                return Poll::Ready(false);
            }
        }
        Poll::Ready(true)
    }
}

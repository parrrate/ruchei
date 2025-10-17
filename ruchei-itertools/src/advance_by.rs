use core::{
    num::NonZero,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{Stream, StreamExt, ready};

pub struct AdvanceBy<'a, S: ?Sized> {
    pub(crate) stream: &'a mut S,
    pub(crate) remaining: usize,
}

impl<'a, S: ?Sized + Unpin + Stream> Future for AdvanceBy<'a, S> {
    type Output = Result<(), NonZero<usize>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            break if let Some(remaining) = NonZero::new(self.remaining) {
                if ready!(self.stream.poll_next_unpin(cx)).is_some() {
                    self.remaining -= 1;
                    continue;
                } else {
                    Poll::Ready(Err(remaining))
                }
            } else {
                Poll::Ready(Ok(()))
            };
        }
    }
}

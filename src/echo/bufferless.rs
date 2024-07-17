use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{stream::FusedStream, Future, Sink, Stream};
use pin_project::pin_project;

#[derive(Debug)]
#[pin_project]
#[must_use = "futures must be awaited"]
pub struct Echo<T, S> {
    #[pin]
    stream: S,
    item: Option<T>,
    started: bool,
}

impl<T, E, S: FusedStream<Item = Result<T, E>> + Sink<T, Error = E>> Future for Echo<T, S> {
    type Output = Result<(), E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        if this.stream.is_terminated() {
            return Poll::Ready(Ok(()));
        }
        loop {
            match this.item.take() {
                Some(item) => match this.stream.as_mut().poll_ready(cx)? {
                    Poll::Ready(()) => {
                        this.stream.as_mut().start_send(item)?;
                        *this.started = true;
                    }
                    Poll::Pending => {
                        *this.item = Some(item);
                        break;
                    }
                },
                None => match this.stream.as_mut().poll_next(cx)? {
                    Poll::Ready(Some(item)) => *this.item = Some(item),
                    Poll::Ready(None) => return Poll::Ready(Ok(())),
                    Poll::Pending => break,
                },
            }
        }
        if *this.started && this.stream.as_mut().poll_flush(cx)?.is_ready() {
            *this.started = false;
        }
        Poll::Pending
    }
}

impl<T, E, S: Stream<Item = Result<T, E>>> From<S> for Echo<T, S> {
    fn from(stream: S) -> Self {
        Self {
            stream,
            item: None,
            started: false,
        }
    }
}

pub trait EchoBufferless: Sized {
    /// Item yielded and accepted by `self` as [`Stream`]/[`Sink`].
    type T;

    fn echo_bufferless(self) -> Echo<Self::T, Self>;
}

impl<T, E, S: FusedStream<Item = Result<T, E>> + Sink<T, Error = E>> EchoBufferless for S {
    type T = T;

    fn echo_bufferless(self) -> Echo<Self::T, Self> {
        self.into()
    }
}

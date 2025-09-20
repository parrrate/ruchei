use std::{
    collections::VecDeque,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{Future, Sink, TryStream, stream::FusedStream};
use pin_project::pin_project;

#[derive(Debug)]
#[pin_project]
#[must_use = "futures must be awaited"]
pub struct Echo<S, T = <S as TryStream>::Ok> {
    #[pin]
    stream: S,
    queue: VecDeque<T>,
    item: Option<T>,
    started: bool,
}

impl<S: Default, T> Default for Echo<S, T> {
    fn default() -> Self {
        S::default().into()
    }
}

impl<T, E, S: FusedStream + TryStream<Ok = T, Error = E> + Sink<T, Error = E>> Future
    for Echo<S, T>
{
    type Output = Result<(), E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        loop {
            let mut pending = false;
            if !this.stream.is_terminated() {
                if let Poll::Ready(ready) = this.stream.as_mut().try_poll_next(cx)? {
                    match ready {
                        Some(t) => this.queue.push_back(t),
                        None => return Poll::Ready(Ok(())),
                    }
                } else {
                    pending = true;
                }
            } else {
                return Poll::Ready(Ok(()));
            }
            match this.item.take() {
                Some(item) => match this.stream.as_mut().poll_ready(cx)? {
                    Poll::Ready(()) => {
                        this.stream.as_mut().start_send(item)?;
                        *this.started = true;
                    }
                    Poll::Pending => {
                        *this.item = Some(item);
                        if pending {
                            return Poll::Pending;
                        }
                    }
                },
                None => match this.queue.pop_front() {
                    Some(item) => *this.item = Some(item),
                    None => {
                        if pending {
                            break;
                        }
                    }
                },
            }
        }
        if *this.started && this.stream.as_mut().poll_flush(cx)?.is_ready() {
            *this.started = false;
        }
        Poll::Pending
    }
}

impl<T, S> From<S> for Echo<S, T> {
    fn from(stream: S) -> Self {
        Self {
            stream,
            queue: Default::default(),
            item: None,
            started: false,
        }
    }
}

pub trait EchoInterleaved:
    Sized + FusedStream + TryStream<Ok = Self::T, Error = Self::E> + Sink<Self::T, Error = Self::E>
{
    type T;
    type E;

    fn echo_interleaved(self) -> Echo<Self> {
        self.into()
    }
}

impl<T, E, S: FusedStream + TryStream<Ok = T, Error = E> + Sink<T, Error = E>> EchoInterleaved
    for S
{
    type T = T;
    type E = E;
}

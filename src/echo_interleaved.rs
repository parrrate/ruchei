use std::{
    collections::VecDeque,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{stream::FusedStream, Future, Sink, Stream};
use pin_project::pin_project;

#[pin_project]
pub struct Echo<T, S> {
    #[pin]
    stream: S,
    queue: VecDeque<T>,
    item: Option<T>,
    started: bool,
}

impl<T, E, S: FusedStream<Item = Result<T, E>> + Sink<T, Error = E>> Future for Echo<T, S> {
    type Output = Result<(), E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        loop {
            let mut pending = false;
            if !this.stream.is_terminated() {
                if let Poll::Ready(ready) = this.stream.as_mut().poll_next(cx)? {
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

impl<T, E, S: Stream<Item = Result<T, E>>> From<S> for Echo<T, S> {
    fn from(stream: S) -> Self {
        Self {
            stream,
            queue: Default::default(),
            item: None,
            started: false,
        }
    }
}

pub trait EchoInterleaved: Sized {
    type T;

    fn echo_interleaved(self) -> Echo<Self::T, Self>;
}

impl<T, E, S: Stream<Item = Result<T, E>>> EchoInterleaved for S {
    type T = T;

    fn echo_interleaved(self) -> Echo<Self::T, Self> {
        self.into()
    }
}

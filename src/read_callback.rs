use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{stream::Fuse, Sink, Stream, StreamExt};
use pin_project::pin_project;

use crate::callback::OnItem;

#[pin_project]
pub struct ReadCallback<S, F> {
    #[pin]
    stream: Fuse<S>,
    callback: F,
}

impl<In, E, S: Stream<Item = Result<In, E>>, F: OnItem<In>> ReadCallback<S, F> {
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), E>> {
        let mut this = self.project();
        loop {
            match this.stream.as_mut().poll_next(cx)? {
                Poll::Ready(Some(item)) => this.callback.on_item(item),
                Poll::Ready(None) => break Poll::Ready(Ok(())),
                Poll::Pending => break Poll::Pending,
            }
        }
    }
}

impl<In, Out, E, S: Stream<Item = Result<In, E>> + Sink<Out, Error = E>, F: OnItem<In>> Sink<Out>
    for ReadCallback<S, F>
{
    type Error = E;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let _ = self.as_mut().poll_next(cx);
        self.project().stream.poll_ready(cx)
    }

    fn start_send(self: Pin<&mut Self>, item: Out) -> Result<(), Self::Error> {
        self.project().stream.start_send(item)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let _ = self.as_mut().poll_next(cx);
        self.project().stream.poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let _ = self.as_mut().poll_next(cx);
        self.project().stream.poll_close(cx)
    }
}

impl<In, E, S: Stream<Item = Result<In, E>>, F> ReadCallback<S, F> {
    pub fn new(stream: S, callback: F) -> Self {
        Self {
            stream: stream.fuse(),
            callback,
        }
    }
}

pub trait ReadCallbackExt: Sized {
    type In;

    fn read_callback<F: OnItem<Self::In>>(self, callback: F) -> ReadCallback<Self, F>;
}

impl<In, E, S: Stream<Item = Result<In, E>>> ReadCallbackExt for S {
    type In = In;

    fn read_callback<F: OnItem<Self::In>>(self, callback: F) -> ReadCallback<Self, F> {
        ReadCallback::new(self, callback)
    }
}

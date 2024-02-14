use std::{
    convert::Infallible,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{ready, stream::FusedStream, Sink, Stream};
use pin_project::pin_project;

use crate::callback::OnItem;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[pin_project]
pub struct ReadCallback<S, F> {
    #[pin]
    stream: S,
    callback: F,
}

impl<In, E, S: FusedStream<Item = Result<In, E>>, F: OnItem<In>> ReadCallback<S, F> {
    fn poll_inner(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), E>> {
        let mut this = self.project();
        if !this.stream.is_terminated() {
            loop {
                match this.stream.as_mut().poll_next(cx)? {
                    Poll::Ready(Some(item)) => this.callback.on_item(item),
                    Poll::Ready(None) => break Poll::Pending,
                    Poll::Pending => break Poll::Ready(Ok(())),
                }
            }
        } else {
            Poll::Pending
        }
    }
}

impl<In, E, S: FusedStream<Item = Result<In, E>>, F: OnItem<In>> Stream for ReadCallback<S, F> {
    type Item = Result<Infallible, E>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.poll_inner(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(None),
            Poll::Ready(Err(e)) => Poll::Ready(Some(Err(e))),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<In, E, S: FusedStream<Item = Result<In, E>>, F: OnItem<In>> FusedStream
    for ReadCallback<S, F>
{
    fn is_terminated(&self) -> bool {
        self.stream.is_terminated()
    }
}

impl<In, Out, E, S: FusedStream<Item = Result<In, E>> + Sink<Out, Error = E>, F: OnItem<In>>
    Sink<Out> for ReadCallback<S, F>
{
    type Error = E;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        ready!(self.as_mut().poll_inner(cx))?;
        if let poll @ Poll::Ready(_) = self.as_mut().project().stream.poll_ready(cx) {
            return poll;
        }
        ready!(self.as_mut().poll_inner(cx))?;
        self.project().stream.poll_ready(cx)
    }

    fn start_send(self: Pin<&mut Self>, item: Out) -> Result<(), Self::Error> {
        self.project().stream.start_send(item)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        ready!(self.as_mut().poll_inner(cx))?;
        if let poll @ Poll::Ready(_) = self.as_mut().project().stream.poll_flush(cx) {
            return poll;
        }
        ready!(self.as_mut().poll_inner(cx))?;
        self.project().stream.poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        ready!(self.as_mut().poll_inner(cx))?;
        if let poll @ Poll::Ready(_) = self.as_mut().project().stream.poll_close(cx) {
            return poll;
        }
        ready!(self.as_mut().poll_inner(cx))?;
        self.project().stream.poll_close(cx)
    }
}

impl<In, E, S: Stream<Item = Result<In, E>>, F> ReadCallback<S, F> {
    pub fn new(stream: S, callback: F) -> Self {
        Self { stream, callback }
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

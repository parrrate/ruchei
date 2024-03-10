use std::{
    convert::Infallible,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{
    stream::{FusedStream, FuturesUnordered, SelectAll},
    Sink, Stream,
};
use pin_project::pin_project;

use crate::{
    callback::OnClose,
    owned_close::OwnedClose,
    pinned_extend::{AutoPinnedExtend, Extending, ExtendingExt},
};

#[derive(Debug)]
#[pin_project]
struct Unicast<S, F> {
    #[pin]
    stream: S,
    callback: F,
}

impl<In, E, S: Stream<Item = Result<In, E>>, F: OnClose<E>> Stream for Unicast<S, F> {
    type Item = In;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        this.stream.poll_next(cx).map(|o| match o {
            Some(Ok(out)) => Some(out),
            Some(Err(e)) => {
                this.callback.on_close(Some(e));
                None
            }
            None => {
                this.callback.on_close(None);
                None
            }
        })
    }
}

#[derive(Debug)]
#[pin_project]
pub struct Multicast<S, Out, F> {
    #[pin]
    select: SelectAll<Unicast<S, F>>,
    #[pin]
    closing: FuturesUnordered<OwnedClose<S, Out>>,
    callback: F,
}

impl<In, Out: Clone, E, S: Unpin + Stream<Item = Result<In, E>>, F: OnClose<E>>
    Multicast<S, Out, F>
{
    fn poll_next_infallible(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<In>> {
        self.as_mut().project().select.poll_next(cx)
    }
}

impl<In, Out: Clone, E, S: Unpin + Stream<Item = Result<In, E>>, F: OnClose<E>> Stream
    for Multicast<S, Out, F>
{
    type Item = Result<In, Infallible>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.poll_next_infallible(cx).map(|o| o.map(Ok))
    }
}

impl<In, Out: Clone, E, S: Unpin + Stream<Item = Result<In, E>>, F: OnClose<E>> FusedStream
    for Multicast<S, Out, F>
{
    fn is_terminated(&self) -> bool {
        self.select.is_terminated()
    }
}

impl<
        In,
        Out: Clone,
        E,
        S: Unpin + Stream<Item = Result<In, E>> + Sink<Out, Error = E>,
        F: OnClose<E>,
    > Sink<Out> for Multicast<S, Out, F>
{
    type Error = Infallible;

    fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, _: Out) -> Result<(), Self::Error> {
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let mut this = self.project();
        if !this.select.is_empty() {
            for unicast in std::mem::take(this.select.get_mut()) {
                this.closing.push(unicast.stream.into())
            }
        }
        loop {
            match this.closing.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(()))) => this.callback.on_close(None),
                Poll::Ready(Some(Err(e))) => this.callback.on_close(Some(e)),
                Poll::Ready(None) => break Poll::Ready(Ok(())),
                Poll::Pending => break Poll::Pending,
            }
        }
    }
}

impl<
        In,
        Out: Clone,
        E,
        S: Unpin + Stream<Item = Result<In, E>> + Sink<Out, Error = E>,
        F: OnClose<E>,
    > Multicast<S, Out, F>
{
    pub fn new(callback: F) -> Self {
        Self {
            select: Default::default(),
            closing: Default::default(),
            callback,
        }
    }

    pub fn push(&mut self, stream: S) {
        self.select.push(Unicast {
            stream,
            callback: self.callback.clone(),
        });
    }
}

impl<
        In,
        Out: Clone,
        E,
        S: Unpin + Stream<Item = Result<In, E>> + Sink<Out, Error = E>,
        F: OnClose<E>,
    > From<F> for Multicast<S, Out, F>
{
    fn from(callback: F) -> Self {
        Self::new(callback)
    }
}

impl<
        In,
        Out: Clone,
        E,
        S: Unpin + Stream<Item = Result<In, E>> + Sink<Out, Error = E>,
        F: Default + OnClose<E>,
    > Default for Multicast<S, Out, F>
{
    fn default() -> Self {
        Self::new(F::default())
    }
}

impl<
        In,
        Out: Clone,
        E,
        S: Unpin + Stream<Item = Result<In, E>> + Sink<Out, Error = E>,
        F: OnClose<E>,
    > Extend<S> for Multicast<S, Out, F>
{
    fn extend<T: IntoIterator<Item = S>>(&mut self, iter: T) {
        for stream in iter {
            self.push(stream)
        }
    }
}

impl<
        In,
        Out: Clone,
        E,
        S: Unpin + Stream<Item = Result<In, E>> + Sink<Out, Error = E>,
        F: Default + OnClose<E>,
    > FromIterator<S> for Multicast<S, Out, F>
{
    fn from_iter<T: IntoIterator<Item = S>>(iter: T) -> Self {
        let mut this = Self::default();
        this.extend(iter);
        this
    }
}

impl<S, Out, F> AutoPinnedExtend for Multicast<S, Out, F> {}

pub trait MulticastIgnore<Out>: Sized {
    type S;

    type E;

    fn multicast_ignore<F: OnClose<Self::E>>(
        self,
        callback: F,
    ) -> Extending<Multicast<Self::S, Out, F>, Self>;
}

impl<
        In,
        Out: Clone,
        E,
        S: Unpin + Stream<Item = Result<In, E>> + Sink<Out, Error = E>,
        R: Stream<Item = S>,
    > MulticastIgnore<Out> for R
{
    type S = S;

    type E = E;

    fn multicast_ignore<F: OnClose<Self::E>>(
        self,
        callback: F,
    ) -> Extending<Multicast<Self::S, Out, F>, Self> {
        self.extending(Multicast::new(callback))
    }
}

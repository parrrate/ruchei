use std::{
    convert::Infallible,
    pin::Pin,
    task::{Context, Poll, Waker},
};

use futures_util::{
    stream::{Fuse, FuturesUnordered, SelectAll},
    Future, Sink, Stream, StreamExt,
};
use pin_project::pin_project;

use crate::{
    callback::OnClose,
    owned_close::OwnedClose,
    wait_all::{Completable, CompleteOne, WaitMany},
};

#[pin_project]
struct Unicast<S, Out, F> {
    #[pin]
    stream: S,
    waker: Option<Waker>,
    readying: CompleteOne,
    flushing: CompleteOne,
    ready: bool,
    started: Option<Out>,
    callback: F,
}

impl<S, Out, F> Unicast<S, Out, F> {
    fn wake(&mut self) {
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
}

impl<In, Out, E, S: Stream<Item = Result<In, E>> + Sink<Out, Error = E>, F: OnClose<E>> Stream
    for Unicast<S, Out, F>
{
    type Item = In;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        if this.readying.pending() {
            match this.stream.as_mut().poll_ready(cx) {
                Poll::Ready(Ok(())) => {
                    *this.ready = true;
                    this.readying.complete()
                }
                Poll::Ready(Err(e)) => {
                    this.callback.on_close(Some(e));
                    return Poll::Ready(None);
                }
                Poll::Pending => {}
            }
        }
        if *this.ready {
            if let Some(out) = this.started.take() {
                match this.stream.as_mut().start_send(out) {
                    Ok(()) => {
                        *this.ready = false;
                    }
                    Err(e) => {
                        this.callback.on_close(Some(e));
                        return Poll::Ready(None);
                    }
                }
            }
        }
        if this.flushing.pending() {
            match this.stream.as_mut().poll_flush(cx) {
                Poll::Ready(Ok(())) => this.flushing.complete(),
                Poll::Ready(Err(e)) => {
                    this.callback.on_close(Some(e));
                    return Poll::Ready(None);
                }
                Poll::Pending => {}
            }
        }
        *this.waker = Some(cx.waker().clone());
        this.stream.poll_next(cx).map(|o| match o {
            Some(Ok(item)) => Some(item),
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

#[pin_project]
pub struct Multicast<S, Out, F, R> {
    #[pin]
    streams: Fuse<R>,
    #[pin]
    select: SelectAll<Unicast<S, Out, F>>,
    #[pin]
    readying: WaitMany,
    #[pin]
    flushing: WaitMany,
    #[pin]
    closing: FuturesUnordered<OwnedClose<S, Out>>,
    ready: bool,
    flushed: bool,
    callback: F,
}

impl<
        In,
        Out,
        E,
        S: Unpin + Stream<Item = Result<In, E>> + Sink<Out, Error = E>,
        F: OnClose<E>,
        R: Stream<Item = S>,
    > Multicast<S, Out, F, R>
{
    fn poll_next_infallible(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<In>> {
        let mut this = self.project();
        while let Poll::Ready(Some(stream)) = this.streams.as_mut().poll_next(cx) {
            this.select.push(Unicast {
                stream,
                waker: None,
                readying: Default::default(),
                flushing: Default::default(),
                ready: false,
                started: None,
                callback: this.callback.clone(),
            });
        }
        match this.select.poll_next(cx) {
            Poll::Ready(None) if !this.streams.is_done() => Poll::Pending,
            poll => poll,
        }
    }
}

impl<
        In,
        Out,
        E,
        S: Unpin + Stream<Item = Result<In, E>> + Sink<Out, Error = E>,
        F: OnClose<E>,
        R: Stream<Item = S>,
    > Stream for Multicast<S, Out, F, R>
{
    type Item = Result<In, Infallible>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.poll_next_infallible(cx).map(|o| o.map(Ok))
    }
}

impl<
        In,
        Out: Clone,
        E,
        S: Unpin + Stream<Item = Result<In, E>> + Sink<Out, Error = E>,
        F: OnClose<E>,
        R: Stream<Item = S>,
    > Sink<Out> for Multicast<S, Out, F, R>
{
    type Error = Infallible;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let mut this = self.project();
        loop {
            match this.readying.as_mut().poll(cx) {
                Poll::Ready(()) => {
                    if *this.ready {
                        *this.ready = false;
                        break Poll::Ready(Ok(()));
                    } else {
                        *this.ready = true;
                        let completable = Completable::default();
                        for unicast in this.select.iter_mut() {
                            unicast.readying.completable(completable.clone());
                            unicast.wake();
                        }
                        this.readying.completable(completable);
                    }
                }
                Poll::Pending => break Poll::Pending,
            }
        }
    }

    fn start_send(self: Pin<&mut Self>, item: Out) -> Result<(), Self::Error> {
        let mut this = self.project();
        for unicast in this.select.iter_mut() {
            if unicast.ready {
                unicast.started = Some(item.clone());
                unicast.wake();
            }
        }
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let mut this = self.project();
        loop {
            match this.flushing.as_mut().poll(cx) {
                Poll::Ready(()) => {
                    if *this.flushed {
                        *this.flushed = false;
                        break Poll::Ready(Ok(()));
                    } else {
                        *this.flushed = true;
                        let completable = Completable::default();
                        for unicast in this.select.iter_mut() {
                            unicast.flushing.completable(completable.clone());
                            unicast.wake();
                        }
                        this.flushing.completable(completable);
                    }
                }
                Poll::Pending => break Poll::Pending,
            }
        }
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
        Out,
        E,
        S: Unpin + Stream<Item = Result<In, E>> + Sink<Out, Error = E>,
        F: OnClose<E>,
        R: Stream<Item = S>,
    > Multicast<S, Out, F, R>
{
    pub fn new(streams: R, callback: F) -> Self {
        Self {
            streams: streams.fuse(),
            select: Default::default(),
            readying: Default::default(),
            flushing: Default::default(),
            closing: Default::default(),
            ready: false,
            flushed: false,
            callback,
        }
    }
}

pub trait MulticastBufferless<Out>: Sized {
    type S;

    type E;

    fn multicast_bufferless<F: OnClose<Self::E>>(
        self,
        callback: F,
    ) -> Multicast<Self::S, Out, F, Self>;
}

impl<
        In,
        Out,
        E,
        S: Unpin + Stream<Item = Result<In, E>> + Sink<Out, Error = E>,
        R: Stream<Item = S>,
    > MulticastBufferless<Out> for R
{
    type S = S;

    type E = E;

    fn multicast_bufferless<F: OnClose<Self::E>>(
        self,
        callback: F,
    ) -> Multicast<Self::S, Out, F, Self> {
        Multicast::new(self, callback)
    }
}

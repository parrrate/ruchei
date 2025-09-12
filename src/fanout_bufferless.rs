use std::{
    convert::Infallible,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{
    stream::{FusedStream, FuturesUnordered, SelectAll},
    task::AtomicWaker,
    Future, Sink, Stream,
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
    waker: AtomicWaker,
    readying: CompleteOne,
    flushing: CompleteOne,
    ready: bool,
    started: Option<Out>,
    callback: F,
}

impl<S, Out, F> Unicast<S, Out, F> {
    fn wake(&mut self) {
        self.waker.wake();
    }
}

impl<In, Out, E, S: Stream<Item = Result<In, E>> + Sink<Out, Error = E>, F: OnClose<E>> Stream
    for Unicast<S, Out, F>
{
    type Item = In;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        this.waker.register(cx.waker());
        if *this.ready {
            if let Some(out) = this.started.take() {
                match this.stream.as_mut().start_send(out) {
                    Ok(()) => *this.ready = false,
                    Err(e) => {
                        this.callback.on_close(Some(e));
                        return Poll::Ready(None);
                    }
                }
            }
        }
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
    streams: R,
    #[pin]
    select: SelectAll<Unicast<S, Out, F>>,
    #[pin]
    readying: WaitMany,
    #[pin]
    flushing: WaitMany,
    #[pin]
    closing: FuturesUnordered<OwnedClose<S, Out>>,
    polled_for_ready: bool,
    polled_for_flush: bool,
    callback: F,
}

impl<
        In,
        Out,
        E,
        S: Unpin + Stream<Item = Result<In, E>> + Sink<Out, Error = E>,
        F: OnClose<E>,
        R: FusedStream<Item = S>,
    > Multicast<S, Out, F, R>
{
    fn poll_next_infallible(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<In>> {
        let mut this = self.project();
        if !this.streams.is_terminated() {
            while let Poll::Ready(Some(stream)) = this.streams.as_mut().poll_next(cx) {
                this.select.push(Unicast {
                    stream,
                    waker: Default::default(),
                    readying: Default::default(),
                    flushing: Default::default(),
                    ready: false,
                    started: None,
                    callback: this.callback.clone(),
                });
            }
        }
        match this.select.poll_next(cx) {
            Poll::Ready(None) if !this.streams.is_terminated() => Poll::Pending,
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
        R: FusedStream<Item = S>,
    > Stream for Multicast<S, Out, F, R>
{
    type Item = Result<In, Infallible>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.poll_next_infallible(cx).map(|o| o.map(Ok))
    }
}

impl<
        In,
        Out,
        E,
        S: Unpin + Stream<Item = Result<In, E>> + Sink<Out, Error = E>,
        F: OnClose<E>,
        R: FusedStream<Item = S>,
    > FusedStream for Multicast<S, Out, F, R>
{
    fn is_terminated(&self) -> bool {
        self.streams.is_terminated() && self.select.is_terminated()
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
        if !*this.polled_for_ready {
            *this.polled_for_ready = true;
            let completable = Completable::default();
            for unicast in this.select.iter_mut() {
                unicast.readying.completable(completable.clone());
                unicast.wake();
            }
            this.readying.completable(completable);
        }
        match this.readying.as_mut().poll(cx) {
            Poll::Ready(()) => {
                *this.polled_for_ready = false;
                Poll::Ready(Ok(()))
            }
            Poll::Pending => Poll::Pending,
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
        if !*this.polled_for_flush {
            *this.polled_for_flush = true;
            let completable = Completable::default();
            for unicast in this.select.iter_mut() {
                unicast.flushing.completable(completable.clone());
                unicast.wake();
            }
            this.flushing.completable(completable);
        }
        match this.flushing.as_mut().poll(cx) {
            Poll::Ready(()) => {
                *this.polled_for_flush = false;
                Poll::Ready(Ok(()))
            }
            Poll::Pending => Poll::Pending,
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
        R,
    > Multicast<S, Out, F, R>
{
    pub fn new(streams: R, callback: F) -> Self {
        Self {
            streams,
            select: Default::default(),
            readying: Default::default(),
            flushing: Default::default(),
            closing: Default::default(),
            polled_for_ready: false,
            polled_for_flush: false,
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

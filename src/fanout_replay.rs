use std::{
    collections::HashMap,
    convert::Infallible,
    pin::Pin,
    task::{Context, Poll},
};

use futures_channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use futures_util::{
    ready,
    stream::{FusedStream, FuturesUnordered, SelectAll},
    Future, Sink, Stream,
};
use pin_project::pin_project;

use crate::{callback::OnClose, key_drop::KeyDropCopy};

#[derive(PartialEq, Eq, Hash, Clone, Copy)]
struct Key(usize);

#[derive(Clone, Copy)]
struct Index(usize);

#[derive(Default)]
enum State<Out> {
    #[default]
    Flushed,
    Readying(Out, Index),
    Started,
}

impl<Out> State<Out> {
    fn take(&mut self) -> Self {
        std::mem::take(self)
    }

    fn is_empty(&self) -> bool {
        matches!(self, State::Flushed | State::Started)
    }
}

type Done<Out> = (Key, Index, UnboundedSender<(Out, Index)>);

#[pin_project]
struct Unicast<S, Out, F> {
    #[pin]
    stream: S,
    #[pin]
    out_r: UnboundedReceiver<(Out, Index)>,
    state: State<Out>,
    out_s: UnboundedSender<(Out, Index)>,
    done_s: UnboundedSender<Done<Out>>,
    key_drop: KeyDropCopy<Key>,
    callback: F,
}

impl<In, Out, E, S: Sink<Out, Error = E> + Stream<Item = Result<In, E>>, F: OnClose<E>>
    Unicast<S, Out, F>
{
    fn state(self: Pin<&mut Self>) -> &mut State<Out> {
        self.project().state
    }

    fn poll_out(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.project();
        match this.out_r.poll_next(cx) {
            Poll::Ready(Some((out, ix))) => {
                *this.state = State::Readying(out, ix);
                Poll::Ready(())
            }
            Poll::Ready(None) => Poll::Pending,
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_send(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        out: Out,
        ix: Index,
    ) -> Poll<Result<(), E>> {
        let mut this = self.project();
        match this.stream.as_mut().poll_ready(cx)? {
            Poll::Ready(()) => {
                this.stream.start_send(out)?;
                *this.state = State::Started;
                _ = this
                    .done_s
                    .unbounded_send((this.key_drop.key(), ix, this.out_s.clone()));
                Poll::Ready(Ok(()))
            }
            Poll::Pending => {
                *this.state = State::Readying(out, ix);
                Poll::Pending
            }
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), E>> {
        match self.as_mut().poll_out(cx) {
            Poll::Ready(_) => Poll::Ready(Ok(())),
            Poll::Pending => {
                let this = self.project();
                match this.stream.poll_flush(cx)? {
                    Poll::Ready(()) => Poll::Ready(Ok(())),
                    Poll::Pending => {
                        *this.state = State::Started;
                        Poll::Pending
                    }
                }
            }
        }
    }

    fn pre_poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<Infallible, E>> {
        loop {
            match self.as_mut().state().take() {
                State::Flushed => ready!(self.as_mut().poll_out(cx)),
                State::Readying(out, done) => ready!(self.as_mut().poll_send(cx, out, done))?,
                State::Started => ready!(self.as_mut().poll_flush(cx))?,
            }
        }
    }

    fn poll_inner(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<In, E>>> {
        let _ = self.as_mut().pre_poll(cx)?;
        self.project().stream.poll_next(cx)
    }

    fn poll_pre_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), E>> {
        match self.as_mut().pre_poll(cx)? {
            Poll::Ready(i) => match i {},
            Poll::Pending if self.is_terminated() => Poll::Ready(Ok(())),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), E>> {
        self.project().stream.poll_close(cx)
    }

    fn is_terminated(&self) -> bool {
        self.out_r.is_terminated() && self.state.is_empty()
    }
}

impl<In, Out, E, S: Sink<Out, Error = E> + Stream<Item = Result<In, E>>, F: OnClose<E>> Stream
    for Unicast<S, Out, F>
{
    type Item = In;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.as_mut().poll_inner(cx).map(|o| match o {
            Some(Ok(out)) => Some(out),
            Some(Err(e)) => {
                self.callback.on_close(Some(e));
                None
            }
            None => {
                self.callback.on_close(None);
                None
            }
        })
    }
}

#[pin_project]
struct Finalize<S, Out, F> {
    #[pin]
    unicast: Unicast<S, Out, F>,
    closing: bool,
}
impl<
        In,
        Out: Clone,
        E,
        S: Unpin + Sink<Out, Error = E> + Stream<Item = Result<In, E>>,
        F: OnClose<E>,
    > Finalize<S, Out, F>
{
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), E>> {
        let this = self.project();
        let mut unicast = this.unicast;
        loop {
            if *this.closing {
                break unicast.poll_close(cx);
            } else {
                match unicast.as_mut().poll_pre_close(cx)? {
                    Poll::Ready(()) => *this.closing = true,
                    Poll::Pending => todo!(),
                }
            }
        }
    }
}

impl<
        In,
        Out: Clone,
        E,
        S: Unpin + Sink<Out, Error = E> + Stream<Item = Result<In, E>>,
        F: OnClose<E>,
    > Future for Finalize<S, Out, F>
{
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.as_mut().poll_close(cx).map(|r| match r {
            Ok(()) => (),
            Err(e) => self.unicast.callback.on_close(Some(e)),
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
    done_r: UnboundedReceiver<Done<Out>>,
    #[pin]
    key_r: UnboundedReceiver<Key>,
    #[pin]
    finalizing: FuturesUnordered<Finalize<S, Out, F>>,
    stale: HashMap<Key, UnboundedSender<(Out, Index)>>,
    out: Vec<Out>,
    key: usize,
    done_s: UnboundedSender<Done<Out>>,
    key_s: UnboundedSender<Key>,
    callback: F,
}

impl<
        In,
        Out: Clone,
        E,
        S: Unpin + Sink<Out, Error = E> + Stream<Item = Result<In, E>>,
        R: FusedStream<Item = S>,
        F: OnClose<E>,
    > Multicast<S, Out, F, R>
{
    fn poll_next_raw(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<In>> {
        let mut this = self.project();
        if !this.streams.is_terminated() {
            while let Poll::Ready(Some(stream)) = this.streams.as_mut().poll_next(cx) {
                *this.key += 1;
                let key = Key(*this.key);
                let (out_s, out_r) = unbounded();
                this.select.push(Unicast {
                    stream,
                    out_r,
                    state: Default::default(),
                    out_s: out_s.clone(),
                    done_s: this.done_s.clone(),
                    key_drop: KeyDropCopy::new(key, this.key_s.clone()),
                    callback: this.callback.clone(),
                });
                if let Some(out) = this.out.first() {
                    let _ = out_s.unbounded_send((out.clone(), Index(0)));
                } else {
                    this.stale.insert(key, out_s);
                }
            }
        }
        loop {
            if let Poll::Ready(Some(item)) = this.select.as_mut().poll_next(cx) {
                break Poll::Ready(Some(item));
            }
            let mut any = false;
            while let Poll::Ready(Some((key, index, out_s))) = this.done_r.as_mut().poll_next(cx) {
                any = true;
                let index = index.0 + 1;
                if let Some(out) = this.out.get(index) {
                    let _ = out_s.unbounded_send((out.clone(), Index(index)));
                } else {
                    assert!(this.stale.insert(key, out_s).is_none());
                }
            }
            while let Poll::Ready(Some(key)) = this.key_r.as_mut().poll_next(cx) {
                any = true;
                this.stale.remove(&key);
            }
            if !any {
                break if this.select.is_empty() && this.streams.is_terminated() {
                    Poll::Ready(None)
                } else {
                    Poll::Pending
                };
            }
        }
    }
}

impl<
        In,
        Out: Clone,
        E,
        S: Unpin + Sink<Out, Error = E> + Stream<Item = Result<In, E>>,
        R: FusedStream<Item = S>,
        F: OnClose<E>,
    > Stream for Multicast<S, Out, F, R>
{
    type Item = Result<In, Infallible>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.poll_next_raw(cx).map(|o| o.map(Ok))
    }
}

impl<
        In,
        Out: Clone,
        E,
        S: Unpin + Sink<Out, Error = E> + Stream<Item = Result<In, E>>,
        R: FusedStream<Item = S>,
        F: OnClose<E>,
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
        S: Unpin + Sink<Out, Error = E> + Stream<Item = Result<In, E>>,
        R: FusedStream<Item = S>,
        F: OnClose<E>,
    > Sink<Out> for Multicast<S, Out, F, R>
{
    type Error = Infallible;

    fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, item: Out) -> Result<(), Self::Error> {
        let this = self.project();
        let index = Index(this.out.len());
        for sender in this.stale.values() {
            let _ = sender.unbounded_send((item.clone(), index));
        }
        this.stale.clear();
        this.out.push(item);
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let mut this = self.project();
        this.key_s.close_channel();
        if !this.select.is_empty() {
            for unicast in std::mem::take(this.select.get_mut()) {
                this.finalizing.push(Finalize {
                    unicast,
                    closing: false,
                });
            }
        }
        loop {
            if !this.stale.is_empty() {
                for sender in std::mem::take(this.stale).into_values() {
                    sender.close_channel();
                }
            }
            while let Poll::Ready(poll) = this.finalizing.as_mut().poll_next(cx) {
                match poll {
                    Some(()) => {}
                    None => return Poll::Ready(Ok(())),
                }
            }
            let mut any = false;
            while let Poll::Ready(Some((_, index, out_s))) = this.done_r.as_mut().poll_next(cx) {
                any = true;
                let index = index.0 + 1;
                if let Some(out) = this.out.get(index) {
                    let _ = out_s.unbounded_send((out.clone(), Index(index)));
                } else {
                    out_s.close_channel();
                }
            }
            if !any {
                break Poll::Pending;
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
        R,
    > Multicast<S, Out, F, R>
{
    pub fn new(streams: R, callback: F) -> Self {
        let (done_s, done_r) = unbounded();
        let (key_s, key_r) = unbounded();
        Self {
            streams,
            select: Default::default(),
            done_r,
            key_r,
            finalizing: Default::default(),
            stale: Default::default(),
            out: Default::default(),
            key: 0,
            done_s,
            key_s,
            callback,
        }
    }
}

pub trait MulticastReplay<Out>: Sized {
    type S;

    type E;

    fn multicast_replay<F: OnClose<Self::E>>(self, callback: F)
        -> Multicast<Self::S, Out, F, Self>;
}

impl<
        In,
        Out: Clone,
        E,
        S: Unpin + Stream<Item = Result<In, E>> + Sink<Out, Error = E>,
        R: Stream<Item = S>,
    > MulticastReplay<Out> for R
{
    type S = S;

    type E = E;

    fn multicast_replay<F: OnClose<Self::E>>(
        self,
        callback: F,
    ) -> Multicast<Self::S, Out, F, Self> {
        Multicast::new(self, callback)
    }
}

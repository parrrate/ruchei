//! Replays all sent events on new connection.
//!
//! ```rust
//! # use async_net::TcpListener;
//! # use futures_util::StreamExt;
//! # use ruchei::concurrent::ConcurrentExt;
//! # use ruchei::poll_on_wake::PollOnWakeExt;
//! use ruchei::multicast::replay::MulticastReplay;
//!
//! # async fn __() {
//! TcpListener::bind("127.0.0.1:8080")
//!     .await
//!     .unwrap()
//!     .incoming()
//!     .poll_on_wake()
//!     .filter_map(|r| async { r.ok() })
//!     .map(async_tungstenite::accept_async)
//!     .fuse()
//!     .concurrent()
//!     .filter_map(|r| async { r.ok() })
//!     .multicast_replay(|_| {}); // ignore errors
//! # }
//! ```

use std::{
    collections::HashMap,
    convert::Infallible,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker},
};

use futures_util::{
    Future, Sink, Stream, TryStream,
    lock::Mutex,
    ready,
    stream::{FusedStream, FuturesUnordered, SelectAll},
};
use pin_project::pin_project;

use crate::{
    callback::OnClose,
    pinned_extend::{AutoPinnedExtend, Extending, ExtendingExt},
};

struct Shared<Out> {
    stale: HashMap<Key, Waker>,
    fstale: HashMap<Key, Waker>,
    out: Vec<Out>,
    flush: usize,
}

impl<Out> Default for Shared<Out> {
    fn default() -> Self {
        Self {
            stale: Default::default(),
            fstale: Default::default(),
            out: Default::default(),
            flush: 0,
        }
    }
}

#[derive(PartialEq, Eq, Hash, Clone, Copy, Debug)]
struct Key(usize);

#[derive(Default, Debug)]
enum State<Out> {
    #[default]
    Flushed,
    Readying(Out),
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

#[derive(Debug)]
#[pin_project]
struct Unicast<S, Out, F> {
    #[pin]
    stream: S,
    state: State<Out>,
    shared: Arc<Mutex<Shared<Out>>>,
    index: usize,
    flushed: usize,
    stale: bool,
    key: Key,
    callback: F,
}

impl<In, Out: Clone, E, S: Sink<Out, Error = E> + TryStream<Ok = In, Error = E>, F: OnClose<E>>
    Unicast<S, Out, F>
{
    fn state(self: Pin<&mut Self>) -> &mut State<Out> {
        self.project().state
    }

    fn poll_out(self: Pin<&mut Self>, cx: &mut Context<'_>, shared: &mut Shared<Out>) -> Poll<()> {
        let this = self.project();
        if let Some(out) = shared.out.get(*this.index) {
            *this.state = State::Readying(out.clone());
            *this.index += 1;
            Poll::Ready(())
        } else {
            shared.stale.insert(*this.key, cx.waker().clone());
            *this.stale = true;
            Poll::Pending
        }
    }

    fn poll_send(self: Pin<&mut Self>, cx: &mut Context<'_>, out: Out) -> Poll<Result<(), E>> {
        let mut this = self.project();
        match this.stream.as_mut().poll_ready(cx)? {
            Poll::Ready(()) => {
                this.stream.start_send(out)?;
                *this.state = State::Started;
                Poll::Ready(Ok(()))
            }
            Poll::Pending => {
                *this.state = State::Readying(out);
                Poll::Pending
            }
        }
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        shared: &mut Shared<Out>,
    ) -> Poll<Result<(), E>> {
        match self.as_mut().poll_out(cx, shared) {
            Poll::Ready(()) => Poll::Ready(Ok(())),
            Poll::Pending => {
                let this = self.project();
                if *this.flushed < shared.flush {
                    match this.stream.poll_flush(cx)? {
                        Poll::Ready(()) => {
                            *this.flushed = *this.index;
                            Poll::Pending
                        }
                        Poll::Pending => {
                            *this.state = State::Started;
                            Poll::Pending
                        }
                    }
                } else {
                    shared.fstale.insert(*this.key, cx.waker().clone());
                    *this.state = State::Started;
                    Poll::Pending
                }
            }
        }
    }

    fn pre_poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<Infallible, E>> {
        *self.as_mut().project().stale = false;
        let mut shared = None;
        loop {
            match self.as_mut().state().take() {
                State::Flushed => {
                    let shared = shared.get_or_insert_with(|| {
                        self.as_mut()
                            .project()
                            .shared
                            .clone()
                            .try_lock_owned()
                            .unwrap()
                    });
                    ready!(self.as_mut().poll_out(cx, shared))
                }
                State::Readying(out) => ready!(self.as_mut().poll_send(cx, out))?,
                State::Started => {
                    let shared = shared.get_or_insert_with(|| {
                        self.as_mut()
                            .project()
                            .shared
                            .clone()
                            .try_lock_owned()
                            .unwrap()
                    });
                    ready!(self.as_mut().poll_flush(cx, shared))?
                }
            }
        }
    }

    fn poll_inner(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<In, E>>> {
        let _ = self.as_mut().pre_poll(cx)?;
        self.project().stream.try_poll_next(cx)
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
        self.stale && self.state.is_empty()
    }

    fn on_close(&self, error: Option<E>) {
        let mut shared = self.shared.try_lock().unwrap();
        shared.stale.remove(&self.key);
        shared.fstale.remove(&self.key);
        self.callback.on_close(error);
    }
}

impl<In, Out: Clone, E, S: Sink<Out, Error = E> + TryStream<Ok = In, Error = E>, F: OnClose<E>>
    Stream for Unicast<S, Out, F>
{
    type Item = In;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.as_mut().poll_inner(cx).map(|o| match o {
            Some(Ok(out)) => Some(out),
            Some(Err(e)) => {
                self.on_close(Some(e));
                None
            }
            None => {
                self.on_close(None);
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
    S: Unpin + Sink<Out, Error = E> + TryStream<Ok = In, Error = E>,
    F: OnClose<E>,
> Finalize<S, Out, F>
{
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), E>> {
        let this = self.project();
        let mut unicast = this.unicast;
        loop {
            if *this.closing {
                break unicast.poll_close(cx);
            }
            ready!(unicast.as_mut().poll_pre_close(cx))?;
            *this.closing = true;
        }
    }
}

impl<
    In,
    Out: Clone,
    E,
    S: Unpin + Sink<Out, Error = E> + TryStream<Ok = In, Error = E>,
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

#[derive(Debug)]
#[pin_project]
pub struct Multicast<S, Out, F> {
    #[pin]
    select: SelectAll<Unicast<S, Out, F>>,
    #[pin]
    finalizing: FuturesUnordered<Finalize<S, Out, F>>,
    shared: Arc<Mutex<Shared<Out>>>,
    key: usize,
    callback: F,
}

impl<
    In,
    Out: Clone,
    E,
    S: Unpin + Sink<Out, Error = E> + TryStream<Ok = In, Error = E>,
    F: OnClose<E>,
> Multicast<S, Out, F>
{
    fn poll_next_raw(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<In>> {
        self.project().select.poll_next(cx)
    }
}

impl<
    In,
    Out: Clone,
    E,
    S: Unpin + Sink<Out, Error = E> + TryStream<Ok = In, Error = E>,
    F: OnClose<E>,
> Stream for Multicast<S, Out, F>
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
    S: Unpin + Sink<Out, Error = E> + TryStream<Ok = In, Error = E>,
    F: OnClose<E>,
> FusedStream for Multicast<S, Out, F>
{
    fn is_terminated(&self) -> bool {
        self.select.is_terminated()
    }
}

impl<
    In,
    Out: Clone,
    E,
    S: Unpin + Sink<Out, Error = E> + TryStream<Ok = In, Error = E>,
    F: OnClose<E>,
> Sink<Out> for Multicast<S, Out, F>
{
    type Error = Infallible;

    fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, item: Out) -> Result<(), Self::Error> {
        let this = self.project();
        let mut shared = this.shared.try_lock().unwrap();
        shared.out.push(item);
        for (_, waker) in shared.stale.drain() {
            waker.wake();
        }
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.project();
        let mut shared = this.shared.try_lock().unwrap();
        if shared.flush < shared.out.len() {
            shared.flush = shared.out.len();
            for (_, waker) in shared.fstale.drain() {
                waker.wake();
            }
        }
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let mut this = self.project();
        if !this.select.is_empty() {
            for unicast in std::mem::take(this.select.get_mut()) {
                this.finalizing.push(Finalize {
                    unicast,
                    closing: false,
                });
            }
        }
        for (_, waker) in this.shared.try_lock().unwrap().stale.drain() {
            waker.wake();
        }
        while let Poll::Ready(poll) = this.finalizing.as_mut().poll_next(cx) {
            match poll {
                Some(()) => {}
                None => return Poll::Ready(Ok(())),
            }
        }
        Poll::Pending
    }
}

impl<
    In,
    Out: Clone,
    E,
    S: Unpin + TryStream<Ok = In, Error = E> + Sink<Out, Error = E>,
    F: OnClose<E>,
> Multicast<S, Out, F>
{
    #[must_use]
    pub fn new(callback: F) -> Self {
        Self {
            select: Default::default(),
            finalizing: Default::default(),
            shared: Default::default(),
            key: 0,
            callback,
        }
    }

    pub fn push(&mut self, stream: S) {
        self.key += 1;
        let key = Key(self.key);
        self.select.push(Unicast {
            stream,
            state: Default::default(),
            shared: self.shared.clone(),
            index: 0,
            flushed: 0,
            stale: false,
            key,
            callback: self.callback.clone(),
        });
    }
}

impl<
    In,
    Out: Clone,
    E,
    S: Unpin + TryStream<Ok = In, Error = E> + Sink<Out, Error = E>,
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
    S: Unpin + TryStream<Ok = In, Error = E> + Sink<Out, Error = E>,
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
    S: Unpin + TryStream<Ok = In, Error = E> + Sink<Out, Error = E>,
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
    S: Unpin + TryStream<Ok = In, Error = E> + Sink<Out, Error = E>,
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

pub trait MulticastReplay<Out>: Sized {
    /// Single [`Stream`]/[`Sink`].
    type S;
    /// Error.
    type E;

    #[must_use]
    fn multicast_replay<F: OnClose<Self::E>>(
        self,
        callback: F,
    ) -> Extending<Multicast<Self::S, Out, F>, Self>;
}

impl<
    In,
    Out: Clone,
    E,
    S: Unpin + TryStream<Ok = In, Error = E> + Sink<Out, Error = E>,
    R: FusedStream<Item = S>,
> MulticastReplay<Out> for R
{
    type S = S;

    type E = E;

    fn multicast_replay<F: OnClose<Self::E>>(
        self,
        callback: F,
    ) -> Extending<Multicast<Self::S, Out, F>, Self> {
        self.extending(Multicast::new(callback))
    }
}

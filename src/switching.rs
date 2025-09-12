//! [`Stream`]`+`[`Sink`] from a [`Stream`] of [`Stream`]`+`[`Sink`]s switching to a new one as soon
//! as it becomes available.
//!
//! ```rust
//! # use async_net::TcpListener;
//! # use futures_util::StreamExt;
//! # use ruchei::concurrent::ConcurrentExt;
//! # use ruchei::poll_on_wake::PollOnWakeExt;
//! use ruchei::switching::SwitchingExt;
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
//!     .switching();
//! # }
//! ```

use std::{
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{Sink, Stream, TryStream, ready, stream::FusedStream, task::AtomicWaker};
use pin_project::pin_project;

/// When a new [`Stream`]`+`[`Sink`] becomes available, closes the existing one, then switches to a
/// next one.
///
/// Closes when the incoming stream is done.
#[pin_project]
pub struct Switching<R, S, Out> {
    #[pin]
    incoming: R,
    #[pin]
    current: Option<S>,
    swap: Option<S>,
    waker_next: AtomicWaker,
    waker_ready: AtomicWaker,
    waker_flush: AtomicWaker,
    _out: PhantomData<Out>,
}

impl<In, Out, E, S: TryStream<Ok = In, Error = E> + Sink<Out, Error = E>, R: FusedStream<Item = S>>
    Switching<R, S, Out>
{
    fn ensure_current(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), E>> {
        let mut this = self.project();
        loop {
            match this.swap {
                Some(_) => {
                    if let Some(current) = this.current.as_mut().as_pin_mut() {
                        ready!(current.poll_close(cx))?;
                    }
                    this.waker_next.wake();
                    this.waker_ready.wake();
                    this.waker_flush.wake();
                    this.current.as_mut().set(this.swap.take());
                }
                None => {
                    if !this.incoming.is_terminated()
                        && let Poll::Ready(Some(swap)) = this.incoming.as_mut().poll_next(cx)
                    {
                        *this.swap = Some(swap);
                        continue;
                    }
                    break Poll::Ready(Ok(()));
                }
            }
        }
    }
}

impl<In, Out, E, S: TryStream<Ok = In, Error = E> + Sink<Out, Error = E>, R: FusedStream<Item = S>>
    Stream for Switching<R, S, Out>
{
    type Item = Result<In, E>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        ready!(self.as_mut().ensure_current(cx))?;
        let mut this = self.project();
        this.waker_next.register(cx.waker());
        if let Some(current) = this.current.as_mut().as_pin_mut() {
            if let Some(r) = ready!(current.try_poll_next(cx)) {
                return Poll::Ready(Some(r));
            }
            this.current.as_mut().set(None)
        }
        if this.incoming.is_terminated() {
            Poll::Ready(None)
        } else {
            Poll::Pending
        }
    }
}

impl<In, Out, E, S: TryStream<Ok = In, Error = E> + Sink<Out, Error = E>, R: FusedStream<Item = S>>
    FusedStream for Switching<R, S, Out>
{
    fn is_terminated(&self) -> bool {
        self.current.is_none() && self.swap.is_none() && self.incoming.is_terminated()
    }
}

impl<In, Out, E, S: TryStream<Ok = In, Error = E> + Sink<Out, Error = E>, R: FusedStream<Item = S>>
    Sink<Out> for Switching<R, S, Out>
{
    type Error = E;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        ready!(self.as_mut().ensure_current(cx))?;
        let this = self.project();
        this.waker_ready.register(cx.waker());
        match this.current.as_pin_mut() {
            Some(current) => current.poll_ready(cx),
            None => Poll::Ready(Ok(())),
        }
    }

    fn start_send(self: Pin<&mut Self>, item: Out) -> Result<(), Self::Error> {
        let mut this = self.project();
        match this.current.as_mut().as_pin_mut() {
            Some(current) => current.start_send(item),
            None => Ok(()),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        ready!(self.as_mut().ensure_current(cx))?;
        let this = self.project();
        this.waker_flush.register(cx.waker());
        match this.current.as_pin_mut() {
            Some(current) => current.poll_flush(cx),
            None => Poll::Ready(Ok(())),
        }
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        ready!(self.as_mut().ensure_current(cx))?;
        let mut this = self.project();
        if let Some(current) = this.current.as_mut().as_pin_mut() {
            ready!(current.poll_close(cx))?;
            this.current.as_mut().set(None)
        }
        if this.incoming.is_terminated() {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }
}

/// Extension trait for constructing [`Switching`].
pub trait SwitchingExt<Out>: Sized {
    /// [`Stream`]`+`[`Sink`] that this [`Stream`] yields.
    type S;

    /// Convert a [`Stream`] of [`Stream`]`+`[`Sink`]s into a single [`Stream`]`+`[`Sink`].
    ///
    /// See [`Switching`] for details.
    #[must_use]
    fn switching(self) -> Switching<Self, Self::S, Out>;
}

impl<In, Out, E, S: TryStream<Ok = In, Error = E> + Sink<Out, Error = E>, R: FusedStream<Item = S>>
    SwitchingExt<Out> for R
{
    type S = S;

    fn switching(self) -> Switching<Self, Self::S, Out> {
        Switching {
            incoming: self,
            current: None,
            swap: None,
            waker_next: Default::default(),
            waker_ready: Default::default(),
            waker_flush: Default::default(),
            _out: PhantomData,
        }
    }
}

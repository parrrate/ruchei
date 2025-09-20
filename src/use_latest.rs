//! [`Stream`]`+`[`Sink`] from a [`Stream`] of [`Stream`]`+`[`Sink`]s switching to a new one as soon
//! as it becomes available.
//!
//! ```rust
//! # use async_net::TcpListener;
//! # use futures_util::StreamExt;
//! # use ruchei::concurrent::ConcurrentExt;
//! # use ruchei::poll_on_wake::PollOnWakeExt;
//! use ruchei::use_latest::UseLatestExt;
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
//!     .use_latest();
//! # }
//! ```

use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Wake},
};

use futures_util::{Sink, Stream, TryStream, ready, stream::FusedStream, task::AtomicWaker};
use pin_project::pin_project;

#[derive(Debug, Default)]
struct Wakers {
    next: AtomicWaker,
    ready: AtomicWaker,
    flush: AtomicWaker,
}

impl Wake for Wakers {
    fn wake(self: Arc<Self>) {
        self.next.wake();
        self.ready.wake();
        self.flush.wake();
    }
}

/// When a new [`Stream`]`+`[`Sink`] becomes available, closes the existing one, then switches to a
/// next one.
///
/// Closes when the incoming stream is done.
#[pin_project]
#[derive(Debug)]
pub struct UseLatest<R, Out, S = <R as Stream>::Item> {
    #[pin]
    incoming: R,
    #[pin]
    current: Option<S>,
    swap: Option<S>,
    w: Arc<Wakers>,
    buffer: Option<Out>,
}

impl<In, Out, E, S: TryStream<Ok = In, Error = E> + Sink<Out, Error = E>, R: FusedStream<Item = S>>
    UseLatest<R, Out, S>
{
    fn poll_current(self: Pin<&mut Self>) -> Poll<Result<(), E>> {
        let mut this = self.project();
        let waker = this.w.clone().into();
        let cx = &mut Context::from_waker(&waker);
        loop {
            match this.swap {
                Some(_) => {
                    if let Some(current) = this.current.as_mut().as_pin_mut() {
                        ready!(current.poll_close(cx))?;
                    }
                    this.current.as_mut().set(this.swap.take());
                    this.w.wake_by_ref();
                }
                None => {
                    if !this.incoming.is_terminated()
                        && let Poll::Ready(Some(swap)) = this.incoming.as_mut().poll_next(cx)
                    {
                        *this.swap = Some(swap);
                        this.w.wake_by_ref();
                        continue;
                    }
                    break Poll::Ready(Ok(()));
                }
            }
        }
    }

    fn poll_buffer(self: Pin<&mut Self>) -> Poll<Result<(), E>> {
        let mut this = self.project();
        let waker = this.w.clone().into();
        let cx = &mut Context::from_waker(&waker);
        if this.buffer.is_some() {
            if let Some(current) = this.current.as_mut().as_pin_mut() {
                ready!(current.poll_ready(cx))?;
            } else {
                return Poll::Pending;
            }
        }
        if let Some(item) = this.buffer.take()
            && let Some(current) = this.current.as_mut().as_pin_mut()
        {
            current.start_send(item)?;
        }
        Poll::Ready(Ok(()))
    }
}

impl<In, Out, E, S: TryStream<Ok = In, Error = E> + Sink<Out, Error = E>, R: FusedStream<Item = S>>
    Stream for UseLatest<R, Out, S>
{
    type Item = Result<In, E>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.w.next.register(cx.waker());
        ready!(self.as_mut().poll_current())?;
        let _ = self.as_mut().poll_buffer()?;
        let mut this = self.project();
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
    FusedStream for UseLatest<R, Out, S>
{
    fn is_terminated(&self) -> bool {
        self.current.is_none() && self.swap.is_none() && self.incoming.is_terminated()
    }
}

impl<In, Out, E, S: TryStream<Ok = In, Error = E> + Sink<Out, Error = E>, R: FusedStream<Item = S>>
    Sink<Out> for UseLatest<R, Out, S>
{
    type Error = E;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.w.ready.register(cx.waker());
        ready!(self.as_mut().poll_current())?;
        self.as_mut().poll_buffer()
    }

    fn start_send(self: Pin<&mut Self>, item: Out) -> Result<(), Self::Error> {
        let this = self.project();
        assert!(this.buffer.is_none());
        *this.buffer = Some(item);
        Ok(())
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.w.flush.register(cx.waker());
        ready!(self.as_mut().poll_current())?;
        ready!(self.as_mut().poll_buffer())?;
        let this = self.project();
        match this.current.as_pin_mut() {
            Some(current) => current.poll_flush(cx),
            None => Poll::Ready(Ok(())),
        }
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        ready!(self.as_mut().poll_current())?;
        ready!(self.as_mut().poll_buffer())?;
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

impl<R, Out, S> From<R> for UseLatest<R, Out, S> {
    fn from(incoming: R) -> Self {
        UseLatest {
            incoming,
            current: None,
            swap: None,
            w: Default::default(),
            buffer: None,
        }
    }
}

/// Extension trait for constructing [`UseLatest`].
pub trait UseLatestExt<Out>:
    Sized + FusedStream<Item: TryStream<Error = Self::E> + Sink<Out, Error = Self::E>>
{
    type E;

    /// Convert a [`Stream`] of [`Stream`]`+`[`Sink`]s into a single [`Stream`]`+`[`Sink`].
    ///
    /// See [`UseLatest`] for details.
    #[must_use]
    fn use_latest(self) -> UseLatest<Self, Out> {
        self.into()
    }
}

impl<In, Out, E, S: TryStream<Ok = In, Error = E> + Sink<Out, Error = E>, R: FusedStream<Item = S>>
    UseLatestExt<Out> for R
{
    type E = E;
}

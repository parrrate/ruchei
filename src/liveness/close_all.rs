use std::{
    fmt::Debug,
    marker::PhantomData,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use futures_util::{
    Future, Sink, Stream, TryStream,
    future::FusedFuture,
    lock::{Mutex, OwnedMutexGuard, OwnedMutexLockFuture},
    ready,
    stream::FusedStream,
};
use pin_project::pin_project;

#[derive(Debug)]
struct Closed;

/// Yielded by [`CloseAll`]. Gets closed when incoming stream terminates.
#[pin_project]
pub struct CloseOne<S, Out> {
    #[pin]
    stream: S,
    #[pin]
    closing: OwnedMutexLockFuture<Closed>,
    terminated: bool,
    _out: PhantomData<Out>,
}

impl<S: Debug, Out> Debug for CloseOne<S, Out> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CloseOne")
            .field("stream", &self.stream)
            .field("closing", &self.closing)
            .field("terminated", &self.terminated)
            .field("_out", &self._out)
            .finish()
    }
}

impl<In, Out, E, S: TryStream<Ok = In, Error = E> + Sink<Out, Error = E>> Stream
    for CloseOne<S, Out>
{
    type Item = Result<In, E>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        if *this.terminated {
            Poll::Ready(None)
        } else if this.closing.is_terminated() || this.closing.poll(cx).is_ready() {
            let r = ready!(this.stream.poll_close(cx));
            *this.terminated = true;
            r?;
            Poll::Ready(None)
        } else {
            Poll::Ready(match ready!(this.stream.try_poll_next(cx)) {
                Some(Ok(item)) => Some(Ok(item)),
                Some(Err(e)) => {
                    *this.terminated = true;
                    Some(Err(e))
                }
                None => {
                    *this.terminated = true;
                    None
                }
            })
        }
    }
}

impl<In, Out, E, S: TryStream<Ok = In, Error = E> + Sink<Out, Error = E>> FusedStream
    for CloseOne<S, Out>
{
    fn is_terminated(&self) -> bool {
        self.terminated
    }
}

impl<In, Out, E, S: TryStream<Ok = In, Error = E> + Sink<Out, Error = E>> Sink<Out>
    for CloseOne<S, Out>
{
    type Error = E;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.project();
        if this.closing.is_terminated() {
            Poll::Pending
        } else {
            this.stream.poll_ready(cx)
        }
    }

    fn start_send(self: Pin<&mut Self>, item: Out) -> Result<(), Self::Error> {
        let this = self.project();
        if this.closing.is_terminated() {
            Ok(())
        } else {
            this.stream.start_send(item)
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.project();
        if this.closing.is_terminated() {
            Poll::Pending
        } else {
            this.stream.poll_flush(cx)
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.project();
        if *this.terminated {
            Poll::Ready(Ok(()))
        } else {
            let r = ready!(this.stream.poll_close(cx));
            *this.terminated = true;
            Poll::Ready(r)
        }
    }
}

/// Closes all yielded streams ([`CloseOne`]s) on termination of incoming stream.
#[pin_project]
pub struct CloseAll<R, Out> {
    #[pin]
    stream: R,
    guard: Option<OwnedMutexGuard<Closed>>,
    lock: Arc<Mutex<Closed>>,
    _out: PhantomData<Out>,
}

impl<R: Debug, Out> Debug for CloseAll<R, Out> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CloseAll")
            .field("stream", &self.stream)
            .field("guard", &self.guard)
            .field("lock", &self.lock)
            .field("_out", &self._out)
            .finish()
    }
}

impl<Out, S, R: Stream<Item = S>> Stream for CloseAll<R, Out> {
    type Item = CloseOne<S, Out>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        Poll::Ready(match ready!(this.stream.poll_next(cx)) {
            Some(stream) => Some(CloseOne {
                stream,
                closing: this.lock.clone().lock_owned(),
                terminated: false,
                _out: PhantomData,
            }),
            None => {
                this.guard.take();
                None
            }
        })
    }
}

impl<Out, S, R: Stream<Item = S>> FusedStream for CloseAll<R, Out> {
    fn is_terminated(&self) -> bool {
        self.guard.is_none()
    }
}

impl<Out, S: Sink<Out>, R: Stream<Item = S>> From<R> for CloseAll<R, Out> {
    fn from(stream: R) -> Self {
        let lock = Arc::new(Mutex::new(Closed));
        let guard = lock.clone().try_lock_owned().unwrap();
        Self {
            stream,
            guard: Some(guard),
            lock,
            _out: PhantomData,
        }
    }
}

pub trait CloseAllExt<Out>: Sized + Stream<Item: Sink<Out>> {
    #[must_use]
    fn close_all(self) -> CloseAll<Self, Out> {
        self.into()
    }
}

impl<Out, S: Sink<Out>, R: Stream<Item = S>> CloseAllExt<Out> for R {}

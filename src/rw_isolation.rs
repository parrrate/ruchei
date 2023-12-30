use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker},
};

use futures_util::{
    lock::{Mutex, OwnedMutexGuard, OwnedMutexLockFuture},
    ready,
    stream::FusedStream,
    Future, Sink, Stream,
};
use pin_project::pin_project;

struct AllowRead;

#[pin_project]
pub struct RwInner<S> {
    #[pin]
    stream: S,
    #[pin]
    read_future: OwnedMutexLockFuture<AllowRead>,
    read_mutex: Arc<Mutex<AllowRead>>,
}

impl<S: Stream> Stream for RwInner<S> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        ready!(this.read_future.as_mut().poll(cx));
        *this.read_future = this.read_mutex.clone().lock_owned();
        this.stream.poll_next(cx)
    }
}

impl<S: FusedStream> FusedStream for RwInner<S> {
    fn is_terminated(&self) -> bool {
        self.stream.is_terminated()
    }
}

impl<Out, S: Sink<Out>> Sink<Out> for RwInner<S> {
    type Error = S::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().stream.poll_ready(cx)
    }

    fn start_send(self: Pin<&mut Self>, item: Out) -> Result<(), Self::Error> {
        self.project().stream.start_send(item)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().stream.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().stream.poll_close(cx)
    }
}

#[pin_project]
pub struct RwOuter<S> {
    #[pin]
    stream: S,
    read_waker: Option<Waker>,
    read_guard: Option<OwnedMutexGuard<AllowRead>>,
    read_mutex: Arc<Mutex<AllowRead>>,
}

impl<S: Stream> Stream for RwOuter<S> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        this.read_guard.take();
        match this.stream.poll_next(cx) {
            poll @ Poll::Ready(_) => poll,
            Poll::Pending => {
                *this.read_waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

impl<S: FusedStream> FusedStream for RwOuter<S> {
    fn is_terminated(&self) -> bool {
        self.stream.is_terminated()
    }
}

impl<S: Stream> RwOuter<S> {
    fn poll_inner(self: Pin<&mut Self>, cx: &mut Context<'_>) {
        let this = self.project();
        if this.read_guard.is_none() {
            *this.read_guard = Some(this.read_mutex.clone().try_lock_owned().unwrap());
        }
        if let Poll::Ready(Some(_)) = this.stream.poll_next(cx) {
            panic!("item in write context")
        }
        if let Some(waker) = this.read_waker.take() {
            waker.wake();
        }
    }
}

impl<Out, S: FusedStream + Sink<Out>> Sink<Out> for RwOuter<S> {
    type Error = S::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.stream.is_terminated() {
            return Poll::Pending;
        }
        self.as_mut().poll_inner(cx);
        self.project().stream.poll_ready(cx)
    }

    fn start_send(self: Pin<&mut Self>, item: Out) -> Result<(), Self::Error> {
        self.project().stream.start_send(item)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.stream.is_terminated() {
            return Poll::Pending;
        }
        self.as_mut().poll_inner(cx);
        self.project().stream.poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.stream.is_terminated() {
            return Poll::Ready(Ok(()));
        }
        self.as_mut().poll_inner(cx);
        self.project().stream.poll_close(cx)
    }
}

#[derive(Clone)]
pub struct CtxInner(Arc<Mutex<AllowRead>>);

pub struct CtxOuter(Arc<Mutex<AllowRead>>);

pub fn isolation() -> (CtxInner, CtxOuter) {
    let mutex = Arc::new(Mutex::new(AllowRead));
    (CtxInner(mutex.clone()), CtxOuter(mutex))
}

pub trait IsolateInner: Sized {
    fn isolate_inner(self, inner: CtxInner) -> RwInner<Self>;
}

impl<S> IsolateInner for S {
    fn isolate_inner(self, inner: CtxInner) -> RwInner<Self> {
        RwInner {
            stream: self,
            read_future: inner.0.clone().lock_owned(),
            read_mutex: inner.0,
        }
    }
}

pub trait IsolateOuter: Sized {
    fn isolate_outer(self, outer: CtxOuter) -> RwOuter<Self>;
}

impl<S> IsolateOuter for S {
    fn isolate_outer(self, outer: CtxOuter) -> RwOuter<Self> {
        RwOuter {
            stream: self,
            read_waker: None,
            read_guard: None,
            read_mutex: outer.0,
        }
    }
}

use std::{
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Poll},
};

use futures_util::{
    future::FusedFuture,
    stream::FusedStream,
    task::{waker, ArcWake, AtomicWaker},
    Future, Sink, Stream,
};
use pin_project::pin_project;

#[pin_project]
pub struct PollOnWake<S> {
    #[pin]
    inner: S,
    future: Arc<SetFlag>,
    next: Arc<SetFlag>,
    ready: Arc<SetFlag>,
    flush: Arc<SetFlag>,
    close: Arc<SetFlag>,
}

struct SetFlag {
    flag: AtomicBool,
    waker: AtomicWaker,
}

impl Default for SetFlag {
    fn default() -> Self {
        Self {
            flag: AtomicBool::new(true),
            waker: Default::default(),
        }
    }
}

impl ArcWake for SetFlag {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        arc_self.flag.store(true, Ordering::Release);
        arc_self.waker.wake()
    }
}

impl SetFlag {
    fn poll<T>(
        self: &Arc<Self>,
        cx: &mut Context,
        f: impl FnOnce(&mut Context) -> Poll<T>,
    ) -> Poll<T> {
        self.waker.register(cx.waker());
        if self.flag.swap(false, Ordering::AcqRel) {
            match f(&mut Context::from_waker(&waker(self.clone()))) {
                poll @ Poll::Ready(_) => {
                    self.flag.store(true, Ordering::Release);
                    poll
                }
                Poll::Pending => Poll::Pending,
            }
        } else {
            Poll::Pending
        }
    }
}

impl<S: Future> Future for PollOnWake<S> {
    type Output = S::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        this.future.poll(cx, |cx| this.inner.poll(cx))
    }
}

impl<S: FusedFuture> FusedFuture for PollOnWake<S> {
    fn is_terminated(&self) -> bool {
        self.inner.is_terminated()
    }
}

impl<S: Stream> Stream for PollOnWake<S> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        this.next.poll(cx, |cx| this.inner.poll_next(cx))
    }
}

impl<S: FusedStream> FusedStream for PollOnWake<S> {
    fn is_terminated(&self) -> bool {
        self.inner.is_terminated()
    }
}

impl<Item, S: Sink<Item>> Sink<Item> for PollOnWake<S> {
    type Error = S::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.project();
        this.ready.poll(cx, |cx| this.inner.poll_ready(cx))
    }

    fn start_send(self: Pin<&mut Self>, item: Item) -> Result<(), Self::Error> {
        self.project().inner.start_send(item)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.project();
        this.flush.poll(cx, |cx| this.inner.poll_flush(cx))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.project();
        this.close.poll(cx, |cx| this.inner.poll_close(cx))
    }
}

impl<S> AsRef<S> for PollOnWake<S> {
    fn as_ref(&self) -> &S {
        &self.inner
    }
}

impl<S> AsMut<S> for PollOnWake<S> {
    fn as_mut(&mut self) -> &mut S {
        &mut self.inner
    }
}

impl<S> From<S> for PollOnWake<S> {
    fn from(inner: S) -> Self {
        Self {
            inner,
            future: Default::default(),
            next: Default::default(),
            ready: Default::default(),
            flush: Default::default(),
            close: Default::default(),
        }
    }
}

impl<S> PollOnWake<S> {
    pub fn into_inner(self) -> S {
        self.inner
    }
}

pub trait PollOnWakeExt: Sized {
    fn poll_on_wake(self) -> PollOnWake<Self>;
}

impl<S> PollOnWakeExt for S {
    fn poll_on_wake(self) -> PollOnWake<Self> {
        self.into()
    }
}

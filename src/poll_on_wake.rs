//! Prevent polling until the wake up.
//!
//!
//! ```rust
//! # use async_net::TcpListener;
//! use ruchei::poll_on_wake::PollOnWakeExt;
//!
//! # async fn __() {
//! TcpListener::bind("127.0.0.1:8080")
//!     .await
//!     .unwrap()
//!     .incoming() // tends to be CPU-intensive on excessive poll
//!     .poll_on_wake();
//! # }
//! ```

use std::{
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Poll, Wake},
};

use futures_util::{
    future::{FusedFuture, Future},
    sink::Sink,
    stream::{FusedStream, Stream},
    task::AtomicWaker,
};
use pin_project::pin_project;

/// Adapter to reduce polling of [`Future`]s, [`Stream`]s and [`Sink`]s.
///
/// See [module-level docs](`crate::poll_on_wake`) for examples.
#[derive(Debug, Default)]
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

#[derive(Debug)]
#[must_use]
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

impl Wake for SetFlag {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref()
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.flag.store(true, Ordering::Release);
        self.waker.wake()
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
            match f(&mut Context::from_waker(&self.clone().into())) {
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
    #[must_use]
    fn as_ref(&self) -> &S {
        &self.inner
    }
}

impl<S> AsMut<S> for PollOnWake<S> {
    #[must_use]
    fn as_mut(&mut self) -> &mut S {
        &mut self.inner
    }
}

impl<S> From<S> for PollOnWake<S> {
    #[must_use]
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
    #[must_use]
    pub fn into_inner(self) -> S {
        self.inner
    }
}

/// Extension trait combinator to reduce polling of [`Future`]s, [`Stream`]s and [`Sink`]s.
///
/// See [module-level docs](`crate::poll_on_wake`) for examples.
pub trait PollOnWakeExt: Sized {
    #[must_use]
    fn poll_on_wake(self) -> PollOnWake<Self>;
}

impl<S> PollOnWakeExt for S {
    #[must_use]
    fn poll_on_wake(self) -> PollOnWake<Self> {
        self.into()
    }
}

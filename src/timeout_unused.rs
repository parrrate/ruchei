//! Close [`Stream`] on timer running out.
//!
//! Starts the timer when all [`KeepAlive`]s are dropped, which includes that upon startup.

use std::{
    pin::Pin,
    sync::{Arc, Weak},
    task::{Context, Poll},
};

use futures_util::{
    future::Future,
    lock::{Mutex, OwnedMutexGuard, OwnedMutexLockFuture},
    ready,
    stream::{FusedStream, Stream},
};
use pin_project::pin_project;
use ruchei_extra::WithExtra;

use crate::callback::Start;

#[derive(Debug)]
#[must_use]
struct Alive;

/// Handle indicating the value from [`WithTimeout`] is used.
#[derive(Debug, Clone)]
pub struct KeepAlive {
    _locked: Arc<OwnedMutexGuard<Alive>>,
}

/// [`Stream`] closing on timeout.
#[derive(Debug)]
#[pin_project]
pub struct WithTimeout<S, Fut, F> {
    #[pin]
    stream: S,
    #[pin]
    timeout: Option<Fut>,
    start: F,
    #[pin]
    locking: OwnedMutexLockFuture<Alive>,
    locked: Weak<OwnedMutexGuard<Alive>>,
    mutex: Arc<Mutex<Alive>>,
    done: bool,
}

impl<S, Fut, F> WithTimeout<S, Fut, F> {
    /// Pinned mutable reference to the inner stream.
    #[must_use]
    pub fn as_pin_mut(self: Pin<&mut Self>) -> Pin<&mut S> {
        self.project().stream
    }

    /// Shared reference to the timer future factory. **Does not start the timer.**
    #[must_use]
    pub fn start(&self) -> &F {
        &self.start
    }

    /// Mutable reference to the timer future factory. **Does not start the timer.**
    #[must_use]
    pub fn start_mut(&mut self) -> &mut F {
        &mut self.start
    }
}

impl<S, Fut, F> AsRef<S> for WithTimeout<S, Fut, F> {
    #[must_use]
    fn as_ref(&self) -> &S {
        &self.stream
    }
}

impl<S, Fut, F> AsMut<S> for WithTimeout<S, Fut, F> {
    #[must_use]
    fn as_mut(&mut self) -> &mut S {
        &mut self.stream
    }
}

impl<S: Stream, Fut: Future<Output = ()>, F: Start<Fut = Fut>> Stream for WithTimeout<S, Fut, F> {
    type Item = WithExtra<S::Item, KeepAlive>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        match this.stream.poll_next(cx) {
            Poll::Ready(Some(item)) => {
                this.timeout.set(None);
                let locked = this.locked.upgrade().unwrap_or_else(|| {
                    *this.locking = this.mutex.clone().lock_owned();
                    Arc::new(this.mutex.try_lock_owned().unwrap())
                });
                *this.locked = Arc::downgrade(&locked);
                Poll::Ready(Some(WithExtra::new(item, KeepAlive { _locked: locked })))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => loop {
                match this.timeout.as_mut().as_pin_mut() {
                    Some(fut) => {
                        ready!(fut.poll(cx));
                        *this.done = true;
                        break Poll::Ready(None);
                    }
                    None => {
                        ready!(this.locking.as_mut().poll(cx));
                        this.timeout.set(Some(this.start.make()))
                    }
                }
            },
        }
    }
}

impl<S: FusedStream, Fut: Future<Output = ()>, F: Start<Fut = Fut>> FusedStream
    for WithTimeout<S, Fut, F>
{
    fn is_terminated(&self) -> bool {
        self.done || self.stream.is_terminated()
    }
}

impl<S, Fut, F> WithTimeout<S, Fut, F> {
    #[must_use]
    fn new(stream: S, start: F) -> Self {
        let mutex = Arc::new(Mutex::new(Alive));
        let locking = mutex.clone().lock_owned();
        WithTimeout {
            stream,
            timeout: None,
            start,
            locking,
            locked: Weak::new(),
            mutex,
            done: false,
        }
    }
}

/// Extension trait combinator for closing [`Stream`]s on timeout.
pub trait TimeoutUnused: Sized {
    #[must_use]
    fn timeout_unused<Fut: Future<Output = ()>, F: Start<Fut = Fut>>(
        self,
        start: F,
    ) -> WithTimeout<Self, Fut, F>;
}

impl<S: Stream> TimeoutUnused for S {
    #[must_use]
    fn timeout_unused<Fut: Future<Output = ()>, F: Start<Fut = Fut>>(
        self,
        start: F,
    ) -> WithTimeout<Self, Fut, F> {
        WithTimeout::new(self, start)
    }
}

impl<S, Fut: Future<Output = ()>, F: Default + Start<Fut = Fut>> From<S>
    for WithTimeout<S, Fut, F>
{
    #[must_use]
    fn from(stream: S) -> Self {
        Self::new(stream, Default::default())
    }
}

impl<S: Default, Fut: Future<Output = ()>, F: Default + Start<Fut = Fut>> Default
    for WithTimeout<S, Fut, F>
{
    #[must_use]
    fn default() -> Self {
        S::default().into()
    }
}

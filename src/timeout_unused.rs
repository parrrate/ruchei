use std::{
    pin::Pin,
    sync::{Arc, Weak},
    task::{Context, Poll},
};

use futures_util::{
    lock::{Mutex, OwnedMutexGuard, OwnedMutexLockFuture},
    ready,
    stream::FusedStream,
    Future, Stream,
};
use pin_project::pin_project;

use crate::{callback::FutureFactory, with_extra::WithExtra};

struct Alive;

pub struct KeepAlive(Arc<OwnedMutexGuard<Alive>>);

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

impl<S: Stream, Fut: Future<Output = ()>, F: FutureFactory<Fut = Fut>> Stream
    for WithTimeout<S, Fut, F>
{
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
                Poll::Ready(Some(WithExtra::new(item, KeepAlive(locked))))
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

impl<S: FusedStream, Fut: Future<Output = ()>, F: FutureFactory<Fut = Fut>> FusedStream
    for WithTimeout<S, Fut, F>
{
    fn is_terminated(&self) -> bool {
        self.done || self.stream.is_terminated()
    }
}

pub trait TimeoutUnused: Sized {
    fn timeout_unused<Fut: Future<Output = ()>, F: FutureFactory<Fut = Fut>>(
        self,
        start: F,
    ) -> WithTimeout<Self, Fut, F>;
}

impl<S> TimeoutUnused for S {
    fn timeout_unused<Fut: Future<Output = ()>, F: FutureFactory<Fut = Fut>>(
        self,
        start: F,
    ) -> WithTimeout<Self, Fut, F> {
        let mutex = Arc::new(Mutex::new(Alive));
        let locking = mutex.clone().lock_owned();
        WithTimeout {
            stream: self,
            timeout: None,
            start,
            locking,
            locked: Weak::new(),
            mutex,
            done: false,
        }
    }
}

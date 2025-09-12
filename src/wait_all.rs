use std::{
    pin::Pin,
    sync::{Arc, Mutex, Weak},
    task::{Context, Poll, Waker},
};

use futures_util::Future;

#[derive(Debug, Clone, Default)]
#[must_use]
pub(crate) struct Completable {
    waker: Arc<Mutex<Option<Waker>>>,
}

impl Completable {
    fn complete(self) {
        if let Some(waker) = Arc::into_inner(self.waker)
            && let Ok(mut waker) = waker.try_lock()
            && let Some(waker) = waker.take()
        {
            waker.wake();
        }
    }
}

#[derive(Debug, Clone, Default)]
#[must_use]
pub(crate) struct CompleteOne {
    completable: Option<Completable>,
}

impl CompleteOne {
    #[must_use]
    pub fn pending(&self) -> bool {
        self.completable.is_some()
    }

    pub fn complete(&mut self) {
        if let Some(completable) = self.completable.take() {
            completable.complete();
        }
    }

    pub fn completable(&mut self, completable: Completable) {
        self.completable = Some(completable);
    }
}

impl Drop for CompleteOne {
    fn drop(&mut self) {
        self.complete();
    }
}

#[derive(Default, Debug)]
#[must_use]
pub(crate) struct WaitMany {
    waker: Weak<Mutex<Option<Waker>>>,
}

impl WaitMany {
    pub fn completable(&mut self, completable: Completable) {
        self.waker = Arc::downgrade(&completable.waker);
        completable.complete();
    }
}

impl Future for WaitMany {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        {
            let Some(waker) = self.waker.upgrade() else {
                return Poll::Ready(());
            };
            let Ok(mut waker) = waker.try_lock() else {
                return Poll::Ready(());
            };
            *waker = Some(cx.waker().clone());
        }
        if self.waker.strong_count() == 0 {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

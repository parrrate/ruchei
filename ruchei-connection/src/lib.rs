use std::{
    marker::PhantomPinned,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
};

use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use futures_util::{Stream, task::AtomicWaker};
use pin_project::pin_project;
use ruchei_collections::as_linked_slab::{AsLinkedSlab, SlabKey};

#[derive(Debug, Default)]
#[must_use]
struct SlabWaker {
    waker: AtomicWaker,
}

impl Wake for SlabWaker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.waker.wake();
    }
}

#[must_use]
#[pin_project]
#[derive(Debug)]
pub struct Ready {
    send: UnboundedSender<SlabKey>,
    #[pin]
    recv: UnboundedReceiver<SlabKey>,
    inner: Arc<SlabWaker>,
    waker: Waker,
    _pinned: PhantomPinned,
}

impl Default for Ready {
    fn default() -> Self {
        let (send, recv) = unbounded();
        let inner = Arc::<SlabWaker>::default();
        let waker = inner.clone().into();
        Self {
            send,
            recv,
            inner,
            waker,
            _pinned: PhantomPinned,
        }
    }
}

#[must_use]
#[derive(Debug, Default)]
pub struct ReadyWeak(Option<UnboundedSender<SlabKey>>);

impl Ready {
    pub fn downgrade(&self) -> ReadyWeak {
        ReadyWeak(Some(self.send.clone()))
    }

    pub fn wake(&self) {
        self.inner.wake_by_ref();
    }

    pub fn compact<const M: usize>(self: Pin<&mut Self>, slab: &mut impl AsLinkedSlab) {
        let mut this = self.project();
        while let Poll::Ready(Some(key)) = this
            .recv
            .as_mut()
            .poll_next(&mut Context::from_waker(this.waker))
        {
            slab.link_push_back::<M>(key);
        }
    }

    #[must_use]
    pub fn next<const M: usize>(
        self: Pin<&mut Self>,
        slab: &mut impl AsLinkedSlab,
    ) -> Option<SlabKey> {
        self.compact::<M>(slab);
        slab.link_pop_front::<M>()
    }

    pub fn register(&self, cx: &mut Context<'_>) {
        self.inner.waker.register(cx.waker());
    }
}

impl ReadyWeak {
    pub fn insert(&self, key: SlabKey) {
        if let Some(sender) = self.0.as_ref() {
            let _ = sender.unbounded_send(key);
        }
    }
}

impl Extend<SlabKey> for ReadyWeak {
    fn extend<T: IntoIterator<Item = SlabKey>>(&mut self, iter: T) {
        for key in iter {
            self.insert(key);
        }
    }
}

#[must_use]
#[derive(Debug)]
pub struct ConnectionWaker {
    waker: AtomicWaker,
    ready: ReadyWeak,
    key: SlabKey,
}

impl ConnectionWaker {
    #[must_use]
    pub fn new(key: SlabKey, ready: ReadyWeak) -> Arc<Self> {
        Arc::new(Self {
            waker: Default::default(),
            ready,
            key,
        })
    }

    pub fn wake(&self) {
        self.ready.insert(self.key);
        self.waker.wake();
    }
}

impl Wake for ConnectionWaker {
    fn wake(self: Arc<Self>) {
        (*self).wake();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        (**self).wake();
    }
}

impl ConnectionWaker {
    pub fn poll<T>(
        self: &Arc<Self>,
        cx: &mut Context<'_>,
        f: impl FnOnce(&mut Context<'_>) -> T,
    ) -> T {
        self.waker.register(cx.waker());
        self.poll_detached(f)
    }

    pub fn poll_detached<T>(self: &Arc<Self>, f: impl FnOnce(&mut Context<'_>) -> T) -> T {
        f(&mut Context::from_waker(&Waker::from(self.clone())))
    }
}

#[must_use]
#[derive(Debug)]
pub struct Connection<S> {
    pub stream: S,
    pub next: Arc<ConnectionWaker>,
    pub ready: Arc<ConnectionWaker>,
    pub flush: Arc<ConnectionWaker>,
    pub close: Arc<ConnectionWaker>,
}

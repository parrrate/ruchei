use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
};

use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use futures_util::{Stream, task::AtomicWaker};
use pin_project::pin_project;
use ruchei_collections::as_linked_slab::{AsLinkedSlab, SlabKey};

#[derive(Debug, Default)]
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
pub(crate) struct Ready(
    UnboundedSender<SlabKey>,
    #[pin] UnboundedReceiver<SlabKey>,
    Arc<SlabWaker>,
    Waker,
);

impl Default for Ready {
    fn default() -> Self {
        let (sender, receiver) = unbounded();
        let inner = Arc::<SlabWaker>::default();
        let waker = inner.clone().into();
        Self(sender, receiver, inner, waker)
    }
}

#[must_use]
#[derive(Debug, Default)]
pub(crate) struct ReadyWeak(Option<UnboundedSender<SlabKey>>);

impl Ready {
    pub(crate) fn downgrade(&self) -> ReadyWeak {
        ReadyWeak(Some(self.0.clone()))
    }

    pub(crate) fn wake(&self) {
        self.2.wake_by_ref();
    }

    pub(crate) fn compact<const M: usize>(self: Pin<&mut Self>, slab: &mut impl AsLinkedSlab) {
        let mut this = self.project();
        while let Poll::Ready(Some(key)) =
            this.1.as_mut().poll_next(&mut Context::from_waker(this.3))
        {
            slab.link_push_back::<M>(key);
        }
    }

    #[must_use]
    pub(crate) fn next<const M: usize>(
        self: Pin<&mut Self>,
        slab: &mut impl AsLinkedSlab,
    ) -> Option<SlabKey> {
        self.compact::<M>(slab);
        slab.link_pop_front::<M>()
    }

    pub(crate) fn register(&self, cx: &mut Context<'_>) {
        self.2.waker.register(cx.waker());
    }
}

impl ReadyWeak {
    pub(crate) fn insert(&self, key: SlabKey) {
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
pub(crate) struct ConnectionWaker {
    pub(crate) waker: AtomicWaker,
    ready: ReadyWeak,
    key: SlabKey,
}

impl ConnectionWaker {
    #[must_use]
    pub(crate) fn new(key: SlabKey, ready: ReadyWeak) -> Arc<Self> {
        Arc::new(Self {
            waker: Default::default(),
            ready,
            key,
        })
    }
}

impl Wake for ConnectionWaker {
    fn wake(self: Arc<Self>) {
        self.ready.insert(self.key);
        self.waker.wake();
    }
}

impl ConnectionWaker {
    pub(crate) fn poll<T>(
        self: &Arc<Self>,
        cx: &mut Context<'_>,
        f: impl FnOnce(&mut Context<'_>) -> T,
    ) -> T {
        self.waker.register(cx.waker());
        self.poll_detached(f)
    }

    pub(crate) fn poll_detached<T>(self: &Arc<Self>, f: impl FnOnce(&mut Context<'_>) -> T) -> T {
        f(&mut Context::from_waker(&Waker::from(self.clone())))
    }
}

#[must_use]
pub(crate) struct Connection<S> {
    pub(crate) stream: S,
    pub(crate) next: Arc<ConnectionWaker>,
    pub(crate) ready: Arc<ConnectionWaker>,
    pub(crate) flush: Arc<ConnectionWaker>,
    pub(crate) close: Arc<ConnectionWaker>,
}

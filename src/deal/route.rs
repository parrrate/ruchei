use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
};

use futures_util::{Sink, Stream, TryStream, ready, stream::FusedStream, task::AtomicWaker};
use pin_project::pin_project;
use route_sink::ReadySome;

use crate::merge::pair_item::PairStream;

#[derive(Debug, Default)]
struct Wakers {
    ready: AtomicWaker,
    flush: AtomicWaker,
}

impl Wake for Wakers {
    fn wake(self: Arc<Self>) {
        self.ready.wake();
        self.flush.wake();
    }
}

#[pin_project]
pub struct Dealer<S, K = <S as PairStream>::K, V = <S as PairStream>::V> {
    #[pin]
    router: S,
    ready: Option<K>,
    buffer: Option<V>,
    wakers: Arc<Wakers>,
    waker: Waker,
}

impl<S, K, V> Dealer<S, K, V> {
    fn new(router: S) -> Self {
        let wakers = Arc::<Wakers>::default();
        let waker = Waker::from(wakers.clone());
        Self {
            router,
            ready: None,
            buffer: None,
            wakers,
            waker,
        }
    }
}

impl<S: Default, K, V> Default for Dealer<S, K, V> {
    fn default() -> Self {
        Self::new(Default::default())
    }
}

impl<S: TryStream<Ok = (K, V), Error = E>, K, V, E> Stream for Dealer<S, K, V> {
    type Item = Result<V, E>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.project()
            .router
            .try_poll_next(cx)
            .map_ok(|(_, item)| item)
    }
}

impl<S: FusedStream + TryStream<Ok = (K, V), Error = E>, K, V, E> FusedStream for Dealer<S, K, V> {
    fn is_terminated(&self) -> bool {
        self.router.is_terminated()
    }
}

impl<S: ReadySome<K, V, Error = E>, K, V, E> Dealer<S, K, V> {
    fn poll_buffer(self: Pin<&mut Self>) -> Poll<Result<(), E>> {
        let mut this = self.project();
        if this.buffer.is_some() && this.ready.is_none() {
            let cx = &mut Context::from_waker(this.waker);
            *this.ready = Some(ready!(this.router.as_mut().poll_ready_some(cx))?);
        }
        if let Some(item) = this.buffer.take() {
            let route = this.ready.take().expect("inconsistent state");
            this.router.start_send((route, item))?;
        }
        Poll::Ready(Ok(()))
    }
}

impl<S: ReadySome<K, V, Error = E>, K, V, E> Sink<V> for Dealer<S, K, V> {
    type Error = E;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.wakers.ready.register(cx.waker());
        self.poll_buffer()
    }

    fn start_send(self: Pin<&mut Self>, item: V) -> Result<(), Self::Error> {
        let this = self.project();
        assert!(this.buffer.is_none(), "sink not ready");
        *this.buffer = Some(item);
        Ok(())
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.wakers.flush.register(cx.waker());
        ready!(self.as_mut().poll_buffer())?;
        self.project().router.poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.wakers.flush.register(cx.waker());
        ready!(self.as_mut().poll_buffer())?;
        self.project().router.poll_close(cx)
    }
}

pub trait DealRoute: Sized + TryStream<Ok = (Self::K, Self::T)> {
    type K;
    type T;
    #[must_use]
    fn deal_route(self) -> Dealer<Self, Self::K, Self::T> {
        Dealer::new(self)
    }
}

impl<S: TryStream<Ok = (K, T)>, K, T> DealRoute for S {
    type K = K;
    type T = T;
}

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{Sink, Stream, TryStream, ready, stream::FusedStream};
use pin_project::pin_project;
use route_sink::ReadySome;

use crate::merge::pair_item::PairStream;

#[pin_project]
pub struct Dealer<S, K = <S as PairStream>::K> {
    #[pin]
    router: S,
    ready: Option<K>,
}

impl<S: Default, K> Default for Dealer<S, K> {
    fn default() -> Self {
        Self {
            router: Default::default(),
            ready: Default::default(),
        }
    }
}

impl<S: TryStream<Ok = (K, T), Error = E>, K, T, E> Stream for Dealer<S, K> {
    type Item = Result<T, E>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.project()
            .router
            .try_poll_next(cx)
            .map_ok(|(_, item)| item)
    }
}

impl<S: FusedStream + TryStream<Ok = (K, T), Error = E>, K, T, E> FusedStream for Dealer<S, K> {
    fn is_terminated(&self) -> bool {
        self.router.is_terminated()
    }
}

impl<S: ReadySome<K, T, Error = E>, K, T, E> Sink<T> for Dealer<S, K> {
    type Error = E;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.project();
        if this.ready.is_none() {
            *this.ready = Some(ready!(this.router.poll_ready_some(cx))?);
        }
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, item: T) -> Result<(), Self::Error> {
        let this = self.project();
        let route = this.ready.take().expect("sink not ready");
        this.router.start_send((route, item))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().router.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().router.poll_close(cx)
    }
}

pub trait DealRoute<K>: Sized {
    #[must_use]
    fn deal_route(self) -> Dealer<Self, K> {
        Dealer {
            router: self,
            ready: None,
        }
    }
}

impl<S: TryStream<Ok = (K, T)>, K, T> DealRoute<K> for S {}

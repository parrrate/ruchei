extern crate std;

use core::{
    pin::Pin,
    task::{Context, Poll},
};

use extend_pinned::ExtendPinned;
use futures_core::{FusedStream, Stream};
#[cfg(feature = "sink")]
use futures_sink::Sink;
use pin_project::pin_project;
#[cfg(feature = "route-sink")]
use route_sink::{FlushRoute, ReadyRoute, ReadySome};

use crate::{Extending, ExtendingExt};

pub enum ExtendItem<K, T> {
    Pushed(K),
    Item(T),
}

impl<K, T, E> ExtendItem<K, Result<T, E>> {
    pub fn transpose(self) -> Result<ExtendItem<K, T>, E> {
        match self {
            Self::Pushed(x) => Ok(ExtendItem::Pushed(x)),
            Self::Item(Ok(x)) => Ok(ExtendItem::Item(x)),
            Self::Item(Err(x)) => Err(x),
        }
    }
}

#[pin_project]
#[derive(Debug)]
pub struct ExtendKeyed<S, K> {
    #[pin]
    inner: S,
    pushed: std::collections::VecDeque<K>,
}

impl<S: Default, K> Default for ExtendKeyed<S, K> {
    fn default() -> Self {
        S::default().into()
    }
}

impl<S, K> From<S> for ExtendKeyed<S, K> {
    fn from(inner: S) -> Self {
        Self {
            inner,
            pushed: Default::default(),
        }
    }
}

impl<S: ExtendPinned<T>, K, T> ExtendPinned<(K, T)> for ExtendKeyed<S, K> {
    fn extend_pinned<I: IntoIterator<Item = (K, T)>>(self: Pin<&mut Self>, iter: I) {
        let this = self.project();
        this.inner.extend_pinned(iter.into_iter().map(|(k, t)| {
            this.pushed.push_back(k);
            t
        }));
    }
}

impl<S: Stream, K> Stream for ExtendKeyed<S, K> {
    type Item = ExtendItem<K, S::Item>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        if let Some(k) = this.pushed.pop_front() {
            return Poll::Ready(Some(ExtendItem::Pushed(k)));
        }
        this.inner.poll_next(cx).map(|o| o.map(ExtendItem::Item))
    }
}

impl<S: FusedStream, K> FusedStream for ExtendKeyed<S, K> {
    fn is_terminated(&self) -> bool {
        self.pushed.is_empty() && self.inner.is_terminated()
    }
}

#[cfg(feature = "sink")]
impl<Item, S: Sink<Item>, K> Sink<Item> for ExtendKeyed<S, K> {
    type Error = S::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().inner.poll_ready(cx)
    }

    fn start_send(self: Pin<&mut Self>, item: Item) -> Result<(), Self::Error> {
        self.project().inner.start_send(item)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().inner.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().inner.poll_close(cx)
    }
}

#[cfg(feature = "route-sink")]
impl<Route, Msg, S: FlushRoute<Route, Msg>, K> FlushRoute<Route, Msg> for ExtendKeyed<S, K> {
    fn poll_flush_route(
        self: Pin<&mut Self>,
        route: &Route,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.project().inner.poll_flush_route(route, cx)
    }

    fn poll_close_route(
        self: Pin<&mut Self>,
        route: &Route,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.project().inner.poll_close_route(route, cx)
    }
}

#[cfg(feature = "route-sink")]
impl<Route, Msg, S: ReadyRoute<Route, Msg>, K> ReadyRoute<Route, Msg> for ExtendKeyed<S, K> {
    fn poll_ready_route(
        self: Pin<&mut Self>,
        route: &Route,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.project().inner.poll_ready_route(route, cx)
    }
}

#[cfg(feature = "route-sink")]
impl<Route, Msg, S: ReadySome<Route, Msg>, K> ReadySome<Route, Msg> for ExtendKeyed<S, K> {
    fn poll_ready_some(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Route, Self::Error>> {
        self.project().inner.poll_ready_some(cx)
    }
}

pub trait ExtendingKeyedExt: Sized + FusedStream<Item = (Self::K, Self::T)> {
    type K;
    type T;

    #[must_use]
    fn extending_keyed<S: ExtendPinned<Self::T>>(
        self,
        inner: S,
    ) -> Extending<ExtendKeyed<S, Self::K>, Self> {
        self.extending(inner.into())
    }

    #[must_use]
    fn extending_keyed_default<S: Default + ExtendPinned<Self::T>>(
        self,
    ) -> Extending<ExtendKeyed<S, Self::K>, Self> {
        self.extending_default()
    }
}

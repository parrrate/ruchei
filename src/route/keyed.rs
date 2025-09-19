use std::{
    convert::Infallible,
    fmt::Debug,
    hash::Hash,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{Sink, Stream, TryStream, ready, stream::FusedStream};
use linked_hash_map::LinkedHashMap;
use linked_hash_set::LinkedHashSet;
use pin_project::pin_project;
use route_sink::{FlushRoute, ReadyRoute};

use crate::{
    multi_item::{MultiItem, MultiRouteItem},
    pinned_extend::{Extending, PinnedExtend},
};

use super::{Key, without_multicast::RouteKey};

#[pin_project]
#[derive(Debug)]
struct One<K, S> {
    key: K,
    #[pin]
    stream: S,
}

impl<In, K: Key, E, S: TryStream<Ok = In, Error = E>> Stream for One<K, S> {
    type Item = Result<(K, In), E>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        this.stream
            .try_poll_next(cx)
            .map_ok(|v| (this.key.clone(), v))
    }
}

impl<Out, K, E, S: Sink<Out, Error = E>> Sink<Out> for One<K, S> {
    type Error = E;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().stream.poll_ready(cx)
    }

    fn start_send(self: Pin<&mut Self>, item: Out) -> Result<(), Self::Error> {
        self.project().stream.start_send(item)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().stream.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().stream.poll_close(cx)
    }
}

/// [`ReadyRoute`]/[`Stream`] implemented over the stream of incoming [`Sink`]s/[`Stream`]s.
#[pin_project]
#[derive(Debug)]
pub struct Router<K: Hash + Eq, S, E = <S as TryStream>::Error> {
    #[pin]
    router: super::without_multicast::Router<One<K, S>, E>,
    routes: LinkedHashMap<K, LinkedHashSet<RouteKey>>,
}

impl<K: Hash + Eq, S, E> Default for Router<K, S, E> {
    fn default() -> Self {
        Self {
            router: Default::default(),
            routes: Default::default(),
        }
    }
}

impl<In, K: Key, E, S: Unpin + TryStream<Ok = In, Error = E>> Stream for Router<K, S, E> {
    type Item = MultiRouteItem<K, S>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        while let Some(item) = ready!(this.router.as_mut().poll_next(cx)) {
            return Poll::Ready(Some(match item {
                MultiItem::Item((i, (k, v))) => {
                    this.routes.entry(k.clone()).or_default().insert(i);
                    MultiItem::Item((k, v))
                }
                MultiItem::Closed((i, One { key, stream }), e) => {
                    let mut entry = match this.routes.entry(key.clone()) {
                        linked_hash_map::Entry::Occupied(entry) => entry,
                        linked_hash_map::Entry::Vacant(_) => continue,
                    };
                    if !entry.get_mut().remove(&i) {
                        continue;
                    }
                    if entry.get().is_empty() {
                        entry.remove();
                    }
                    MultiItem::Closed((key, stream), e)
                }
            }));
        }
        Poll::Ready(None)
    }
}

impl<In, K: Key, E, S: Unpin + TryStream<Ok = In, Error = E>> FusedStream for Router<K, S, E> {
    fn is_terminated(&self) -> bool {
        self.router.is_terminated()
    }
}

impl<Out, K: Key, E, S: Unpin + Sink<Out, Error = E>> Sink<(K, Out)> for Router<K, S, E> {
    type Error = Infallible;

    fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Pending
    }

    fn start_send(self: Pin<&mut Self>, (key, msg): (K, Out)) -> Result<(), Self::Error> {
        let key = &key;
        let this = self.project();
        if let Some(routes) = this.routes.get(key) {
            let route = *routes.back().expect("empty routes per key");
            this.router.start_send((route, msg))
        } else {
            Ok(())
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Pending
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().router.poll_close(cx)
    }
}

impl<Out, K: Key, E, S: Unpin + Sink<Out, Error = E>> FlushRoute<K, Out> for Router<K, S, E> {
    fn poll_flush_route(
        self: Pin<&mut Self>,
        key: &K,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        let this = self.project();
        if let Some(routes) = this.routes.get(key) {
            let route = routes.back().expect("empty routes per key");
            this.router.poll_flush_route(route, cx)
        } else {
            Poll::Ready(Ok(()))
        }
    }
}

impl<Out, K: Key, E, S: Unpin + Sink<Out, Error = E>> ReadyRoute<K, Out> for Router<K, S, E> {
    fn poll_ready_route(
        self: Pin<&mut Self>,
        key: &K,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        let this = self.project();
        if let Some(routes) = this.routes.get(key) {
            let route = routes.back().expect("empty routes per key");
            this.router.poll_ready_route(route, cx)
        } else {
            Poll::Ready(Ok(()))
        }
    }
}

impl<K: Key, S, E> Router<K, S, E> {
    fn push(self: Pin<&mut Self>, key: K, stream: S) {
        let this = self.project();
        this.router.extend_pinned([One { key, stream }]);
    }
}

impl<K: Key, S, E> PinnedExtend<(K, S)> for Router<K, S, E> {
    fn extend_pinned<T: IntoIterator<Item = (K, S)>>(mut self: Pin<&mut Self>, iter: T) {
        for (key, stream) in iter {
            self.as_mut().push(key, stream)
        }
    }
}

/// [`ReadyRoute`]/[`Stream`] Returned by [`RouterKeyedExt::route_keyed`].
pub type RouterExtending<R> =
    Extending<Router<<R as RouterKeyedExt>::K, <R as RouterKeyedExt>::S>, R>;

/// Extension trait to auto-extend a [`Router`] from a stream of connections.
pub trait RouterKeyedExt: Sized + FusedStream<Item = (Self::K, Self::S)> {
    /// Key.
    type K: Key;
    /// Single [`Stream`]/[`Sink`].
    type S: Unpin + TryStream;

    /// Extend the stream of connections (`self`) into a [`Router`].
    #[must_use]
    fn route_keyed(self) -> RouterExtending<Self> {
        Extending::new(self, Default::default())
    }
}

impl<K: Key, S: Unpin + TryStream, R: FusedStream<Item = (K, S)>> RouterKeyedExt for R {
    type K = K;
    type S = S;
}

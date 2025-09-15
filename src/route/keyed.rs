use std::{
    convert::Infallible,
    hash::Hash,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{Sink, Stream, TryStream, ready, stream::FusedStream};
use linked_hash_map::LinkedHashMap;
use pin_project::pin_project;
pub use ruchei_route::{RouteExt, RouteSink, Unroute, WithRoute};

use crate::{
    multi_item::MultiItem,
    pinned_extend::{Extending, ExtendingRoute, PinnedExtend},
};

use super::Key;

#[pin_project]
pub struct One<K, S> {
    ctr: usize,
    key: K,
    #[pin]
    stream: S,
}

impl<In, K: Key, E, S: TryStream<Ok = In, Error = E>> Stream for One<K, S> {
    type Item = Result<(usize, K, In), E>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        this.stream
            .try_poll_next(cx)
            .map_ok(|v| (*this.ctr, this.key.clone(), v))
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

/// [`RouteSink`]/[`Stream`] implemented over the stream of incoming [`Sink`]s/[`Stream`]s.
#[pin_project]
pub struct Router<K, I, R> {
    #[pin]
    router: R,
    routes: LinkedHashMap<K, LinkedHashMap<usize, I>>,
    ctr: usize,
}

impl<K: Hash + Eq, I, R: Default> Default for Router<K, I, R> {
    fn default() -> Self {
        Self {
            router: Default::default(),
            routes: Default::default(),
            ctr: Default::default(),
        }
    }
}

impl<In, K: Key, E, S, I, R: Stream<Item = MultiItem<(I, (usize, K, In)), One<K, S>, E>>> Stream
    for Router<K, I, R>
{
    type Item = MultiItem<(K, In), (K, S), E>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        while let Some(item) = ready!(this.router.as_mut().poll_next(cx)) {
            return Poll::Ready(Some(match item {
                MultiItem::Item((i, (ctr, k, v))) => {
                    this.routes.entry(k.clone()).or_default().insert(ctr, i);
                    MultiItem::Item((k, v))
                }
                MultiItem::Closed(One { ctr, key, stream }, e) => {
                    let mut entry = match this.routes.entry(key.clone()) {
                        linked_hash_map::Entry::Occupied(entry) => entry,
                        linked_hash_map::Entry::Vacant(_) => continue,
                    };
                    if entry.get_mut().remove(&ctr).is_none() {
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

impl<In, K: Key, E, S, I, R: FusedStream<Item = MultiItem<(I, (usize, K, In)), One<K, S>, E>>>
    FusedStream for Router<K, I, R>
{
    fn is_terminated(&self) -> bool {
        self.router.is_terminated()
    }
}

impl<Out, K: Key, I: Clone, R: RouteSink<I, Out, Error = Infallible>> RouteSink<K, Out>
    for Router<K, I, R>
{
    type Error = Infallible;

    fn poll_ready(
        self: Pin<&mut Self>,
        key: &K,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        let this = self.project();
        if let Some(routes) = this.routes.get(key) {
            let route = routes.back().expect("empty routes per key").1;
            this.router.poll_ready(route, cx)
        } else {
            Poll::Ready(Ok(()))
        }
    }

    fn start_send(self: Pin<&mut Self>, key: K, msg: Out) -> Result<(), Self::Error> {
        let key = &key;
        let this = self.project();
        if let Some(routes) = this.routes.get(key) {
            let route = routes.back().expect("empty routes per key").1.clone();
            this.router.start_send(route, msg)
        } else {
            Ok(())
        }
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        key: &K,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        let this = self.project();
        if let Some(routes) = this.routes.get(key) {
            let route = routes.back().expect("empty routes per key").1;
            this.router.poll_flush(route, cx)
        } else {
            Poll::Ready(Ok(()))
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().router.poll_close(cx)
    }
}

impl<K: Key, I, R> Router<K, I, R> {
    fn push<S>(self: Pin<&mut Self>, key: K, stream: S)
    where
        R: PinnedExtend<One<K, S>>,
    {
        let this = self.project();
        let ctr = *this.ctr;
        *this.ctr += 1;
        this.router.extend_pinned([One { ctr, key, stream }]);
    }
}

impl<K: Key, S, I, R: PinnedExtend<One<K, S>>> PinnedExtend<(K, S)> for Router<K, I, R> {
    fn extend_pinned<T: IntoIterator<Item = (K, S)>>(mut self: Pin<&mut Self>, iter: T) {
        for (key, stream) in iter {
            self.as_mut().push(key, stream)
        }
    }
}

/// [`RouteSink`]/[`Stream`] Returned by [`RouterKeyedExt::route_keyed`].
pub type RouterExtending<R> = ExtendingRoute<
    Router<
        <R as RouterKeyedExt>::K,
        usize,
        super::slab::Router<
            One<<R as RouterKeyedExt>::K, <R as RouterKeyedExt>::S>,
            <R as RouterKeyedExt>::E,
        >,
    >,
    R,
>;

/// Extension trait to auto-extend a [`Router`] from a stream of connections.
pub trait RouterKeyedExt: Sized {
    /// Key.
    type K;
    /// Single [`Stream`]/[`Sink`].
    type S;
    /// Error.
    type E;

    /// Extend the stream of connections (`self`) into a [`Router`].
    #[must_use]
    fn route_keyed(self) -> RouterExtending<Self>;
}

impl<In, K: Key, E, S: Unpin + TryStream<Ok = In, Error = E>, R: FusedStream<Item = (K, S)>>
    RouterKeyedExt for R
{
    type K = K;
    type S = S;
    type E = E;

    fn route_keyed(self) -> RouterExtending<Self> {
        ExtendingRoute(Extending::new(self, Default::default()))
    }
}

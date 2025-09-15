use futures_sink::Sink;
use futures_util::{
    Stream, TryFuture, TryStream,
    stream::{
        AndThen, Enumerate, Filter, FilterMap, FlatMap, FlatMapUnordered, Flatten, Fuse,
        IntoStream, Map, MapErr, MapOk, Then, TryFilter, TryFilterMap, TryFlatten,
        TryFlattenUnordered,
    },
};

use crate::{FlushRoute, ReadyRoute, ReadySome};

macro_rules! delegate_fr {
    ($route:ty) => {
        fn poll_flush_route(
            self: core::pin::Pin<&mut Self>,
            route: &$route,
            cx: &mut core::task::Context<'_>,
        ) -> core::task::Poll<Result<(), Self::Error>> {
            self.get_pin_mut().poll_flush_route(route, cx)
        }

        fn poll_close_route(
            self: core::pin::Pin<&mut Self>,
            route: &$route,
            cx: &mut core::task::Context<'_>,
        ) -> core::task::Poll<Result<(), Self::Error>> {
            self.get_pin_mut().poll_close_route(route, cx)
        }
    };
}

macro_rules! delegate_rr {
    ($route:ty) => {
        fn poll_ready_route(
            self: core::pin::Pin<&mut Self>,
            route: &$route,
            cx: &mut core::task::Context<'_>,
        ) -> core::task::Poll<Result<(), Self::Error>> {
            self.get_pin_mut().poll_ready_route(route, cx)
        }
    };
}

macro_rules! delegate_rs {
    ($route:ty) => {
        fn poll_ready_some(
            self: core::pin::Pin<&mut Self>,
            cx: &mut core::task::Context<'_>,
        ) -> core::task::Poll<Result<$route, Self::Error>> {
            self.get_pin_mut().poll_ready_some(cx)
        }
    };
}

impl<St, Fut, F, Route, Msg> FlushRoute<Route, Msg> for AndThen<St, Fut, F>
where
    St: TryStream + FlushRoute<Route, Msg>,
    F: FnMut(St::Ok) -> Fut,
    Fut: TryFuture<Error = <St as TryStream>::Error>,
{
    delegate_fr!(Route);
}

impl<St, Fut, F, Route, Msg> ReadyRoute<Route, Msg> for AndThen<St, Fut, F>
where
    St: TryStream + ReadyRoute<Route, Msg>,
    F: FnMut(St::Ok) -> Fut,
    Fut: TryFuture<Error = <St as TryStream>::Error>,
{
    delegate_rr!(Route);
}

impl<St, Fut, F, Route, Msg> ReadySome<Route, Msg> for AndThen<St, Fut, F>
where
    St: TryStream + ReadySome<Route, Msg>,
    F: FnMut(St::Ok) -> Fut,
    Fut: TryFuture<Error = <St as TryStream>::Error>,
{
    delegate_rs!(Route);
}

impl<St: Stream, Route, Msg> FlushRoute<Route, Msg> for Enumerate<St>
where
    St: FlushRoute<Route, Msg>,
{
    delegate_fr!(Route);
}

impl<St: Stream, Route, Msg> ReadyRoute<Route, Msg> for Enumerate<St>
where
    St: ReadyRoute<Route, Msg>,
{
    delegate_rr!(Route);
}

impl<St: Stream, Route, Msg> ReadySome<Route, Msg> for Enumerate<St>
where
    St: ReadySome<Route, Msg>,
{
    delegate_rs!(Route);
}

impl<St, Fut, F, Route, Msg> FlushRoute<Route, Msg> for Filter<St, Fut, F>
where
    St: Stream + FlushRoute<Route, Msg>,
    F: FnMut(&St::Item) -> Fut,
    Fut: Future<Output = bool>,
{
    delegate_fr!(Route);
}

impl<St, Fut, F, Route, Msg> ReadyRoute<Route, Msg> for Filter<St, Fut, F>
where
    St: Stream + ReadyRoute<Route, Msg>,
    F: FnMut(&St::Item) -> Fut,
    Fut: Future<Output = bool>,
{
    delegate_rr!(Route);
}

impl<St, Fut, F, Route, Msg> ReadySome<Route, Msg> for Filter<St, Fut, F>
where
    St: Stream + ReadySome<Route, Msg>,
    F: FnMut(&St::Item) -> Fut,
    Fut: Future<Output = bool>,
{
    delegate_rs!(Route);
}

impl<St, Fut, F, Route, Msg> FlushRoute<Route, Msg> for FilterMap<St, Fut, F>
where
    St: Stream + FlushRoute<Route, Msg>,
    F: FnMut(St::Item) -> Fut,
    Fut: Future,
{
    delegate_fr!(Route);
}

impl<St, Fut, F, Route, Msg> ReadyRoute<Route, Msg> for FilterMap<St, Fut, F>
where
    St: Stream + ReadyRoute<Route, Msg>,
    F: FnMut(St::Item) -> Fut,
    Fut: Future,
{
    delegate_rr!(Route);
}

impl<St, Fut, F, Route, Msg> ReadySome<Route, Msg> for FilterMap<St, Fut, F>
where
    St: Stream + ReadySome<Route, Msg>,
    F: FnMut(St::Item) -> Fut,
    Fut: Future,
{
    delegate_rs!(Route);
}

impl<St, U, F, Route, Msg, E> FlushRoute<Route, Msg> for FlatMap<St, U, F>
where
    St: FlushRoute<Route, Msg, Error = E>,
    Self: Sink<(Route, Msg), Error = E>,
{
    delegate_fr!(Route);
}

impl<St, U, F, Route, Msg, E> ReadyRoute<Route, Msg> for FlatMap<St, U, F>
where
    St: ReadyRoute<Route, Msg, Error = E>,
    Self: Sink<(Route, Msg), Error = E>,
{
    delegate_rr!(Route);
}

impl<St, U, F, Route, Msg, E> ReadySome<Route, Msg> for FlatMap<St, U, F>
where
    St: ReadySome<Route, Msg, Error = E>,
    Self: Sink<(Route, Msg), Error = E>,
{
    delegate_rs!(Route);
}

impl<St, U, F, Route, Msg> FlushRoute<Route, Msg> for FlatMapUnordered<St, U, F>
where
    St: Stream + FlushRoute<Route, Msg>,
    U: Stream + Unpin,
    F: FnMut(St::Item) -> U,
{
    delegate_fr!(Route);
}

impl<St, U, F, Route, Msg> ReadyRoute<Route, Msg> for FlatMapUnordered<St, U, F>
where
    St: Stream + ReadyRoute<Route, Msg>,
    U: Stream + Unpin,
    F: FnMut(St::Item) -> U,
{
    delegate_rr!(Route);
}

impl<St, U, F, Route, Msg> ReadySome<Route, Msg> for FlatMapUnordered<St, U, F>
where
    St: Stream + ReadySome<Route, Msg>,
    U: Stream + Unpin,
    F: FnMut(St::Item) -> U,
{
    delegate_rs!(Route);
}

impl<St, Route, Msg, E> FlushRoute<Route, Msg> for Flatten<St>
where
    St: Stream + FlushRoute<Route, Msg, Error = E>,
    Self: Sink<(Route, Msg), Error = E>,
{
    delegate_fr!(Route);
}

impl<St, Route, Msg, E> ReadyRoute<Route, Msg> for Flatten<St>
where
    St: Stream + ReadyRoute<Route, Msg, Error = E>,
    Self: Sink<(Route, Msg), Error = E>,
{
    delegate_rr!(Route);
}

impl<St, Route, Msg, E> ReadySome<Route, Msg> for Flatten<St>
where
    St: Stream + ReadySome<Route, Msg, Error = E>,
    Self: Sink<(Route, Msg), Error = E>,
{
    delegate_rs!(Route);
}

impl<St: Stream, Route, Msg> FlushRoute<Route, Msg> for Fuse<St>
where
    St: FlushRoute<Route, Msg>,
{
    delegate_fr!(Route);
}

impl<St: Stream, Route, Msg> ReadyRoute<Route, Msg> for Fuse<St>
where
    St: ReadyRoute<Route, Msg>,
{
    delegate_rr!(Route);
}

impl<St: Stream, Route, Msg> ReadySome<Route, Msg> for Fuse<St>
where
    St: ReadySome<Route, Msg>,
{
    delegate_rs!(Route);
}

impl<St, Route, Msg> FlushRoute<Route, Msg> for IntoStream<St>
where
    St: FlushRoute<Route, Msg>,
{
    delegate_fr!(Route);
}

impl<St, Route, Msg> ReadyRoute<Route, Msg> for IntoStream<St>
where
    St: ReadyRoute<Route, Msg>,
{
    delegate_rr!(Route);
}

impl<St, Route, Msg> ReadySome<Route, Msg> for IntoStream<St>
where
    St: ReadySome<Route, Msg>,
{
    delegate_rs!(Route);
}

impl<St, F, Route, Msg> FlushRoute<Route, Msg> for Map<St, F>
where
    St: Stream + FlushRoute<Route, Msg>,
    F: FnMut(St::Item),
{
    delegate_fr!(Route);
}

impl<St, F, Route, Msg> ReadyRoute<Route, Msg> for Map<St, F>
where
    St: Stream + ReadyRoute<Route, Msg>,
    F: FnMut(St::Item),
{
    delegate_rr!(Route);
}

impl<St, F, Route, Msg> ReadySome<Route, Msg> for Map<St, F>
where
    St: Stream + ReadySome<Route, Msg>,
    F: FnMut(St::Item),
{
    delegate_rs!(Route);
}

impl<St, F, Route, Msg> FlushRoute<Route, Msg> for MapOk<St, F>
where
    St: TryStream + FlushRoute<Route, Msg>,
    F: FnMut(St::Ok),
{
    delegate_fr!(Route);
}

impl<St, F, Route, Msg> ReadyRoute<Route, Msg> for MapOk<St, F>
where
    St: TryStream + ReadyRoute<Route, Msg>,
    F: FnMut(St::Ok),
{
    delegate_rr!(Route);
}

impl<St, F, Route, Msg> ReadySome<Route, Msg> for MapOk<St, F>
where
    St: TryStream + ReadySome<Route, Msg>,
    F: FnMut(St::Ok),
{
    delegate_rs!(Route);
}

impl<St, F, Route, Msg> FlushRoute<Route, Msg> for MapErr<St, F>
where
    St: TryStream + FlushRoute<Route, Msg>,
    F: FnMut(<St as TryStream>::Error),
{
    delegate_fr!(Route);
}

impl<St, F, Route, Msg> ReadyRoute<Route, Msg> for MapErr<St, F>
where
    St: TryStream + ReadyRoute<Route, Msg>,
    F: FnMut(<St as TryStream>::Error),
{
    delegate_rr!(Route);
}

impl<St, F, Route, Msg> ReadySome<Route, Msg> for MapErr<St, F>
where
    St: TryStream + ReadySome<Route, Msg>,
    F: FnMut(<St as TryStream>::Error),
{
    delegate_rs!(Route);
}

impl<St, Fut, F, Route, Msg> FlushRoute<Route, Msg> for Then<St, Fut, F>
where
    St: Stream + FlushRoute<Route, Msg>,
    F: FnMut(St::Item) -> Fut,
{
    delegate_fr!(Route);
}

impl<St, Fut, F, Route, Msg> ReadyRoute<Route, Msg> for Then<St, Fut, F>
where
    St: Stream + ReadyRoute<Route, Msg>,
    F: FnMut(St::Item) -> Fut,
{
    delegate_rr!(Route);
}

impl<St, Fut, F, Route, Msg> ReadySome<Route, Msg> for Then<St, Fut, F>
where
    St: Stream + ReadySome<Route, Msg>,
    F: FnMut(St::Item) -> Fut,
{
    delegate_rs!(Route);
}

impl<St, Fut, F, Route, Msg> FlushRoute<Route, Msg> for TryFilter<St, Fut, F>
where
    St: TryStream + FlushRoute<Route, Msg>,
    F: FnMut(&St::Item) -> Fut,
{
    delegate_fr!(Route);
}

impl<St, Fut, F, Route, Msg> ReadyRoute<Route, Msg> for TryFilter<St, Fut, F>
where
    St: TryStream + ReadyRoute<Route, Msg>,
    F: FnMut(&St::Item) -> Fut,
{
    delegate_rr!(Route);
}

impl<St, Fut, F, Route, Msg> ReadySome<Route, Msg> for TryFilter<St, Fut, F>
where
    St: TryStream + ReadySome<Route, Msg>,
    F: FnMut(&St::Item) -> Fut,
{
    delegate_rs!(Route);
}

impl<St, Fut, F, Route, Msg> FlushRoute<Route, Msg> for TryFilterMap<St, Fut, F>
where
    St: FlushRoute<Route, Msg>,
{
    delegate_fr!(Route);
}

impl<St, Fut, F, Route, Msg> ReadyRoute<Route, Msg> for TryFilterMap<St, Fut, F>
where
    St: ReadyRoute<Route, Msg>,
{
    delegate_rr!(Route);
}

impl<St, Fut, F, Route, Msg> ReadySome<Route, Msg> for TryFilterMap<St, Fut, F>
where
    St: ReadySome<Route, Msg>,
{
    delegate_rs!(Route);
}

impl<St, Route, Msg> FlushRoute<Route, Msg> for TryFlatten<St>
where
    St: TryStream + FlushRoute<Route, Msg>,
    St::Ok: TryStream,
    <St::Ok as TryStream>::Error: From<<St as TryStream>::Error>,
{
    delegate_fr!(Route);
}

impl<St, Route, Msg> ReadyRoute<Route, Msg> for TryFlatten<St>
where
    St: TryStream + ReadyRoute<Route, Msg>,
    St::Ok: TryStream,
    <St::Ok as TryStream>::Error: From<<St as TryStream>::Error>,
{
    delegate_rr!(Route);
}

impl<St, Route, Msg> ReadySome<Route, Msg> for TryFlatten<St>
where
    St: TryStream + ReadySome<Route, Msg>,
    St::Ok: TryStream,
    <St::Ok as TryStream>::Error: From<<St as TryStream>::Error>,
{
    delegate_rs!(Route);
}

impl<St, Route, Msg> FlushRoute<Route, Msg> for TryFlattenUnordered<St>
where
    St: TryStream + FlushRoute<Route, Msg>,
    St::Ok: Unpin + TryStream,
    <St::Ok as TryStream>::Error: From<<St as TryStream>::Error>,
{
    delegate_fr!(Route);
}

impl<St, Route, Msg> ReadyRoute<Route, Msg> for TryFlattenUnordered<St>
where
    St: TryStream + ReadyRoute<Route, Msg>,
    St::Ok: Unpin + TryStream,
    <St::Ok as TryStream>::Error: From<<St as TryStream>::Error>,
{
    delegate_rr!(Route);
}

impl<St, Route, Msg> ReadySome<Route, Msg> for TryFlattenUnordered<St>
where
    St: TryStream + ReadySome<Route, Msg>,
    St::Ok: Unpin + TryStream,
    <St::Ok as TryStream>::Error: From<<St as TryStream>::Error>,
{
    delegate_rs!(Route);
}

use futures_util::{
    Stream, TryFuture, TryStream,
    stream::{AndThen, Enumerate, Filter, FilterMap},
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

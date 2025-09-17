use core::{
    ops::DerefMut,
    pin::Pin,
    task::{Context, Poll},
};

use crate::{FlushRoute, ReadyRoute, ReadySome};

impl<P, Route, Msg> FlushRoute<Route, Msg> for Pin<P>
where
    P: Unpin + DerefMut,
    P::Target: FlushRoute<Route, Msg>,
{
    fn poll_flush_route(
        self: Pin<&mut Self>,
        route: &Route,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.get_mut().as_mut().poll_flush_route(route, cx)
    }

    fn poll_close_route(
        self: Pin<&mut Self>,
        route: &Route,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.get_mut().as_mut().poll_close_route(route, cx)
    }
}

impl<P, Route, Msg> ReadyRoute<Route, Msg> for Pin<P>
where
    P: Unpin + DerefMut,
    P::Target: ReadyRoute<Route, Msg>,
{
    fn poll_ready_route(
        self: Pin<&mut Self>,
        route: &Route,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.get_mut().as_mut().poll_ready_route(route, cx)
    }
}

impl<P, Route, Msg> ReadySome<Route, Msg> for Pin<P>
where
    P: Unpin + DerefMut,
    P::Target: ReadySome<Route, Msg>,
{
    fn poll_ready_some(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Route, Self::Error>> {
        self.get_mut().as_mut().poll_ready_some(cx)
    }
}

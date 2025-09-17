extern crate std;

use std::{
    boxed::Box,
    pin::Pin,
    task::{Context, Poll},
};

use crate::{FlushRoute, ReadyRoute, ReadySome};

impl<S: ?Sized + Unpin + FlushRoute<Route, Msg>, Route, Msg> FlushRoute<Route, Msg> for Box<S> {
    fn poll_flush_route(
        mut self: Pin<&mut Self>,
        route: &Route,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut **self).poll_flush_route(route, cx)
    }

    fn poll_close_route(
        mut self: Pin<&mut Self>,
        route: &Route,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut **self).poll_close_route(route, cx)
    }
}

impl<S: ?Sized + Unpin + ReadyRoute<Route, Msg>, Route, Msg> ReadyRoute<Route, Msg> for Box<S> {
    fn poll_ready_route(
        mut self: Pin<&mut Self>,
        route: &Route,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut **self).poll_ready_route(route, cx)
    }
}

impl<S: ?Sized + Unpin + ReadySome<Route, Msg>, Route, Msg> ReadySome<Route, Msg> for Box<S> {
    fn poll_ready_some(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Route, Self::Error>> {
        Pin::new(&mut **self).poll_ready_some(cx)
    }
}

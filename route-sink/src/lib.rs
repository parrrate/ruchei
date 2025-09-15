#![no_std]

use core::{
    pin::Pin,
    task::{Context, Poll},
};

use futures_sink::Sink;

#[cfg(feature = "futures-util")]
mod futures_util_impls;

pub trait FlushRoute<Route, Msg>: Sink<(Route, Msg)> {
    fn poll_flush_route(
        self: Pin<&mut Self>,
        route: &Route,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        let _ = route;
        self.poll_flush(cx)
    }

    fn poll_close_route(
        self: Pin<&mut Self>,
        route: &Route,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.poll_flush_route(route, cx)
    }
}

pub trait ReadyRoute<Route, Msg>: FlushRoute<Route, Msg> {
    fn poll_ready_route(
        self: Pin<&mut Self>,
        route: &Route,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        let _ = route;
        self.poll_ready(cx)
    }
}

pub trait ReadySome<Route, Msg>: FlushRoute<Route, Msg> {
    fn poll_ready_some(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Route, Self::Error>>;
}

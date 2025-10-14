#![no_std]
#![cfg_attr(docsrs, doc(cfg_hide(doc)))]

use core::{
    pin::Pin,
    task::{Context, Poll},
};

use futures_sink::Sink;

mod core_impls;
#[cfg(feature = "futures-util")]
mod futures_util_impls;
#[cfg(feature = "std")]
mod std_impls;

// https://docs.rs/futures-sink/0.3.31/src/futures_sink/lib.rs.html#51
#[must_use = "sinks do nothing unless polled"]
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

// https://docs.rs/futures-sink/0.3.31/src/futures_sink/lib.rs.html#51
#[must_use = "sinks do nothing unless polled"]
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

// https://docs.rs/futures-sink/0.3.31/src/futures_sink/lib.rs.html#51
#[must_use = "sinks do nothing unless polled"]
pub trait ReadySome<Route, Msg>: FlushRoute<Route, Msg> {
    fn poll_ready_some(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Route, Self::Error>>;
}

use core::{
    pin::Pin,
    task::{Context, Poll},
};

use futures_sink::Sink;

pub trait RouteSink<Route, Msg> {
    type Error;

    fn poll_ready(
        self: Pin<&mut Self>,
        route: &Route,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>>;

    fn start_send(self: Pin<&mut Self>, route: Route, msg: Msg) -> Result<(), Self::Error>;

    fn poll_flush(
        self: Pin<&mut Self>,
        route: &Route,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>>;

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>>;
}

impl<Route, Msg, E, T: Sink<(Route, Msg), Error = E>> RouteSink<Route, Msg> for T {
    type Error = E;

    fn poll_ready(
        self: Pin<&mut Self>,
        _: &Route,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Sink::poll_ready(self, cx)
    }

    fn start_send(self: Pin<&mut Self>, route: Route, msg: Msg) -> Result<(), Self::Error> {
        Sink::start_send(self, (route, msg))
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        _: &Route,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Sink::poll_flush(self, cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Sink::poll_close(self, cx)
    }
}

pub struct WithRoute<'a, T: ?Sized, Route> {
    route_sink: Pin<&'a mut T>,
    route: Route,
}

impl<'a, T: ?Sized, Route> Unpin for WithRoute<'a, T, Route> {}

impl<'a, Route: Clone, Msg, E, T: ?Sized + RouteSink<Route, Msg, Error = E>> Sink<Msg>
    for WithRoute<'a, T, Route>
{
    type Error = E;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        this.route_sink.as_mut().poll_ready(&this.route, cx)
    }

    fn start_send(self: Pin<&mut Self>, msg: Msg) -> Result<(), Self::Error> {
        let this = self.get_mut();
        this.route_sink.as_mut().start_send(this.route.clone(), msg)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        this.route_sink.as_mut().poll_flush(&this.route, cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.get_mut().route_sink.as_mut().poll_close(cx)
    }
}

pub trait RouteExt<Route> {
    type T: ?Sized;

    fn route(&mut self, route: Route) -> WithRoute<'_, Self::T, Route>;
}

impl<T: ?Sized, Route> RouteExt<Route> for Pin<&'_ mut T> {
    type T = T;

    fn route(&mut self, route: Route) -> WithRoute<'_, Self::T, Route> {
        WithRoute {
            route_sink: self.as_mut(),
            route,
        }
    }
}

impl<T: ?Sized + Unpin, Route> RouteExt<Route> for &'_ mut T {
    type T = T;

    fn route(&mut self, route: Route) -> WithRoute<'_, Self::T, Route> {
        WithRoute {
            route_sink: Pin::new(self),
            route,
        }
    }
}

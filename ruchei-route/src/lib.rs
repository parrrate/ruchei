//! [`Sink`]s with routing inspired by ZeroMQ's ROUTER sockets.
//!
//! This model sits somewhere between explicit connection management and ZMQ-like routing trying to
//! be a reasonable abstraction around both, with some trade-offs.

use core::{
    pin::Pin,
    task::{Context, Poll},
};

use futures_sink::Sink;

/// [`Sink`] with `route` provided to some methods.
///
/// This trait is considered weaker than [`Sink<Route, Msg>`] (thus is blanked-implemented for it).
///
/// Unless specified otherwise by the implementor, all operations on [`RouteSink`] for all routes
/// (and [`poll_close`]) are considered invalid once an error occurs.
///
/// [`poll_close`]: RouteSink::poll_close
pub trait RouteSink<Route, Msg> {
    /// The type of value produced by the sink when an error occurs.
    ///
    /// See also: [`Sink::Error`]
    type Error;

    /// Attempts to prepare the [`RouteSink`] to receive a message on a specified route.
    ///
    /// Must return `Poll::Ready(Ok(()))` before each call to [`start_send`] on the same route.
    ///
    /// See also: [`Sink::poll_ready`]
    ///
    /// [`start_send`]: RouteSink::start_send
    fn poll_ready(
        self: Pin<&mut Self>,
        route: &Route,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>>;

    /// Begin the process of sending a message to the [`RouteSink`] on a specified route.
    ///
    /// Each call to this method must be preceeded by [`poll_ready`] returning `Poll::Ready(Ok(()))`
    /// on the same route.
    ///
    /// May or may not trigger the actual sending process. To guarantee the delivery, use either
    /// [`poll_flush`] on the same route, or [`poll_close`] on the whole sink.
    ///
    /// See also: [`Sink::start_send`]
    ///
    /// [`poll_ready`]: RouteSink::poll_ready
    /// [`poll_flush`]: RouteSink::poll_flush
    /// [`poll_close`]: RouteSink::poll_close
    fn start_send(self: Pin<&mut Self>, route: Route, msg: Msg) -> Result<(), Self::Error>;

    /// Flush all the remaining items on a specified route.
    ///
    /// Returns `Poll::Ready(Ok(()))` once all items sent via [`start_send`] on that route have been
    /// flushed from the buffer to the underlying transport.
    ///
    /// See also: [`Sink::poll_flush`]
    ///
    /// [`start_send`]: RouteSink::start_send
    fn poll_flush(
        self: Pin<&mut Self>,
        route: &Route,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>>;

    /// Flush and close all the routes.
    ///
    /// All other operations on the sink are considered invalid as soon as it has been polled for
    /// close for the first time
    ///
    /// [`RouteSink`] doesn't provide a method to close a specific route yet, since most interfaces,
    /// that we've looked into, don't. It might be added later on with an empty (returning
    /// `Poll::Ready(Ok(()))`) implementation (as not to break compatibility). If you really need
    /// that type of control, it is recommended to use message transports that do explicit sessions
    /// or connections, like TCP or WebSocket.
    ///
    /// See also: [`Sink::poll_close`]
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>>;

    /// Whether this [`RouteSink`] itself is doing any actual routing and keeping wakers separate.
    ///
    /// The value must be constant for the whole lifetime of the sink. It is defined on `&self`
    /// instead of the type itself to keep the trait object-safe.
    ///
    /// If the value is `false`, it should be taken as [`poll_ready`] and [`poll_flush`] being
    /// route-unaware and the whole thing acting like a regular [`Sink`]. This is important for
    /// systems that normally assume each ready/flushed route state to be independent (thus not
    /// polling other routes once one of them is done, which leaves them unprocessed forever).
    ///
    /// [`poll_ready`]: RouteSink::poll_ready
    /// [`poll_flush`]: RouteSink::poll_flush
    fn is_routing(&self) -> bool {
        true
    }
}

impl<Route, Msg, E, T: ?Sized + Sink<(Route, Msg), Error = E>> RouteSink<Route, Msg> for T {
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

    fn is_routing(&self) -> bool {
        false
    }
}

/// [`Sink`] for [`RouteSink`] and a specific route.
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

/// Extension trait to make [`WithRoute`] from some mutable (pinned) reference to [`RouteSink`].
pub trait RouteExt<Route> {
    /// [`RouteSink`] being mutably referred to.
    type T: ?Sized;

    /// Create a route-specific [`Sink`].
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

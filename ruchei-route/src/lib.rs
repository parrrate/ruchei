//! [`Sink`]s with routing inspired by ZeroMQ's `ROUTER` sockets.
//!
//! This model sits somewhere between explicit connection management and ZMQ-like routing trying to
//! be a reasonable abstraction around both, with some trade-offs.
//!
//! ## [`Sink`]s
//!
//! - [`RouteSink::poll_ready_any`]
//! - [`RouteSink::start_send`]
//! - [`RouteSink::poll_flush_all`]
//! - [`RouteSink::poll_close`]
//!
//! ## Proper [`RouteSink`]s
//!
//! - [`RouteSink::poll_ready`]
//! - [`RouteSink::start_send`]
//! - [`RouteSink::poll_flush`]
//! - [`RouteSink::poll_close`]
//!
//! ## [`RouteSink`] as a trait union
//!
//! - [`RouteSink::is_routing`]
//!
//! ## Dynamicity
//!
//! ### Object safety
//!
//! [`RouteSink`] method is object safe.
//!
//! *However*, we don't provide any methods to upcast to [`Sink`], since we believe plain [`Sink`]
//! and [`RouteSink`] traits don't represent our target usecases, *specifically networking*, which
//! involve *`Stream`s* as a necessary component of the object. Since this crate doesn't depend on
//! `futures-core`, we don't provide `dyn Stream + ...` either. Another consideration is to provide
//! mechanisms for creating upcasting instead of providing upcasting itself, to, for example, allow
//! for a more efficient FFI functionality.
//!
//! ### FFI
//!
//! ***Coming Soon...***

use core::{
    pin::Pin,
    task::{Context, Poll},
};

use futures_sink::Sink;

/// [`Sink`] with `route` provided to some methods.
///
/// This trait is considered weaker than [`Sink<(Route, Msg)>`] (thus is blanked-implemented for it).
///
/// Unless specified otherwise by the implementor, all operations on [`RouteSink`] for all routes
/// (and all route-independent fallible operations) are considered invalid once an error occurs.
///
/// ## For [`Sink`]s
///
/// This trait is implemented for all [`Sink<(Route, Msg)>`]s:
///
/// - [`RouteSink::Error`] forwards [`Sink::Error`]
/// - [`RouteSink::poll_ready`] discards the route and calls [`RouteSink::poll_ready_any`]
/// - [`RouteSink::poll_ready_any`] forwards [`Sink::poll_ready`]
/// - [`RouteSink::start_send`] forwards [`Sink::start_send`]
/// - [`RouteSink::poll_flush`] discards the route and calls [`RouteSink::poll_flush_all`]
/// - [`RouteSink::poll_flush_all`] forwards [`Sink::poll_flush`]
/// - [`RouteSink::poll_close`] forwards [`Sink::poll_close`]
/// - [`RouteSink::is_routing`] returns `false`
///
/// **NOTE**: this arrangement conflates wakers from separate routes as one, potentially leading to
/// losing track of them. Check for [`is_routing`] to check if the implementation comes from a
/// [`Sink`] (in which case [`is_routing`] is `false`).
///
/// [`is_routing`]: RouteSink::is_routing
/// [`Sink<(Route, Msg)>`]: Sink
pub trait RouteSink<Route, Msg> {
    /// The type of value produced by the sink when an error occurs.
    ///
    /// See also: [`Sink::Error`] ([forwarded][RouteSink#for-sinks] by this associated type)
    type Error;

    /// Attempts to prepare the [`RouteSink`] to receive a message on a specified route.
    ///
    /// Must return `Poll::Ready(Ok(()))` before each call to [`start_send`] on the same route.
    ///
    /// See also: [`Sink::poll_ready`], [`RouteSink::poll_ready_any`]
    ///
    /// [`start_send`]: RouteSink::start_send
    fn poll_ready(
        self: Pin<&mut Self>,
        route: &Route,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>>;

    /// Attempt to prepare the [`RouteSink`] to receive a message on an arbitrary route.
    ///
    /// Forever pending by default. Must be implemented when [`is_routing`] returns `false`.
    ///
    /// If this returns `Poll::Ready(Ok(()))`, call to [`start_send`] should be as correct as after
    /// [`poll_ready`] with a specific route.
    ///
    /// See also: [`Sink::poll_ready`] ([forwarded][RouteSink#for-sinks] by this method),
    /// [`RouteSink::poll_ready`]
    ///
    /// [`start_send`]: RouteSink::start_send
    /// [`poll_ready`]: RouteSink::poll_ready
    /// [`is_routing`]: RouteSink::is_routing
    fn poll_ready_any(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let _ = cx;
        Poll::Pending
    }

    /// Begin the process of sending a message to the [`RouteSink`] on a specified route.
    ///
    /// Each call to this method must be preceeded by [`poll_ready`] returning `Poll::Ready(Ok(()))`
    /// on the same route (or [`poll_ready_any`] without a route).
    ///
    /// May or may not trigger the actual sending process. To guarantee the delivery, use either
    /// [`poll_flush`] on the same route, [`poll_flush_all`] if supported, or [`poll_close`] on the
    /// whole sink.
    ///
    /// See also: [`Sink::start_send`] ([forwarded][RouteSink#for-sinks] by this method)
    ///
    /// [`poll_ready`]: RouteSink::poll_ready
    /// [`poll_ready_any`]: RouteSink::poll_ready_any
    /// [`poll_flush`]: RouteSink::poll_flush
    /// [`poll_flush_all`]: RouteSink::poll_flush_all
    /// [`poll_close`]: RouteSink::poll_close
    fn start_send(self: Pin<&mut Self>, route: Route, msg: Msg) -> Result<(), Self::Error>;

    /// Flush all the remaining items on a specified route.
    ///
    /// Forever pending by default. Must be implemented when [`is_routing`] returns `false`.
    ///
    /// Returns `Poll::Ready(Ok(()))` once all items sent via [`start_send`] on that route have been
    /// flushed from the buffer to the underlying transport.
    ///
    /// See also: [`Sink::poll_flush`], [`RouteSink::poll_flush_all`]
    ///
    /// [`start_send`]: RouteSink::start_send
    /// [`is_routing`]: RouteSink::is_routing
    fn poll_flush(
        self: Pin<&mut Self>,
        route: &Route,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>>;

    /// Flush all the routes.
    ///
    /// Returns `Poll::Ready(Ok(()))` once all items sent via [`start_send`] on all routes have been
    /// flushed from the buffer to the underlying transport.
    ///
    /// See also: [`Sink::poll_flush`] ([forwarded][RouteSink#for-sinks] by this method),
    /// [`RouteSink::poll_flush`]
    ///
    /// [`start_send`]: RouteSink::start_send
    /// flushed from the buffer to the underlying transport.
    fn poll_flush_all(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let _ = cx;
        Poll::Pending
    }

    /// Flush and close all the routes.
    ///
    /// All other operations on the sink are considered invalid as soon as it has been polled for
    /// close for the first time.
    ///
    /// [`RouteSink`] doesn't provide a method to close a specific route yet, since most interfaces,
    /// that we've looked into, don't. It might be added later on with an empty (returning
    /// `Poll::Ready(Ok(()))`) implementation (as not to break compatibility). If you really need
    /// that type of control, it is recommended to use message transports that do explicit sessions
    /// or connections, like TCP, [TIPC] or WebSocket.
    ///
    /// See also: [`Sink::poll_close`] ([forwarded][RouteSink#for-sinks] by this method)
    ///
    /// [TIPC]: <https://tipc.sourceforge.net/>
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
    /// Another implication of not routing is that [`poll_ready_any`] and [`poll_flush_all`] are
    /// valid and should succeed or fail eventually.
    ///
    /// [`poll_ready`]: RouteSink::poll_ready
    /// [`poll_ready_any`]: RouteSink::poll_ready_any
    /// [`poll_flush`]: RouteSink::poll_flush
    /// [`poll_flush_all`]: RouteSink::poll_flush_all
    fn is_routing(&self) -> bool {
        true
    }
}

/// See [notes][RouteSink#for-sinks]
impl<Route, Msg, E, T: ?Sized + Sink<(Route, Msg), Error = E>> RouteSink<Route, Msg> for T {
    type Error = E;

    fn poll_ready(
        self: Pin<&mut Self>,
        _: &Route,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.poll_ready_any(cx)
    }

    fn poll_ready_any(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
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
        self.poll_flush_all(cx)
    }

    fn poll_flush_all(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
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
    #[must_use]
    fn route(&mut self, route: Route) -> WithRoute<'_, Self::T, Route>;
}

impl<T: ?Sized, Route> RouteExt<Route> for Pin<&'_ mut T> {
    type T = T;

    #[must_use]
    fn route(&mut self, route: Route) -> WithRoute<'_, Self::T, Route> {
        WithRoute {
            route_sink: self.as_mut(),
            route,
        }
    }
}

impl<T: ?Sized + Unpin, Route> RouteExt<Route> for &'_ mut T {
    type T = T;

    #[must_use]
    fn route(&mut self, route: Route) -> WithRoute<'_, Self::T, Route> {
        WithRoute {
            route_sink: Pin::new(self),
            route,
        }
    }
}

/// Assert that [`RouteSink`] is derived from [`Sink`].
#[cfg(feature = "unroute")]
#[pin_project::pin_project]
pub struct Unroute<S>(#[pin] pub S);

#[cfg(feature = "unroute")]
impl<S: futures_core::Stream> futures_core::Stream for Unroute<S> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.project().0.poll_next(cx)
    }
}

#[cfg(feature = "unroute")]
impl<Route, Msg, S: RouteSink<Route, Msg>> Sink<(Route, Msg)> for Unroute<S> {
    type Error = S::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        debug_assert!(!self.0.is_routing());
        self.project().0.poll_ready_any(cx)
    }

    fn start_send(self: Pin<&mut Self>, (route, msg): (Route, Msg)) -> Result<(), Self::Error> {
        debug_assert!(!self.0.is_routing());
        self.project().0.start_send(route, msg)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        debug_assert!(!self.0.is_routing());
        self.project().0.poll_flush_all(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().0.poll_close(cx)
    }
}

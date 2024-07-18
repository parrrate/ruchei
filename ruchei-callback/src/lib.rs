#![no_std]

//! Callbacks used in [`ruchei`] stream adapters.
//!
//! Placed in a separate crate to improve compatibility across [`ruchei`] versions, since the bigger
//! crate is still at `0.0.X`, at the time of writing, and there are no plans of changing that yet.
//!
//! [`ruchei`]: https://docs.rs/ruchei/0.0.81/ruchei/index.html

/// Act when one of inner channels gets closed instead of closing the outer channel. Used with [`multicast`].
///
/// [`multicast`]: https://docs.rs/ruchei/0.0.81/ruchei/multicast/index.html
pub trait OnClose<E>: Clone {
    /// Get notified about something getting closed (optionally with an error).
    fn on_close(&self, error: Option<E>);
}

/// Implemented only for [`Sized`] because stored.
impl<E, F: Clone + Fn(Option<E>)> OnClose<E> for F {
    fn on_close(&self, error: Option<E>) {
        self(error)
    }
}

/// Used with [`ReadCallback`].
///
/// [`ReadCallback`]: https://docs.rs/ruchei/0.0.81/ruchei/read_callback/index.html
pub trait OnItem<T> {
    /// Receive an item.
    fn on_item(&self, message: T);
}

/// Implemented only for [`Sized`] because stored.
impl<T, F: Fn(T)> OnItem<T> for F {
    fn on_item(&self, message: T) {
        self(message)
    }
}

/// Start a timer [`Future`]. Used with [`TimeoutUnused`].
///
/// [`Future`]: core::future::Future
/// [`TimeoutUnused`]: https://docs.rs/ruchei/0.0.81/ruchei/timeout_unused/index.html
pub trait Start {
    /// Future created by this factory.
    type Fut;

    /// Create the future (start a timer).
    #[must_use]
    fn make(&mut self) -> Self::Fut;
}

/// Implemented only for [`Sized`] because stored.
impl<Fut, F: FnMut() -> Fut> Start for F {
    type Fut = Fut;

    #[must_use]
    fn make(&mut self) -> Self::Fut {
        self()
    }
}

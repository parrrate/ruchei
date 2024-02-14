//! Callbacks used in stream adapters.

/// Act when one of inner channels gets closed instead of closing the outer channel. Used with [`multicast`].
///
/// [`multicast`]: crate::multicast
pub trait OnClose<E>: Clone {
    fn on_close(&self, error: Option<E>);
}

impl<E, F: Clone + Fn(Option<E>)> OnClose<E> for F {
    fn on_close(&self, error: Option<E>) {
        self(error)
    }
}

/// Used with [`ReadCallback`].
///
/// [`ReadCallback`]: crate::read_callback::ReadCallback
pub trait OnItem<T> {
    fn on_item(&self, message: T);
}

impl<T, F: Fn(T)> OnItem<T> for F {
    fn on_item(&self, message: T) {
        self(message)
    }
}

/// Start a timer [`Future`]. Used with [`TimeoutUnused`].
///
/// [`Future`]: std::future::Future
/// [`TimeoutUnused`]: crate::timeout_unused::TimeoutUnused
pub trait Start {
    type Fut;

    fn make(&mut self) -> Self::Fut;
}

impl<Fut, F: ?Sized + FnMut() -> Fut> Start for F {
    type Fut = Fut;

    fn make(&mut self) -> Self::Fut {
        self()
    }
}

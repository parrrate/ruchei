//! See [`ruchei_route`].

use std::hash::Hash;

pub use ruchei_route::{RouteExt, RouteSink, Unroute, WithRoute};

pub mod keyed;

/// Helper trait for something that can be used as a key in [`Router`].
pub trait Key: 'static + Send + Sync + Clone + Hash + Ord {}

impl<K: 'static + Send + Sync + Clone + Hash + Ord> Key for K {}

#[deprecated]
pub type Router<K, S, F> = keyed::Router<K, S, F>;

#[deprecated]
pub type RouterExtending<F, R> = keyed::RouterExtending<F, R>;

#[deprecated]
pub trait RouterExt: keyed::RouterKeyedExt {
    #[must_use]
    #[deprecated]
    fn route<F: crate::callback::OnClose<Self::E>>(
        self,
        callback: F,
    ) -> keyed::RouterExtending<F, Self>;
}

#[allow(deprecated)]
impl<R: keyed::RouterKeyedExt> RouterExt for R {
    #[must_use]
    fn route<F: crate::callback::OnClose<Self::E>>(
        self,
        callback: F,
    ) -> keyed::RouterExtending<F, Self> {
        keyed::RouterKeyedExt::route_keyed(self, callback)
    }
}

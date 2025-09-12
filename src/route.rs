//! See [`ruchei_route`].

use std::hash::Hash;

pub use ruchei_route::{RouteExt, RouteSink, Unroute, WithRoute};

pub mod keyed;
pub mod slab;

/// Helper trait for something that can be used as a key in [`Router`].
pub trait Key: 'static + Send + Sync + Clone + Hash + Ord {}

impl<K: 'static + Send + Sync + Clone + Hash + Ord> Key for K {}

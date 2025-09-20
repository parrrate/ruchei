use std::hash::Hash;

#[cfg(feature = "by-key")]
#[cfg(all(feature = "connection", feature = "extend"))]
pub mod by_key;
#[cfg(all(feature = "connection", feature = "extend"))]
pub mod multicast;
#[cfg(all(feature = "connection", feature = "extend"))]
pub mod without_multicast;

/// Helper trait for something that can be used as a key in `Router`s.
pub trait Key: 'static + Send + Sync + Clone + Hash + Ord {}

impl<K: 'static + Send + Sync + Clone + Hash + Ord> Key for K {}

use std::hash::Hash;

pub mod keyed;
pub mod slab;
pub mod slab_multicast;

/// Helper trait for something that can be used as a key in [`Router`].
pub trait Key: 'static + Send + Sync + Clone + Hash + Ord {}

impl<K: 'static + Send + Sync + Clone + Hash + Ord> Key for K {}

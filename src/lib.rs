//! Utilities for working with many streams

#![forbid(unsafe_code)]

extern crate self as ruchei;

#[cfg(feature = "unstable")]
pub use ruchei_callback as callback;
pub use ruchei_extend as extend;
pub use ruchei_extra as with_extra;

#[cfg(feature = "unstable")]
pub mod compress;
pub mod concurrent;
pub mod connection_item;
pub mod deal;
pub mod echo;
#[cfg(feature = "unstable")]
pub mod group_sequential;
#[cfg(feature = "unstable")]
pub mod liveness;
pub mod merge;
pub mod multicast;
mod owned_close;
pub mod poll_on_wake;
pub mod route;
pub mod use_latest;
pub mod with_reply;

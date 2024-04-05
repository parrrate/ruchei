//! Utilities for working with many streams

#![forbid(unsafe_code)]

extern crate self as ruchei;

pub use ruchei_callback as callback;
pub use ruchei_extra as with_extra;

pub mod close_all;
pub mod compress;
pub mod concurrent;
pub mod echo;
pub mod group_by_key;
pub mod multicast;
mod owned_close;
pub mod pinned_extend;
pub mod poll_on_wake;
pub mod read_callback;
pub mod route;
pub mod rw_isolation;
pub mod switching;
pub mod timeout_unused;
mod wait_all;

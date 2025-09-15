//! Utilities for working with many streams

#![forbid(unsafe_code)]

extern crate self as ruchei;

pub use ruchei_callback as callback;
pub use ruchei_extra as with_extra;

pub mod close_all;
pub mod compress;
pub mod concurrent;
pub mod deal;
pub mod echo;
pub mod group_concurrent;
pub mod group_sequential;
pub mod merge;
pub mod multi_item;
pub mod multicast;
mod owned_close;
pub mod pinned_extend;
pub mod poll_on_wake;
mod ready_slab;
pub mod reply_buffer;
pub mod route;
pub mod switching;
pub mod timeout_unused;

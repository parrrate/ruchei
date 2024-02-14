//! Utilities for working with many streams

#![deny(unsafe_code)]

extern crate self as ruchei;

pub mod callback;
pub mod concurrent;
pub mod echo;
pub mod group_by_key;
pub mod multicast;
mod owned_close;
pub mod pinned_extend;
pub mod poll_on_wake;
pub mod read_callback;
pub mod rw_isolation;
pub mod timeout_unused;
mod wait_all;
pub mod with_extra;

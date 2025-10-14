//! Utilities for working with many streams

#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(docsrs, doc(cfg_hide(doc)))]

extern crate self as ruchei;

#[cfg(feature = "unstable")]
pub use ruchei_callback as callback;
#[cfg(feature = "extend")]
pub use ruchei_extend as extend;
#[cfg(feature = "extra")]
pub use ruchei_extra as with_extra;

#[cfg(feature = "unstable")]
pub mod compress;
pub mod concurrent;
pub mod connection_item;
#[cfg(all(feature = "connection", feature = "extend"))]
pub mod deal;
pub mod echo;
#[cfg(feature = "unstable")]
pub mod group_sequential;
#[cfg(feature = "unstable")]
pub mod liveness;
pub mod merge;
#[cfg(all(feature = "connection", feature = "extend"))]
pub mod multicast;
pub mod poll_on_wake;
#[cfg(feature = "connection")]
pub mod route;
pub mod use_latest;
pub mod with_reply;

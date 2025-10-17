//! [`Sink`]s with round-robin distribution inspired by ZeroMQ's `DEALER` sockets.
//!
//! [`Sink`]: futures_util::Sink

pub mod route;
pub mod without_multicast;

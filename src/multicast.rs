//! Convert a [`Stream`] of [`Stream`]`+`[`Sink`]s into single [`Stream`]`+`[`Sink`].
//!
//! [`Stream`]: futures_util::Stream
//! [`Sink`]: futures_util::Sink

pub mod buffered;
#[cfg(feature = "unstable")]
pub mod buffered_wakelist;
pub mod bufferless;
#[cfg(feature = "unstable")]
pub mod bufferless_wakelist;
pub mod replay;
#[cfg(feature = "unstable")]
pub mod trie;

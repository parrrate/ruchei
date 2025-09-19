//! Convert a [`Stream`] of [`Stream`]`+`[`Sink`]s into single [`Stream`]`+`[`Sink`].
//!
//! [`Stream`]: futures_util::Stream
//! [`Sink`]: futures_util::Sink

pub mod buffered;
pub mod bufferless;
pub mod replay;
pub mod trie;

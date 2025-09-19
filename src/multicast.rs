//! Convert a [`Stream`] of [`Stream`]`+`[`Sink`]s into single [`Stream`]`+`[`Sink`].
//!
//! [`Stream`]: futures_util::Stream
//! [`Sink`]: futures_util::Sink

pub mod buffered_slab;
pub mod bufferless_slab;
pub mod replay_slab;
pub mod trie;

//! Convert a [`Stream`] of [`Stream`]`+`[`Sink`]s into single [`Stream`]`+`[`Sink`].
//!
//! ## A Warning
//!
//! All multicasting strategies in this module require [polling for `next`]
//! to make [`send`] take any effect.
//!
//! Available ways to avoid that:
//!
//! * [`ruchei::read_callback`]
//! * [`ruchei::rw_isolation`]
//!
//! [`Stream`]: futures_util::Stream
//! [`Sink`]: futures_util::Sink
//! [polling for `next`]: futures_util::Stream::poll_next
//! [`send`]: futures_util::Sink::start_send

pub mod buffered_slab;
pub mod bufferless;
pub mod bufferless_slab;
pub mod ignore;
pub mod replay;
pub mod replay_slab;
pub mod trie;

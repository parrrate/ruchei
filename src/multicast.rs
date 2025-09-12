//! Convert a `Stream` of `Stream+Sink`s into single `Stream+Sink`.
//!
//! ## A Warning
//!
//! All multicasting strategies in this module require polling for `next` to make `send` take any effect.
//!
//! Available ways to avoid that:
//!
//! * [`ruchei::read_callback`]
//! * [`ruchei::rw_isolation`]

pub mod buffered;
pub mod bufferless;
pub mod replay;

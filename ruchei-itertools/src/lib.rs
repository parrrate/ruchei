//! once this is somewhat useful and correct, this might get renamed to `async-itertools`
//!
//! or it might not, since we want `route-sink` passthrough, and that's `ruchei`-specific

#![no_std]

use futures_core::Stream;

use self::dedup_eager::DedupEager;

mod dedup_eager;
mod macros;

pub trait AsyncItertools: Stream {
    fn dedup_eager(self) -> DedupEager<Self>
    where
        Self: Sized,
        Self::Item: PartialEq + Clone,
    {
        DedupEager::new(self)
    }
}

impl<T: ?Sized + Stream> AsyncItertools for T {}

//! once it is somewhat useful and correct, this might get renamed to `async-itertools`
//!
//! or it might not, since we want `route-sink` passthrough, and that's `ruchei`-specific

#![no_std]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(docsrs, doc(cfg_hide(doc)))]

use futures_util::Stream;

pub use self::{
    dedup_eager::DedupEager,
    interleave::{Interleave, interleave},
};

mod check;
mod dedup_eager;
mod interleave;
mod macros;

/// `Itertools` for `Stream`s
///
/// most combinators also forward [`futures-sink`] and [`route-sink`] methods
///
/// [`futures-sink`]: https://docs.rs/futures-sink/0.3
/// [`route-sink`]: https://docs.rs/route-sink/0.1
pub trait AsyncItertools: Stream {
    fn interleave<J>(self, other: J) -> Interleave<Self, J>
    where
        Self: Sized,
        J: Stream<Item = Self::Item>,
    {
        interleave(self, other)
    }

    /// deduplicates items, and yields them *as soon as they become available* (that's why `Clone`)
    fn dedup_eager(self) -> DedupEager<Self>
    where
        Self: Sized,
        Self::Item: PartialEq + Clone,
    {
        DedupEager::new(self)
    }
}

impl<T: ?Sized + Stream> AsyncItertools for T {}

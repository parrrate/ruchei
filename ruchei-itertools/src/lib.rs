//! once it is somewhat useful and correct, this might get renamed to `async-itertools`
//!
//! or it might not, since we want `route-sink` passthrough, and that's `ruchei`-specific

#![no_std]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(docsrs, doc(cfg_hide(doc)))]

use futures_util::Stream;

use self::check::{assert_future, assert_stream};
pub use self::interleave::interleave;

mod advance_by;
mod all_equal;
mod check;
mod dedup_eager;
mod interleave;
mod macros;

pub type AdvanceBy<'a, S> = self::advance_by::AdvanceBy<'a, S>;
pub type AllEqual<S> = self::all_equal::AllEqual<S>;
pub type DedupEager<I> = self::dedup_eager::DedupEager<I>;
pub type Interleave<I, J> = self::interleave::Interleave<I, J>;

/// `Itertools` for `Stream`s
///
/// most combinators also forward [`futures-sink`] and [`route-sink`] methods
///
/// [`futures-sink`]: https://docs.rs/futures-sink/0.3
/// [`route-sink`]: https://docs.rs/route-sink/0.1
pub trait AsyncItertools: Stream {
    fn advance_by(&mut self, n: usize) -> AdvanceBy<'_, Self>
    where
        Self: Unpin,
    {
        assert_future(AdvanceBy {
            stream: self,
            remaining: n,
        })
    }

    fn all_equal(self) -> AllEqual<Self>
    where
        Self: Sized,
        Self::Item: PartialEq,
    {
        assert_future(AllEqual {
            stream: self,
            first: None,
        })
    }

    /// deduplicates items, and yields them *as soon as they become available* (that's why `Clone`)
    fn dedup_eager(self) -> DedupEager<Self>
    where
        Self: Sized,
        Self::Item: PartialEq + Clone,
    {
        assert_stream(DedupEager::new(self))
    }

    fn interleave<J>(self, other: J) -> Interleave<Self, J>
    where
        Self: Sized,
        J: Stream<Item = Self::Item>,
    {
        interleave(self, other)
    }
}

impl<T: ?Sized + Stream> AsyncItertools for T {}

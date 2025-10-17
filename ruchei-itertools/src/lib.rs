//! once it is somewhat useful and correct, this might get renamed to `async-itertools`
//!
//! or it might not, since we want `route-sink` passthrough, and that's `ruchei`-specific

#![no_std]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(docsrs, doc(cfg_hide(doc)))]

#[cfg(feature = "std")]
extern crate std;

use core::cmp::Ordering;

use futures_util::Stream;
#[cfg(feature = "std")]
use futures_util::{StreamExt, stream::Collect};

use self::{
    by_fn::ByFn,
    check::{assert_future, assert_stream},
    cmp_by::{ByOrd, cmp_by},
    partial_cmp_by::{ByPartialOrd, partial_cmp_by},
};
pub use self::{either_or_both::EitherOrBoth, interleave::interleave, zip_longest::zip_longest};

mod advance_by;
mod all_equal;
mod by_fn;
mod check;
mod cmp_by;
mod dedup_eager;
mod either_or_both;
mod interleave;
mod macros;
mod partial_cmp_by;
mod zip_longest;

pub type AdvanceBy<'a, S> = self::advance_by::AdvanceBy<'a, S>;
pub type AllEqual<S> = self::all_equal::AllEqual<S>;
pub type Cmp<L, R> = self::cmp_by::CmpBy<L, R, ByOrd>;
pub type CmpBy<L, R, F> = self::cmp_by::CmpBy<L, R, ByFn<F>>;
pub type DedupEager<I> = self::dedup_eager::DedupEager<I>;
pub type Interleave<I, J> = self::interleave::Interleave<I, J>;
pub type PartialCmp<L, R> = self::partial_cmp_by::PartialCmpBy<L, R, ByPartialOrd>;
pub type PartialCmpBy<L, R, F> = self::partial_cmp_by::PartialCmpBy<L, R, ByFn<F>>;
pub type ZipLongest<L, R> = self::zip_longest::ZipLongest<L, R>;

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

    #[cfg(feature = "std")]
    fn collect_vec(self) -> Collect<Self, std::vec::Vec<Self::Item>>
    where
        Self: Sized,
    {
        self.collect()
    }

    fn cmp<J>(self, other: J) -> Cmp<Self, J>
    where
        Self: Sized,
        Self::Item: Ord,
        J: Stream<Item = Self::Item>,
    {
        cmp_by(self, other, ByOrd)
    }

    fn cmp_by<J, F>(self, other: J, cmp: F) -> CmpBy<Self, J, F>
    where
        Self: Sized,
        J: Stream,
        F: FnMut(&Self::Item, &J::Item) -> Ordering,
    {
        cmp_by(self, other, ByFn(cmp))
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

    fn partial_cmp<J>(self, other: J) -> PartialCmp<Self, J>
    where
        Self: Sized,
        Self::Item: PartialOrd<J::Item>,
        J: Stream,
    {
        partial_cmp_by(self, other, ByPartialOrd)
    }

    fn partial_cmp_by<J, F>(self, other: J, cmp: F) -> PartialCmpBy<Self, J, F>
    where
        Self: Sized,
        J: Stream,
        F: FnMut(&Self::Item, &J::Item) -> Option<Ordering>,
    {
        partial_cmp_by(self, other, ByFn(cmp))
    }

    fn zip_longest<J>(self, other: J) -> ZipLongest<Self, J>
    where
        Self: Sized,
        J: Stream,
    {
        zip_longest(self, other)
    }
}

impl<T: ?Sized + Stream> AsyncItertools for T {}

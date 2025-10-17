use core::{
    cmp::Ordering,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{Stream, ready};
use pin_project::pin_project;

use crate::{
    by_fn::ByFn,
    check::assert_future,
    zip_longest::{ZipLongest, zip_longest},
};

pub trait CmpStrategy<L, R> {
    fn cmp(&mut self, l: &L, r: &R) -> Ordering;
}

impl<L, R, F: FnMut(&L, &R) -> Ordering> CmpStrategy<L, R> for ByFn<F> {
    fn cmp(&mut self, l: &L, r: &R) -> Ordering {
        self.0(l, r)
    }
}

pub struct ByOrd;

impl<T: Ord> CmpStrategy<T, T> for ByOrd {
    fn cmp(&mut self, l: &T, r: &T) -> Ordering {
        l.cmp(r)
    }
}

#[pin_project]
pub struct CmpBy<L, R, S, Lt = <L as Stream>::Item, Rt = <R as Stream>::Item> {
    #[pin]
    zip: ZipLongest<L, R, Lt, Rt>,
    strategy: S,
}

pub(crate) fn cmp_by<L: Stream, R: Stream, S: CmpStrategy<L::Item, R::Item>>(
    l: L,
    r: R,
    strategy: S,
) -> CmpBy<L, R, S> {
    assert_future(CmpBy {
        zip: zip_longest(l, r),
        strategy,
    })
}

impl<L: Stream<Item = Lt>, R: Stream<Item = Rt>, S: CmpStrategy<Lt, Rt>, Lt, Rt> Future
    for CmpBy<L, R, S, Lt, Rt>
{
    type Output = Ordering;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        while let Some(ref item) = ready!(this.zip.as_mut().poll_next(cx)) {
            match item {
                crate::EitherOrBoth::Both(l, r) => {
                    let cmp = this.strategy.cmp(l, r);
                    if cmp.is_ne() {
                        return Poll::Ready(cmp);
                    }
                }
                crate::EitherOrBoth::Left(_) => return Poll::Ready(Ordering::Greater),
                crate::EitherOrBoth::Right(_) => return Poll::Ready(Ordering::Less),
            }
        }
        Poll::Ready(Ordering::Equal)
    }
}

#[cfg(test)]
mod test {
    use core::cmp::Ordering;

    use crate::AsyncItertools;

    #[test]
    fn left_longer() {
        let l = futures_util::stream::iter([1, 2, 3]);
        let r = futures_util::stream::iter([1, 2]);
        assert_eq!(futures_executor::block_on(l.cmp(r)), Ordering::Greater);
    }

    #[test]
    fn left_greater() {
        let l = futures_util::stream::iter([1, 2, 4]);
        let r = futures_util::stream::iter([1, 2, 3]);
        assert_eq!(futures_executor::block_on(l.cmp(r)), Ordering::Greater);
    }

    #[test]
    fn equal_length() {
        let l = futures_util::stream::iter([1, 2, 3]);
        let r = futures_util::stream::iter([1, 2, 3]);
        assert_eq!(futures_executor::block_on(l.cmp(r)), Ordering::Equal);
    }

    #[test]
    fn right_greater() {
        let l = futures_util::stream::iter([1, 2, 3]);
        let r = futures_util::stream::iter([1, 2, 4]);
        assert_eq!(futures_executor::block_on(l.cmp(r)), Ordering::Less);
    }

    #[test]
    fn right_longer() {
        let l = futures_util::stream::iter([1, 2]);
        let r = futures_util::stream::iter([1, 2, 3]);
        assert_eq!(futures_executor::block_on(l.cmp(r)), Ordering::Less);
    }
}

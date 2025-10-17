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

pub trait PartialCmpStrategy<L, R> {
    fn partial_cmp(&mut self, l: &L, r: &R) -> Option<Ordering>;
}

impl<L, R, F: FnMut(&L, &R) -> Option<Ordering>> PartialCmpStrategy<L, R> for ByFn<F> {
    fn partial_cmp(&mut self, l: &L, r: &R) -> Option<Ordering> {
        self.0(l, r)
    }
}

pub struct ByPartialOrd;

impl<L: PartialOrd<R>, R> PartialCmpStrategy<L, R> for ByPartialOrd {
    fn partial_cmp(&mut self, l: &L, r: &R) -> Option<Ordering> {
        l.partial_cmp(r)
    }
}

#[pin_project]
pub struct PartialCmpBy<L, R, S, Lt = <L as Stream>::Item, Rt = <R as Stream>::Item> {
    #[pin]
    zip: ZipLongest<L, R, Lt, Rt>,
    strategy: S,
}

pub(crate) fn partial_cmp_by<L: Stream, R: Stream, S: PartialCmpStrategy<L::Item, R::Item>>(
    l: L,
    r: R,
    strategy: S,
) -> PartialCmpBy<L, R, S> {
    assert_future(PartialCmpBy {
        zip: zip_longest(l, r),
        strategy,
    })
}

impl<L: Stream<Item = Lt>, R: Stream<Item = Rt>, S: PartialCmpStrategy<Lt, Rt>, Lt, Rt> Future
    for PartialCmpBy<L, R, S, Lt, Rt>
{
    type Output = Option<Ordering>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        while let Some(ref item) = ready!(this.zip.as_mut().poll_next(cx)) {
            match item {
                crate::EitherOrBoth::Both(l, r) => {
                    let cmp = this.strategy.partial_cmp(l, r);
                    if cmp.is_none_or(|cmp| cmp.is_ne()) {
                        return Poll::Ready(cmp);
                    }
                }
                crate::EitherOrBoth::Left(_) => return Poll::Ready(Some(Ordering::Greater)),
                crate::EitherOrBoth::Right(_) => return Poll::Ready(Some(Ordering::Less)),
            }
        }
        Poll::Ready(Some(Ordering::Equal))
    }
}

#[cfg(test)]
mod test {
    use core::cmp::Ordering;

    use crate::AsyncItertools;

    #[test]
    fn left_longer() {
        let l = futures_util::stream::iter([1.0, 2.0, 3.0]);
        let r = futures_util::stream::iter([1.0, 2.0]);
        assert_eq!(
            futures_executor::block_on(l.partial_cmp(r)),
            Some(Ordering::Greater),
        );
    }

    #[test]
    fn left_greater() {
        let l = futures_util::stream::iter([1.0, 2.0, 4.0]);
        let r = futures_util::stream::iter([1.0, 2.0, 3.0]);
        assert_eq!(
            futures_executor::block_on(l.partial_cmp(r)),
            Some(Ordering::Greater),
        );
    }

    #[test]
    fn equal_length() {
        let l = futures_util::stream::iter([1.0, 2.0, 3.0]);
        let r = futures_util::stream::iter([1.0, 2.0, 3.0]);
        assert_eq!(
            futures_executor::block_on(l.partial_cmp(r)),
            Some(Ordering::Equal),
        );
    }

    #[test]
    fn right_greater() {
        let l = futures_util::stream::iter([1.0, 2.0, 3.0]);
        let r = futures_util::stream::iter([1.0, 2.0, 4.0]);
        assert_eq!(
            futures_executor::block_on(l.partial_cmp(r)),
            Some(Ordering::Less),
        );
    }

    #[test]
    fn right_longer() {
        let l = futures_util::stream::iter([1.0, 2.0]);
        let r = futures_util::stream::iter([1.0, 2.0, 3.0]);
        assert_eq!(
            futures_executor::block_on(l.partial_cmp(r)),
            Some(Ordering::Less),
        );
    }

    #[test]
    fn nan_first() {
        let l = futures_util::stream::iter([f64::NAN, 1.0]);
        let r = futures_util::stream::iter([f64::NAN, 2.0]);
        assert_eq!(futures_executor::block_on(l.partial_cmp(r)), None);
    }

    #[test]
    fn nan_second() {
        let l = futures_util::stream::iter([1.0, f64::NAN]);
        let r = futures_util::stream::iter([2.0, f64::NAN]);
        assert_eq!(
            futures_executor::block_on(l.partial_cmp(r)),
            Some(Ordering::Less),
        );
    }
}

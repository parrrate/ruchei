use std::{
    cmp::Ordering,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{Stream, future::Either, ready};
use pin_project::pin_project;

use super::pair_item::{PairCategory, PairItem, PairStream, StreamPair};

#[pin_project]
#[derive(Debug)]
pub struct ZipGroupSorted<
    L,
    R,
    T,
    K = <(L, R) as StreamPair>::K,
    Lv = <L as PairStream>::V,
    Rv = <R as PairStream>::V,
> {
    #[pin]
    l: L,
    #[pin]
    r: R,
    #[allow(clippy::type_complexity)]
    last: Option<(K, Either<(Lv, T), Rv>)>,
}

impl<L: Default, R: Default, T, K, Lv, Rv> Default for ZipGroupSorted<L, R, T, K, Lv, Rv> {
    fn default() -> Self {
        Self {
            l: Default::default(),
            r: Default::default(),
            last: Default::default(),
        }
    }
}

impl<
    C: PairCategory,
    K: Ord,
    Lv,
    Rv,
    Li: PairItem<C = C, K = K, V = Lv>,
    Ri: PairItem<C = C, K = K, V = Rv>,
    L: PairStream<C = C, K = K, V = Lv, Item = Li>,
    R: PairStream<C = C, K = K, V = Rv, Item = Ri>,
    T: Default + Extend<Rv>,
> Stream for ZipGroupSorted<L, R, T, K, Lv, Rv>
{
    type Item = C::Pair<K, (Lv, T)>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        macro_rules! try_next {
            () => {{
                assert!(this.last.is_none());
                match this.l.as_mut().poll_next(cx) {
                    Poll::Ready(Some(kv)) => match kv.into_kv::<(Lv, T)>() {
                        Ok(kv) => Poll::Ready(kv),
                        Err(e) => {
                            return Poll::Ready(Some(e));
                        }
                    },
                    Poll::Ready(None) => {
                        return Poll::Ready(None);
                    }
                    Poll::Pending => Poll::Pending,
                }
            }};
        }
        Poll::Ready(loop {
            if let Some((k, v)) = this.last.take() {
                let (lk, lvt, rk, rv) = match v {
                    Either::Left(lvt) => {
                        let lk = k;
                        let (rk, rv) = match this.r.as_mut().poll_next(cx) {
                            Poll::Ready(Some(kv)) => match kv.into_kv::<(Lv, T)>() {
                                Ok(kv) => kv,
                                Err(e) => {
                                    return Poll::Ready(Some(e));
                                }
                            },
                            Poll::Ready(None) => {
                                break Some(PairItem::from_kv(lk, lvt));
                            }
                            Poll::Pending => {
                                *this.last = Some((lk, Either::Left(lvt)));
                                return Poll::Pending;
                            }
                        };
                        (lk, lvt, rk, rv)
                    }
                    Either::Right(rv) => {
                        let rk = k;
                        let Poll::Ready((lk, lv)) = try_next!() else {
                            *this.last = Some((rk, Either::Right(rv)));
                            return Poll::Pending;
                        };
                        let lvt = (lv, Default::default());
                        (lk, lvt, rk, rv)
                    }
                };
                match lk.cmp(&rk) {
                    Ordering::Less => {
                        *this.last = Some((rk, Either::Right(rv)));
                        break Some(PairItem::from_kv(lk, lvt));
                    }
                    Ordering::Equal => {
                        let (lv, mut t) = lvt;
                        t.extend(std::iter::once(rv));
                        *this.last = Some((lk, Either::Left((lv, t))));
                    }
                    Ordering::Greater => *this.last = Some((lk, Either::Left(lvt))),
                }
            } else {
                let (lk, lv) = ready!(try_next!());
                *this.last = Some((lk, Either::Left((lv, Default::default()))));
            }
        })
    }
}

#[must_use]
pub fn zip_group_sorted<T: Default + Extend<<R as PairStream>::V>, L: PairStream, R: PairStream>(
    l: L,
    r: R,
) -> ZipGroupSorted<L, R, T>
where
    ZipGroupSorted<L, R, T>: Stream,
    (L, R): StreamPair,
{
    ZipGroupSorted { l, r, last: None }
}

pub trait ZipGroupSortedExt: Sized + PairStream<K: Ord> {
    #[must_use]
    fn zip_group_sorted<
        T: Default + Extend<<R as PairStream>::V>,
        R: PairStream<C = Self::C, K = Self::K>,
    >(
        self,
        right: R,
    ) -> ZipGroupSorted<Self, R, T> {
        zip_group_sorted(self, right)
    }
}

impl<L: PairStream<K: Ord>> ZipGroupSortedExt for L {}

#[test]
fn simple() {
    use futures_util::StreamExt;
    let l = futures_util::stream::iter([(1, 2), (4, 5)]);
    let r = futures_util::stream::iter([(1, 3), (4, 6)]);
    let s = async_io::block_on(l.zip_group_sorted(r).collect::<Vec<_>>());
    assert_eq!(s, [(1, (2, vec![3])), (4, (5, vec![6]))]);
}

#[test]
fn l_extra() {
    use futures_util::StreamExt;
    let l = futures_util::stream::iter([(1, 2), (3, 3), (4, 5)]);
    let r = futures_util::stream::iter([(1, 3), (4, 6)]);
    let s = async_io::block_on(l.zip_group_sorted(r).collect::<Vec<_>>());
    assert_eq!(s, [(1, (2, vec![3])), (3, (3, vec![])), (4, (5, vec![6]))]);
}

#[test]
fn r_extra() {
    use futures_util::StreamExt;
    let l = futures_util::stream::iter([(1, 2), (4, 5)]);
    let r = futures_util::stream::iter([(1, 3), (3, 3), (4, 6)]);
    let s = async_io::block_on(l.zip_group_sorted(r).collect::<Vec<_>>());
    assert_eq!(s, [(1, (2, vec![3])), (4, (5, vec![6]))]);
}

#[test]
fn duplicate_key() {
    use futures_util::StreamExt;
    let l = futures_util::stream::iter([(1, 2), (1, 4)]);
    let r = futures_util::stream::iter([(1, 3), (1, 5)]);
    let s = async_io::block_on(l.zip_group_sorted(r).collect::<Vec<_>>());
    assert_eq!(s, [(1, (2, vec![3, 5])), (1, (4, vec![]))]);
}

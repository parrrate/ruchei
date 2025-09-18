use std::{
    cmp::Ordering,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{Stream, future::Either, ready};
use pin_project::pin_project;

use super::pair_item::{PairCategory, PairItem, PairStream, StreamPair};

#[pin_project]
pub struct ZipSorted<
    L,
    R,
    K = <(L, R) as StreamPair>::K,
    Lv = <L as PairStream>::V,
    Rv = <R as PairStream>::V,
> {
    #[pin]
    l: L,
    #[pin]
    r: R,
    last: Option<(K, Either<Lv, Rv>)>,
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
> Stream for ZipSorted<L, R, K, Lv, Rv>
{
    type Item = C::Pair<K, (Lv, Rv)>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        Poll::Ready(loop {
            if let Some((k, v)) = this.last.take() {
                let (lk, lv, rk, rv) = match v {
                    Either::Left(lv) => {
                        let lk = k;
                        let Poll::Ready(rkv) = this.r.as_mut().poll_next(cx) else {
                            *this.last = Some((lk, Either::Left(lv)));
                            return Poll::Pending;
                        };
                        let Some(rkv) = rkv else {
                            break None;
                        };
                        let (rk, rv) = match rkv.into_kv::<(Lv, Rv)>() {
                            Ok(rkv) => rkv,
                            Err(e) => break Some(e),
                        };
                        (lk, lv, rk, rv)
                    }
                    Either::Right(rv) => {
                        let rk = k;
                        let Poll::Ready(lkv) = this.l.as_mut().poll_next(cx) else {
                            *this.last = Some((rk, Either::Right(rv)));
                            return Poll::Pending;
                        };
                        let Some(lkv) = lkv else {
                            break None;
                        };
                        let (lk, lv) = match lkv.into_kv::<(Lv, Rv)>() {
                            Ok(lkv) => lkv,
                            Err(e) => break Some(e),
                        };
                        (lk, lv, rk, rv)
                    }
                };
                match lk.cmp(&rk) {
                    Ordering::Less => *this.last = Some((rk, Either::Right(rv))),
                    Ordering::Equal => break Some(PairItem::from_kv(lk, (lv, rv))),
                    Ordering::Greater => *this.last = Some((lk, Either::Left(lv))),
                }
            } else {
                let Some(lkv) = ready!(this.l.as_mut().poll_next(cx)) else {
                    break None;
                };
                match lkv.into_kv::<(Lv, Rv)>() {
                    Ok((lk, lv)) => *this.last = Some((lk, Either::Left(lv))),
                    Err(e) => break Some(e),
                }
            }
        })
    }
}

pub fn zip_sorted<L: PairStream, R: PairStream>(l: L, r: R) -> ZipSorted<L, R>
where
    ZipSorted<L, R>: Stream,
    (L, R): StreamPair,
{
    ZipSorted { l, r, last: None }
}

pub trait ZipSortedExt: Sized + PairStream<K: Ord> {
    fn zip_sorted<R: PairStream<C = Self::C, K = Self::K>>(self, right: R) -> ZipSorted<Self, R> {
        zip_sorted(self, right)
    }
}

impl<L: PairStream<K: Ord>> ZipSortedExt for L {}

#[test]
fn simple() {
    use futures_util::StreamExt;
    let l = futures_util::stream::iter([(1, 2), (4, 5)]);
    let r = futures_util::stream::iter([(1, 3), (4, 6)]);
    let s = async_io::block_on(l.zip_sorted(r).collect::<Vec<_>>());
    assert_eq!(s, [(1, (2, 3)), (4, (5, 6))]);
}

#[test]
fn l_extra() {
    use futures_util::StreamExt;
    let l = futures_util::stream::iter([(1, 2), (3, 3), (4, 5)]);
    let r = futures_util::stream::iter([(1, 3), (4, 6)]);
    let s = async_io::block_on(l.zip_sorted(r).collect::<Vec<_>>());
    assert_eq!(s, [(1, (2, 3)), (4, (5, 6))]);
}

#[test]
fn r_extra() {
    use futures_util::StreamExt;
    let l = futures_util::stream::iter([(1, 2), (4, 5)]);
    let r = futures_util::stream::iter([(1, 3), (3, 3), (4, 6)]);
    let s = async_io::block_on(l.zip_sorted(r).collect::<Vec<_>>());
    assert_eq!(s, [(1, (2, 3)), (4, (5, 6))]);
}

#[test]
fn duplicate_key() {
    use futures_util::StreamExt;
    let l = futures_util::stream::iter([(1, 2), (1, 4)]);
    let r = futures_util::stream::iter([(1, 3), (1, 5)]);
    let s = async_io::block_on(l.zip_sorted(r).collect::<Vec<_>>());
    assert_eq!(s, [(1, (2, 3)), (1, (4, 5))]);
}

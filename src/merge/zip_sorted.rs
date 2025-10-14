use std::{
    cmp::Ordering,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{Stream, future::Either};
use pin_project::pin_project;

use super::pair_item::{PairCategory, PairItem, PairStream, StreamPair};

#[pin_project]
#[derive(Debug)]
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

impl<L: Default, R: Default, K, Lv, Rv> Default for ZipSorted<L, R, K, Lv, Rv> {
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
> Stream for ZipSorted<L, R, K, Lv, Rv>
{
    type Item = C::Pair<K, (Lv, Rv)>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        macro_rules! try_next {
            ($s:ident, $on_pending:expr) => {{
                assert!(this.last.is_none());
                match this.$s.as_mut().poll_next(cx) {
                    Poll::Ready(Some(kv)) => match kv.into_kv::<(Lv, Rv)>() {
                        Ok(kv) => kv,
                        Err(e) => {
                            return Poll::Ready(Some(e));
                        }
                    },
                    Poll::Ready(None) => {
                        return Poll::Ready(None);
                    }
                    Poll::Pending => {
                        $on_pending;
                        return Poll::Pending;
                    }
                }
            }};
        }
        Poll::Ready(loop {
            if let Some((k, v)) = this.last.take() {
                let (lk, lv, rk, rv) = match v {
                    Either::Left(lv) => {
                        let lk = k;
                        let (rk, rv) = try_next!(r, {
                            *this.last = Some((lk, Either::Left(lv)));
                        });
                        (lk, lv, rk, rv)
                    }
                    Either::Right(rv) => {
                        let rk = k;
                        let (lk, lv) = try_next!(l, {
                            *this.last = Some((rk, Either::Right(rv)));
                        });
                        (lk, lv, rk, rv)
                    }
                };
                match lk.cmp(&rk) {
                    Ordering::Less => *this.last = Some((rk, Either::Right(rv))),
                    Ordering::Equal => break Some(PairItem::from_kv(lk, (lv, rv))),
                    Ordering::Greater => *this.last = Some((lk, Either::Left(lv))),
                }
            } else {
                let (lk, lv) = try_next!(l, {});
                *this.last = Some((lk, Either::Left(lv)));
            }
        })
    }
}

#[must_use]
pub fn zip_sorted<L: PairStream, R: PairStream>(l: L, r: R) -> ZipSorted<L, R>
where
    ZipSorted<L, R>: Stream,
    (L, R): StreamPair,
{
    ZipSorted { l, r, last: None }
}

pub trait ZipSortedExt: Sized + PairStream<K: Ord> {
    #[must_use]
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

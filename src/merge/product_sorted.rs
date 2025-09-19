use std::{
    cmp::Ordering,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{Stream, future::Either, ready};
use pin_project::pin_project;

use super::pair_item::{PairCategory, PairItem, PairStream, StreamPair};

type Yielded = Either<usize, usize>;

type Next<K, Lv, Rv> = Either<Option<(K, Lv)>, Option<(K, Rv)>>;

type Last<K, Lv, Rv> = (K, Option<Yielded>, Option<Next<K, Lv, Rv>>);

#[pin_project]
#[derive(Debug)]
pub struct ProductSorted<
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
    /// second is less than the related `Vec` (says which one's behind)
    last: Option<Last<K, Lv, Rv>>,
    lv: Vec<Lv>,
    rv: Vec<Rv>,
}

impl<
    C: PairCategory,
    K: Ord + Clone,
    Lv: Clone,
    Rv: Clone,
    Li: PairItem<C = C, K = K, V = Lv>,
    Ri: PairItem<C = C, K = K, V = Rv>,
    L: PairStream<C = C, K = K, V = Lv, Item = Li>,
    R: PairStream<C = C, K = K, V = Rv, Item = Ri>,
> Stream for ProductSorted<L, R, K, Lv, Rv>
{
    type Item = C::Pair<K, (Lv, Rv)>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        macro_rules! clear_all {
            () => {{
                this.lv.clear();
                this.rv.clear();
            }};
        }
        macro_rules! stop {
            () => {{
                clear_all!();
                return Poll::Ready(None);
            }};
        }
        macro_rules! try_next {
            ($s:ident) => {
                match this.$s.as_mut().poll_next(cx) {
                    Poll::Ready(Some(kv)) => match kv.into_kv::<(Lv, Rv)>() {
                        Ok(kv) => Poll::Ready(Some(kv)),
                        Err(e) => {
                            clear_all!();
                            return Poll::Ready(Some(e));
                        }
                    },
                    Poll::Ready(None) => Poll::Ready(None),
                    Poll::Pending => Poll::Pending,
                }
            };
        }
        macro_rules! two_nexts {
            ($lk:ident, $lv:ident, $rk:ident, $rv:ident) => {{
                clear_all!();
                match $lk.cmp(&$rk) {
                    Ordering::Less => {
                        *this.last = Some(($lk, None, Some(Either::Right(Some(($rk, $rv))))));
                    }
                    Ordering::Equal => {
                        this.lv.push($lv);
                        this.rv.push($rv);
                        *this.last = Some(($lk, Some(Either::Left(0)), None));
                    }
                    Ordering::Greater => {
                        *this.last = Some(($rk, None, Some(Either::Left(Some(($lk, $lv))))));
                    }
                }
            }};
        }
        loop {
            if let Some((k, yielded, next)) = this.last {
                if let Some(y) = yielded {
                    macro_rules! next_kv {
                        ($last:ident, $lag:ident, $y:ident, $lv:ident, $rv:ident) => {{
                            let $lag = this.$lag[*$y].clone();
                            *$y += 1;
                            if *$y == this.$lag.len() {
                                yielded.take();
                            }
                            let $last = this.$last.last().expect("inconsistent state").clone();
                            ($lv, $rv)
                        }};
                    }
                    let k = k.clone();
                    let (lv, rv) = match y {
                        Either::Left(y) => next_kv!(rv, lv, y, lv, rv),
                        Either::Right(y) => next_kv!(lv, rv, y, lv, rv),
                    };
                    return Poll::Ready(Some(PairItem::from_kv(k, (lv, rv))));
                }
                if let Some(n) = next {
                    match n {
                        Either::Left(lv) => {
                            assert!(lv.is_some() || !this.lv.is_empty());
                            let Some((rk, rv)) = ready!(try_next!(r)) else {
                                stop!()
                            };
                            if rk == *k {
                                if !this.lv.is_empty() {
                                    this.rv.push(rv);
                                    *yielded = Some(Either::Left(0));
                                }
                            } else {
                                assert!(*k < rk);
                                if let Some((lk, lv)) = lv.take() {
                                    two_nexts!(lk, lv, rk, rv);
                                } else {
                                    stop!()
                                }
                            }
                        }
                        Either::Right(rv) => {
                            assert!(rv.is_some() || !this.rv.is_empty());
                            let Some((lk, lv)) = ready!(try_next!(l)) else {
                                stop!()
                            };
                            if lk == *k {
                                if !this.rv.is_empty() {
                                    this.lv.push(lv);
                                    *yielded = Some(Either::Right(0));
                                }
                            } else {
                                assert!(*k < lk);
                                if let Some((rk, rv)) = rv.take() {
                                    two_nexts!(lk, lv, rk, rv);
                                } else {
                                    stop!()
                                }
                            }
                        }
                    }
                } else if let Poll::Ready(lkv) = try_next!(l) {
                    if let Some((lk, lv)) = lkv {
                        if lk == *k {
                            this.lv.push(lv);
                            if !this.rv.is_empty() {
                                *yielded = Some(Either::Right(0));
                            }
                        } else {
                            assert!(*k < lk);
                            *next = Some(Either::Left(Some((lk, lv))));
                            if this.lv.is_empty() {
                                this.rv.clear();
                            }
                        }
                    } else {
                        *next = Some(Either::Left(None));
                        if this.lv.is_empty() {
                            stop!()
                        }
                    }
                } else if let Poll::Ready(rkv) = try_next!(r) {
                    if let Some((rk, rv)) = rkv {
                        if rk == *k {
                            this.rv.push(rv);
                            if !this.lv.is_empty() {
                                *yielded = Some(Either::Left(0));
                            }
                        } else {
                            assert!(*k < rk);
                            *next = Some(Either::Right(Some((rk, rv))));
                            if this.rv.is_empty() {
                                this.lv.clear();
                            }
                        }
                    } else {
                        *next = Some(Either::Right(None));
                        if this.rv.is_empty() {
                            stop!()
                        }
                    }
                } else {
                    return Poll::Pending;
                }
            } else if let Poll::Ready(lkv) = try_next!(l) {
                let Some((lk, lv)) = lkv else { stop!() };
                *this.last = Some((lk, None, None));
                this.lv.push(lv);
            } else if let Poll::Ready(rkv) = try_next!(r) {
                let Some((rk, rv)) = rkv else { stop!() };
                *this.last = Some((rk, None, None));
                this.rv.push(rv);
            } else {
                return Poll::Pending;
            }
        }
    }
}

pub fn product_sorted<L: PairStream, R: PairStream>(l: L, r: R) -> ProductSorted<L, R>
where
    ProductSorted<L, R>: Stream,
    (L, R): StreamPair,
{
    ProductSorted {
        l,
        r,
        last: None,
        lv: Vec::new(),
        rv: Vec::new(),
    }
}

pub trait ProductSortedExt: Sized + PairStream<K: Ord + Clone, V: Clone> {
    fn product_sorted<R: PairStream<C = Self::C, K = Self::K, V: Clone>>(
        self,
        right: R,
    ) -> ProductSorted<Self, R> {
        product_sorted(self, right)
    }
}

impl<L: PairStream<K: Ord + Clone, V: Clone>> ProductSortedExt for L {}

#[test]
fn simple() {
    use futures_util::StreamExt;
    let l = futures_util::stream::iter([(1, 2), (4, 5)]);
    let r = futures_util::stream::iter([(1, 3), (4, 6)]);
    let s = async_io::block_on(l.product_sorted(r).collect::<Vec<_>>());
    assert_eq!(s, [(1, (2, 3)), (4, (5, 6))]);
}

#[test]
fn prod() {
    use futures_util::StreamExt;
    let l = futures_util::stream::iter([(1, 2), (1, 3)]);
    let r = futures_util::stream::iter([(1, 4), (1, 5)]);
    let s = async_io::block_on(l.product_sorted(r).collect::<Vec<_>>());
    assert_eq!(s, [(1, (2, 4)), (1, (3, 4)), (1, (2, 5)), (1, (3, 5))]);
}

#[test]
fn interleave() {
    use futures_util::StreamExt;
    let l = futures_util::stream::iter([(1, "a"), (2, "c"), (5, "f"), (6, "g")]);
    let r = futures_util::stream::iter([(1, "b"), (3, "d"), (4, "e"), (6, "h")]);
    let s = async_io::block_on(l.product_sorted(r).collect::<Vec<_>>());
    assert_eq!(s, [(1, ("a", "b")), (6, ("g", "h"))]);
}

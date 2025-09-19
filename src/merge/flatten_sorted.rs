use std::{
    cmp::{Ordering, Reverse},
    collections::BinaryHeap,
    ops::{Deref, DerefMut},
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{
    Stream, StreamExt, ready,
    stream::{FusedStream, FuturesUnordered},
};
use pin_project::pin_project;

use super::pair_item::PairItem;

#[derive(Debug)]
struct Unordered<T>(T);

impl<T> PartialEq for Unordered<T> {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl<T> Eq for Unordered<T> {}

impl<T> PartialOrd for Unordered<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for Unordered<T> {
    fn cmp(&self, _: &Self) -> Ordering {
        Ordering::Equal
    }
}

struct OneThenSelf<S> {
    stream: Option<S>,
}

impl<S: Stream + Unpin> Future for OneThenSelf<S> {
    type Output = Option<(S::Item, S)>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Ready(
            match ready!(
                self.stream
                    .as_mut()
                    .expect("polled after ready")
                    .poll_next_unpin(cx)
            ) {
                Some(item) => {
                    let stream = self.stream.take().expect("not None");
                    Some((item, stream))
                }
                None => {
                    self.stream.take();
                    None
                }
            },
        )
    }
}

#[pin_project]
#[derive(Debug)]
struct FlattenOptions<S> {
    #[pin]
    stream: S,
}

impl<T, S: Stream<Item = Option<T>>> Stream for FlattenOptions<S> {
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        while let Some(item) = ready!(self.as_mut().project().stream.poll_next(cx)) {
            if let Some(item) = item {
                return Poll::Ready(Some(item));
            }
        }
        Poll::Ready(None)
    }
}

impl<T, S: FusedStream<Item = Option<T>>> FusedStream for FlattenOptions<S> {
    fn is_terminated(&self) -> bool {
        self.stream.is_terminated()
    }
}

impl<S> Deref for FlattenOptions<S> {
    type Target = S;

    fn deref(&self) -> &Self::Target {
        &self.stream
    }
}

impl<S> DerefMut for FlattenOptions<S> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.stream
    }
}

type KOf<S> = <<S as Stream>::Item as PairItem>::K;
type VOf<S> = <<S as Stream>::Item as PairItem>::V;

type HeapEntry<S, K, V> = (Reverse<K>, Unordered<(V, S)>);

#[derive(Debug)]
pub struct FlattenSorted<S, K = KOf<S>, V = VOf<S>> {
    /// these all have the same last `Option<K>` which is `<= Some(waiting.peek().unwrap().0)`
    active: FlattenOptions<FuturesUnordered<OneThenSelf<S>>>,
    waiting: BinaryHeap<HeapEntry<S, K, V>>,
    floor: Vec<(K, V, S)>,
}

impl<S, K, V> Unpin for FlattenSorted<S, K, V> {}

impl<K: Ord, V, S: Unpin + Stream<Item: PairItem<K = K, V = V>>> Stream for FlattenSorted<S, K, V> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        macro_rules! clear_all {
            () => {{
                this.active.clear();
                this.waiting.clear();
                this.floor.clear();
            }};
        }
        loop {
            if let Some((k, v, s)) = this.floor.pop() {
                this.active.push(OneThenSelf { stream: Some(s) });
                break Poll::Ready(Some(PairItem::from_kv(k, v)));
            }
            assert!(this.floor.is_empty());
            while let Some((kv, s)) = ready!(this.active.poll_next_unpin(cx)) {
                let (k, v) = match kv.into_kv::<V>() {
                    Ok(kv) => kv,
                    Err(kv) => {
                        clear_all!();
                        return Poll::Ready(Some(kv));
                    }
                };
                this.waiting.push((Reverse(k), Unordered((v, s))));
            }
            assert!(this.active.is_empty());
            while let Some((Reverse(next), _)) = this.waiting.peek() {
                if let Some((prev, _, _)) = this.floor.last()
                    && prev < next
                {
                    break;
                } else {
                    let (Reverse(k), Unordered((v, s))) = this.waiting.pop().expect("not empty");
                    this.floor.push((k, v, s));
                }
            }
            if this.waiting.is_empty() && this.floor.is_empty() {
                clear_all!();
                break Poll::Ready(None);
            }
        }
    }
}

impl<S: Unpin + Stream<Item: PairItem<K: Ord>>> FusedStream for FlattenSorted<S> {
    fn is_terminated(&self) -> bool {
        self.active.is_empty() && self.waiting.is_empty()
    }
}

pub trait FlattenSortedExt: Sized + IntoIterator<Item: Stream<Item: PairItem<K: Ord>>> {
    #[must_use]
    fn flatten_sorted(self) -> FlattenSorted<Self::Item> {
        let active = FlattenOptions {
            stream: self
                .into_iter()
                .map(|s| OneThenSelf { stream: Some(s) })
                .collect(),
        };
        FlattenSorted {
            active,
            waiting: Default::default(),
            floor: Default::default(),
        }
    }
}

impl<I: IntoIterator<Item: Stream<Item: PairItem<K: Ord>>>> FlattenSortedExt for I {}

#[test]
fn interleave() {
    let m = |k| (k, ());
    let a = futures_util::stream::iter([1, 3, 5]).map(m);
    let b = futures_util::stream::iter([2, 4, 6]).map(m);
    let c = async_io::block_on([a, b].flatten_sorted().map(|(k, ())| k).collect::<Vec<_>>());
    assert_eq!(c, [1, 2, 3, 4, 5, 6]);
}

#[test]
fn chain() {
    let m = |k| (k, ());
    let a = futures_util::stream::iter([1, 2, 3]).map(m);
    let b = futures_util::stream::iter([4, 5, 6]).map(m);
    let c = async_io::block_on([a, b].flatten_sorted().map(|(k, ())| k).collect::<Vec<_>>());
    assert_eq!(c, [1, 2, 3, 4, 5, 6]);
}

#[test]
fn duplicate() {
    let m = |k| (k, ());
    let a = futures_util::stream::iter([1, 1, 2]).map(m);
    let b = futures_util::stream::iter([1, 2, 2]).map(m);
    let c = async_io::block_on([a, b].flatten_sorted().map(|(k, ())| k).collect::<Vec<_>>());
    assert_eq!(c, [1, 1, 1, 2, 2, 2]);
}

#[test]
fn one_empty() {
    let f = |k: &i32| futures_util::future::ready(*k < 4);
    let m = |k| (k, ());
    let a = futures_util::stream::iter([1, 2, 3]).filter(f).map(m);
    let b = futures_util::stream::iter([4, 5, 6]).filter(f).map(m);
    let c = async_io::block_on([a, b].flatten_sorted().map(|(k, ())| k).collect::<Vec<_>>());
    assert_eq!(c, [1, 2, 3]);
}

#[test]
fn both_empty() {
    let m = |k: i32| (k, ());
    let a = futures_util::stream::iter([]).map(m);
    let b = futures_util::stream::iter([]).map(m);
    let c = async_io::block_on([a, b].flatten_sorted().map(|(k, ())| k).collect::<Vec<_>>());
    assert_eq!(c, []);
}

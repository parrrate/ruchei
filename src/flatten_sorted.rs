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

pub trait Sortable: Sized {
    type K: Ord;
    type V;

    fn into_kv(self) -> Result<(Self::K, Self::V), Self>;
    fn from_kv(k: Self::K, v: Self::V) -> Self;
}

impl<K: Ord, V> Sortable for (K, V) {
    type K = K;
    type V = V;

    fn into_kv(self) -> Result<(Self::K, Self::V), Self> {
        Ok(self)
    }

    fn from_kv(k: Self::K, v: Self::V) -> Self {
        (k, v)
    }
}

impl<K: Ord, V, E> Sortable for Result<(K, V), E> {
    type K = K;
    type V = V;

    fn into_kv(self) -> Result<(Self::K, Self::V), Self> {
        self.map_err(Err)
    }

    fn from_kv(k: Self::K, v: Self::V) -> Self {
        Ok((k, v))
    }
}

type KOf<S> = <<S as Stream>::Item as Sortable>::K;
type VOf<S> = <<S as Stream>::Item as Sortable>::V;

type HeapEntry<S> = (Reverse<KOf<S>>, Unordered<(VOf<S>, S)>);

pub struct FlattenSorted<S: Stream<Item: Sortable>> {
    /// these all have the same last `Option<K>` which is `<= Some(waiting.peek().unwrap().0)`
    active: FlattenOptions<FuturesUnordered<OneThenSelf<S>>>,
    waiting: BinaryHeap<HeapEntry<S>>,
    floor: Vec<(KOf<S>, VOf<S>, S)>,
}

impl<S: Stream<Item: Sortable>> Unpin for FlattenSorted<S> {}

impl<K: Ord, V, S: Unpin + Stream<Item: Sortable<K = K, V = V>>> Stream for FlattenSorted<S> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        loop {
            if let Some((k, v, s)) = this.floor.pop() {
                this.active.push(OneThenSelf { stream: Some(s) });
                break Poll::Ready(Some(Sortable::from_kv(k, v)));
            }
            assert!(this.floor.is_empty());
            while let Some((kv, s)) = ready!(this.active.poll_next_unpin(cx)) {
                let (k, v) = match kv.into_kv() {
                    Ok(kv) => kv,
                    Err(kv) => return Poll::Ready(Some(kv)),
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
                break Poll::Ready(None);
            }
        }
    }
}

impl<S: Unpin + Stream<Item: Sortable>> FusedStream for FlattenSorted<S> {
    fn is_terminated(&self) -> bool {
        self.active.is_empty() && self.waiting.is_empty()
    }
}

pub trait FlattenSortedExt: Sized + IntoIterator<Item: Stream<Item: Sortable>> {
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

impl<I: IntoIterator<Item: Stream<Item: Sortable>>> FlattenSortedExt for I {}

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

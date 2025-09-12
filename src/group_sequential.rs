use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{Stream, ready, stream::FusedStream};
use pin_project::pin_project;

#[pin_project]
pub struct Grouped<S, K, T> {
    #[pin]
    stream: S,
    current: Option<(K, T)>,
}

pub trait GroupItem: Sized {
    type K: PartialEq;
    type V;
    type Grouped<T: Extend<Self::V>>;

    fn poll_next_group<S: FusedStream<Item = Self>, T: Default + Extend<Self::V>>(
        stream: Pin<&mut S>,
        current: &mut Option<(Self::K, T)>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Self::Grouped<T>>>;
}

impl<K: PartialEq, V> GroupItem for (K, V) {
    type K = K;
    type V = V;
    type Grouped<T: Extend<Self::V>> = (K, T);

    fn poll_next_group<S: FusedStream<Item = Self>, T: Default + Extend<Self::V>>(
        mut stream: Pin<&mut S>,
        current: &mut Option<(Self::K, T)>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Self::Grouped<T>>> {
        let mut next = || {
            if stream.is_terminated() {
                Poll::Ready(None)
            } else {
                stream.as_mut().poll_next(cx)
            }
        };
        let (k, t) = match current.as_mut() {
            Some(current) => current,
            None => {
                let Some((k, v)) = ready!(next()) else {
                    return Poll::Ready(None);
                };
                let mut t = T::default();
                t.extend(std::iter::once(v));
                current.insert((k, t))
            }
        };
        while let Some((mut k_other, v)) = ready!(next()) {
            if k_other != *k {
                std::mem::swap(&mut k_other, k);
                let t_other = std::mem::take(t);
                t.extend(std::iter::once(v));
                return Poll::Ready(Some((k_other, t_other)));
            }
            t.extend(std::iter::once(v));
        }
        let current = current.take().expect("has just dealt with Some");
        Poll::Ready(Some(current))
    }
}

impl<K: PartialEq, V, E> GroupItem for Result<(K, V), E> {
    type K = K;
    type V = V;
    type Grouped<T: Extend<Self::V>> = Result<(K, T), E>;

    fn poll_next_group<S: FusedStream<Item = Self>, T: Default + Extend<Self::V>>(
        mut stream: Pin<&mut S>,
        current: &mut Option<(Self::K, T)>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Self::Grouped<T>>> {
        let mut next = || {
            if stream.is_terminated() {
                Poll::Ready(None)
            } else {
                stream.as_mut().poll_next(cx)
            }
        };
        let (k, t) = match current.as_mut() {
            Some(current) => current,
            None => {
                let Some((k, v)) = ready!(next()?) else {
                    return Poll::Ready(None);
                };
                let mut t = T::default();
                t.extend(std::iter::once(v));
                current.insert((k, t))
            }
        };
        while let Some((mut k_other, v)) = ready!(next()?) {
            if k_other != *k {
                std::mem::swap(&mut k_other, k);
                let t_other = std::mem::take(t);
                t.extend(std::iter::once(v));
                return Poll::Ready(Some(Ok((k_other, t_other))));
            }
            t.extend(std::iter::once(v));
        }
        let current = current.take().expect("has just dealt with Some");
        Poll::Ready(Some(Ok(current)))
    }
}

impl<S: FusedStream<Item = I>, I: GroupItem, T: Default + Extend<I::V>> Stream
    for Grouped<S, I::K, T>
{
    type Item = I::Grouped<T>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        I::poll_next_group(this.stream, this.current, cx)
    }
}

impl<S: FusedStream<Item = I>, I: GroupItem, T: Default + Extend<I::V>> FusedStream
    for Grouped<S, I::K, T>
{
    fn is_terminated(&self) -> bool {
        self.current.is_none() && self.stream.is_terminated()
    }
}

pub trait GroupSequential: Sized + FusedStream<Item: GroupItem<K = Self::K, V = Self::V>> {
    type K: PartialEq;
    type V;
    fn group_sequential<T: Default + Extend<Self::V>>(self) -> Grouped<Self, Self::K, T> {
        Grouped {
            stream: self,
            current: None,
        }
    }
}

impl<S: FusedStream<Item = I>, I: GroupItem> GroupSequential for S {
    type K = I::K;
    type V = I::V;
}

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{Stream, ready};
use pin_project::pin_project;

#[pin_project]
#[derive(Debug)]
pub struct Compress<S: Stream, C> {
    #[pin]
    stream: S,
    #[pin]
    credits: Option<C>,
    item: Option<S::Item>,
}

#[derive(Debug)]
pub struct Credit;

#[derive(Debug)]
pub struct Credited<C>(pub C);

mod private {
    use super::{Credit, Credited, Pin};

    pub trait FromPin<C> {
        type Item<T>;

        fn from_pin(pinned: Pin<&mut Option<C>>) -> Self;

        fn item<T>(self, item: T) -> Self::Item<T>;
    }

    impl<C> FromPin<C> for Credit {
        type Item<T> = T;

        fn from_pin(pinned: Pin<&mut Option<C>>) -> Self {
            let _ = pinned;
            Self
        }

        fn item<T>(self, item: T) -> Self::Item<T> {
            item
        }
    }

    impl<C: Unpin> FromPin<C> for Credited<C> {
        type Item<T> = (T, C);

        fn from_pin(mut pinned: Pin<&mut Option<C>>) -> Self {
            Self(pinned.take().unwrap())
        }

        fn item<T>(self, item: T) -> Self::Item<T> {
            (item, self.0)
        }
    }
}

use private::FromPin;

impl<T: IntoIterator + Extend<T::Item>, U: FromPin<C>, S: Stream<Item = T>, C: Stream<Item = U>>
    Stream for Compress<S, C>
{
    type Item = U::Item<T>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        match this.credits.as_mut().as_pin_mut() {
            Some(credits) => loop {
                match this.item.take() {
                    Some(mut item) => match credits.poll_next(cx) {
                        Poll::Ready(credit) => {
                            break Poll::Ready(credit.map(|credit| credit.item(item)));
                        }
                        Poll::Pending => {
                            break loop {
                                match this.stream.as_mut().poll_next(cx) {
                                    Poll::Ready(next) => match next {
                                        Some(next) => item.extend(next),
                                        None => {
                                            break Poll::Ready(Some(
                                                U::from_pin(this.credits).item(item),
                                            ));
                                        }
                                    },
                                    Poll::Pending => {
                                        *this.item = Some(item);
                                        break Poll::Pending;
                                    }
                                }
                            };
                        }
                    },
                    None => match ready!(this.stream.as_mut().poll_next(cx)) {
                        Some(item) => *this.item = Some(item),
                        None => break Poll::Ready(None),
                    },
                }
            },
            None => Poll::Ready(None),
        }
    }
}

pub trait SelfExtend: IntoIterator + Extend<Self::Item> {}

impl<T: IntoIterator + Extend<Self::Item>> SelfExtend for T {}

pub trait CompressExt: Sized + Stream<Item: SelfExtend> {
    #[must_use]
    fn compress<C: Stream<Item: FromPin<C>>>(self, credits: C) -> Compress<Self, C> {
        Compress {
            stream: self,
            credits: Some(credits),
            item: None,
        }
    }
}

impl<S: Stream<Item: SelfExtend>> CompressExt for S {}

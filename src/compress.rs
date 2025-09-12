use std::{
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{ready, Stream};
use pin_project::pin_project;

#[pin_project]
pub struct Compress<S: Stream, C> {
    #[pin]
    stream: S,
    #[pin]
    credits: Option<C>,
    item: Option<S::Item>,
}

pub struct Credit;

pub struct Credited<C>(pub C);

mod private {
    use super::{Credit, Credited, Pin};

    pub trait FromPin<C> {
        fn from_pin(pinned: Pin<&mut Option<C>>) -> Self;
    }

    impl<C> FromPin<C> for Credit {
        fn from_pin(pinned: Pin<&mut Option<C>>) -> Self {
            let _ = pinned;
            Self
        }
    }

    impl<C: Unpin> FromPin<C> for Credited<C> {
        fn from_pin(mut pinned: Pin<&mut Option<C>>) -> Self {
            Self(pinned.take().unwrap())
        }
    }
}

use private::FromPin;

impl<
        T: IntoIterator + Extend<T::Item>,
        U: FromPin<C>,
        S: Stream<Item = T>,
        C: Stream<Item = U>,
    > Stream for Compress<S, C>
{
    type Item = (T, U);

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        match this.credits.as_mut().as_pin_mut() {
            Some(credits) => loop {
                match this.item.take() {
                    Some(mut item) => match credits.poll_next(cx) {
                        Poll::Ready(credit) => {
                            break Poll::Ready(credit.map(|credit| (item, credit)))
                        }
                        Poll::Pending => {
                            break loop {
                                match this.stream.as_mut().poll_next(cx) {
                                    Poll::Ready(next) => match next {
                                        Some(next) => item.extend(next),
                                        None => {
                                            break Poll::Ready(Some((
                                                item,
                                                U::from_pin(this.credits),
                                            )))
                                        }
                                    },
                                    Poll::Pending => {
                                        *this.item = Some(item);
                                        break Poll::Pending;
                                    }
                                }
                            }
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

pub trait CompressExt: Stream + Sized {
    fn compress<C>(self, credits: C) -> Compress<Self, C>;
}

impl<S: Stream> CompressExt for S {
    fn compress<C>(self, credits: C) -> Compress<Self, C> {
        Compress {
            stream: self,
            credits: Some(credits),
            item: None,
        }
    }
}

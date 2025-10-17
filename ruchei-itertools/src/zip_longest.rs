use core::{
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{Stream, StreamExt, future::Either, ready, stream::Fuse};
use pin_project::pin_project;

use crate::EitherOrBoth;

#[pin_project]
pub struct ZipLongest<L, R, Lt = <L as Stream>::Item, Rt = <R as Stream>::Item> {
    #[pin]
    l: Fuse<L>,
    #[pin]
    r: Fuse<R>,
    state: Option<Either<Lt, Rt>>,
}

pub fn zip_longest<L: Stream, R: Stream>(l: L, r: R) -> crate::ZipLongest<L, R> {
    ZipLongest {
        l: l.fuse(),
        r: r.fuse(),
        state: None,
    }
}

impl<L: Stream<Item = Lt>, R: Stream<Item = Rt>, Lt, Rt> Stream for ZipLongest<L, R, Lt, Rt> {
    type Item = EitherOrBoth<Lt, Rt>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        Poll::Ready(loop {
            if let Some(state) = this.state.as_ref() {
                match state {
                    Either::Left(_) => {
                        let r = ready!(this.r.as_mut().poll_next(cx));
                        let l = match this.state.take().unwrap() {
                            Either::Left(l) => l,
                            Either::Right(_) => unreachable!(),
                        };
                        break Some(if let Some(r) = r {
                            EitherOrBoth::Both(l, r)
                        } else {
                            EitherOrBoth::Left(l)
                        });
                    }
                    Either::Right(_) => {
                        let l = ready!(this.l.as_mut().poll_next(cx));
                        let r = match this.state.take().unwrap() {
                            Either::Left(_) => unreachable!(),
                            Either::Right(r) => r,
                        };
                        break Some(if let Some(l) = l {
                            EitherOrBoth::Both(l, r)
                        } else {
                            EitherOrBoth::Right(r)
                        });
                    }
                }
            } else if let Poll::Ready(Some(l)) = this.l.as_mut().poll_next(cx) {
                *this.state = Some(Either::Left(l));
            } else if let Poll::Ready(Some(r)) = this.r.as_mut().poll_next(cx) {
                *this.state = Some(Either::Right(r));
            } else if this.l.is_done() && this.r.is_done() {
                break None;
            } else {
                return Poll::Pending;
            }
        })
    }
}

#[cfg(test)]
mod test {
    use crate::{AsyncItertools, EitherOrBoth};

    #[test]
    fn left_longer() {
        let l = futures_util::stream::iter([1, 2, 3]);
        let r = futures_util::stream::iter([1, 2]);
        assert!(futures_executor::block_on_stream(l.zip_longest(r)).eq([
            EitherOrBoth::Both(1, 1),
            EitherOrBoth::Both(2, 2),
            EitherOrBoth::Left(3),
        ]));
    }

    #[test]
    fn equal_length() {
        let l = futures_util::stream::iter([1, 2, 3]);
        let r = futures_util::stream::iter([1, 2, 3]);
        assert!(futures_executor::block_on_stream(l.zip_longest(r)).eq([
            EitherOrBoth::Both(1, 1),
            EitherOrBoth::Both(2, 2),
            EitherOrBoth::Both(3, 3),
        ]));
    }

    #[test]
    fn right_longer() {
        let l = futures_util::stream::iter([1, 2]);
        let r = futures_util::stream::iter([1, 2, 3]);
        assert!(futures_executor::block_on_stream(l.zip_longest(r)).eq([
            EitherOrBoth::Both(1, 1),
            EitherOrBoth::Both(2, 2),
            EitherOrBoth::Right(3),
        ]));
    }
}

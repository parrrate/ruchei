use core::{
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{Stream, TryStream, ready};
use option_entry::{Entry, OptionEntry};
use pin_project::pin_project;

use crate::check::assert_stream;

#[pin_project]
pub struct TryZipLazy<L, R, Lt = <L as TryStream>::Ok> {
    #[pin]
    l: L,
    #[pin]
    r: R,
    left_item: Option<Lt>,
}

pub fn try_zip_lazy<L: TryStream, R: TryStream<Error = L::Error>>(
    l: L,
    r: R,
) -> crate::TryZipLazy<L, R> {
    assert_stream(TryZipLazy {
        l,
        r,
        left_item: None,
    })
}

impl<L: TryStream<Ok = Lt, Error = E>, R: TryStream<Ok = Rt, Error = E>, Lt, Rt, E> Stream
    for TryZipLazy<L, R, Lt>
{
    type Item = Result<(Lt, Rt), E>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        let l = match this.left_item.entry() {
            Entry::Vacant(e) => {
                let Some(l) = ready!(this.l.try_poll_next(cx)?) else {
                    return Poll::Ready(None);
                };
                e.insert_entry(l)
            }
            Entry::Occupied(e) => e,
        };
        let r = ready!(this.r.try_poll_next(cx)?);
        let l = l.remove();
        let Some(r) = r else {
            return Poll::Ready(None);
        };
        Poll::Ready(Some(Ok((l, r))))
    }
}

#[cfg(test)]
mod test {
    use core::convert::Infallible;

    use crate::AsyncItertools;

    #[test]
    fn left_longer() {
        let l = futures_util::stream::iter([1, 2, 3].map(Ok::<_, Infallible>));
        let r = futures_util::stream::iter([1, 2].map(Ok::<_, Infallible>));
        assert!(
            futures_executor::block_on_stream(l.try_zip_lazy(r))
                .eq([(1, 1), (2, 2)].map(Ok::<_, Infallible>)),
        );
    }

    #[test]
    fn equal_length() {
        let l = futures_util::stream::iter([1, 2, 3].map(Ok::<_, Infallible>));
        let r = futures_util::stream::iter([1, 2, 3].map(Ok::<_, Infallible>));
        assert!(
            futures_executor::block_on_stream(l.try_zip_lazy(r))
                .eq([(1, 1), (2, 2), (3, 3)].map(Ok::<_, Infallible>)),
        );
    }

    #[test]
    fn right_longer() {
        let l = futures_util::stream::iter([1, 2].map(Ok::<_, Infallible>));
        let r = futures_util::stream::iter([1, 2, 3].map(Ok::<_, Infallible>));
        assert!(
            futures_executor::block_on_stream(l.try_zip_lazy(r))
                .eq([(1, 1), (2, 2)].map(Ok::<_, Infallible>)),
        );
    }
}

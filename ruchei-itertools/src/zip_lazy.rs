use core::{
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{Stream, ready};
use option_entry::{Entry, OptionEntry};
use pin_project::pin_project;

use crate::check::assert_stream;

#[pin_project]
pub struct ZipLazy<L, R, Lt = <L as Stream>::Item> {
    #[pin]
    l: L,
    #[pin]
    r: R,
    left_item: Option<Lt>,
}

pub fn zip_lazy<L: Stream, R: Stream>(l: L, r: R) -> crate::ZipLazy<L, R> {
    assert_stream(ZipLazy {
        l,
        r,
        left_item: None,
    })
}

impl<L: Stream<Item = Lt>, R: Stream<Item = Rt>, Lt, Rt> Stream for ZipLazy<L, R, Lt> {
    type Item = (Lt, Rt);

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        let l = match this.left_item.entry() {
            Entry::Vacant(e) => {
                let Some(l) = ready!(this.l.poll_next(cx)) else {
                    return Poll::Ready(None);
                };
                e.insert_entry(l)
            }
            Entry::Occupied(e) => e,
        };
        let r = ready!(this.r.poll_next(cx));
        let l = l.remove();
        let Some(r) = r else {
            return Poll::Ready(None);
        };
        Poll::Ready(Some((l, r)))
    }
}

#[cfg(test)]
mod test {
    use crate::AsyncItertools;

    #[test]
    fn left_longer() {
        let l = futures_util::stream::iter([1, 2, 3]);
        let r = futures_util::stream::iter([1, 2]);
        assert!(futures_executor::block_on_stream(l.zip_lazy(r)).eq([(1, 1), (2, 2)]));
    }

    #[test]
    fn equal_length() {
        let l = futures_util::stream::iter([1, 2, 3]);
        let r = futures_util::stream::iter([1, 2, 3]);
        assert!(futures_executor::block_on_stream(l.zip_lazy(r)).eq([(1, 1), (2, 2), (3, 3)]));
    }

    #[test]
    fn right_longer() {
        let l = futures_util::stream::iter([1, 2]);
        let r = futures_util::stream::iter([1, 2, 3]);
        assert!(futures_executor::block_on_stream(l.zip_lazy(r)).eq([(1, 1), (2, 2)]));
    }
}

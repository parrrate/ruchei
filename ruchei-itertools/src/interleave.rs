//! <https://docs.rs/itertools/0.14.0/src/itertools/adaptors/mod.rs.html#28-105>

use core::{
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{
    Stream, StreamExt, ready,
    stream::{Fuse, FusedStream},
};
use pin_project::pin_project;

use crate::check::assert_stream;

#[pin_project]
pub struct Interleave<I, J> {
    #[pin]
    i: Fuse<I>,
    #[pin]
    j: Fuse<J>,
    next_coming_from_j: bool,
}

impl<I: Stream, J: Stream<Item = I::Item>> Stream for Interleave<I, J> {
    type Item = I::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        *this.next_coming_from_j = !*this.next_coming_from_j;
        if *this.next_coming_from_j {
            match ready!(this.i.poll_next(cx)) {
                None => this.j.poll_next(cx),
                r => Poll::Ready(r),
            }
        } else {
            match ready!(this.j.poll_next(cx)) {
                None => this.i.poll_next(cx),
                r => Poll::Ready(r),
            }
        }
    }
}

pub fn interleave<I: Stream, J: Stream<Item = I::Item>>(i: I, j: J) -> crate::Interleave<I, J> {
    assert_stream(Interleave {
        i: i.fuse(),
        j: j.fuse(),
        next_coming_from_j: false,
    })
}

impl<I, J> FusedStream for Interleave<I, J>
where
    Self: Stream,
{
    fn is_terminated(&self) -> bool {
        self.i.is_done() && self.j.is_done()
    }
}

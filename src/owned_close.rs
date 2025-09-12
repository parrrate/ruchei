use std::{
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{Future, Sink};
use pin_project::pin_project;

#[derive(Debug)]
#[pin_project]
#[must_use]
pub(crate) struct OwnedClose<S, Out> {
    #[pin]
    sink: S,
    _out: PhantomData<Out>,
}

impl<Out, E, S: Sink<Out, Error = E>> Future for OwnedClose<S, Out> {
    type Output = Result<(), E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.project().sink.poll_close(cx)
    }
}

impl<S, Out> From<S> for OwnedClose<S, Out> {
    fn from(sink: S) -> Self {
        Self {
            sink,
            _out: PhantomData,
        }
    }
}

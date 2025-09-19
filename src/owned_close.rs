use std::{
    fmt::Debug,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{Future, Sink};
use pin_project::pin_project;

#[pin_project]
#[must_use]
pub(crate) struct OwnedClose<S, Out> {
    #[pin]
    sink: S,
    _out: PhantomData<Out>,
}

impl<S: Debug, Out> Debug for OwnedClose<S, Out> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnedClose")
            .field("sink", &self.sink)
            .field("_out", &self._out)
            .finish()
    }
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

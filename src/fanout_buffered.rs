use std::{
    convert::Infallible,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use futures_util::{
    future::FusedFuture,
    lock::{Mutex, OwnedMutexGuard, OwnedMutexLockFuture},
    ready,
    stream::{Fuse, FuturesUnordered, SelectAll},
    Future, Sink, Stream, StreamExt,
};
use pin_project::pin_project;

use crate::{callback::Callback, owned_close::OwnedClose};

#[derive(Clone)]
struct Done(Arc<OwnedMutexGuard<()>>);

struct Node<Out>(Out, Done, List<Out>);

struct List<Out>(Arc<Mutex<Option<Node<Out>>>>);

impl<Out> Drop for List<Out> {
    fn drop(&mut self) {
        loop {
            let node = {
                let Some(mut guard) = self.0.try_lock() else {
                    break;
                };
                let Some(node) = guard.take() else {
                    break;
                };
                node
            };
            *self = node.2;
        }
    }
}

#[derive(Default)]
enum State<Out> {
    #[default]
    Flushed,
    Readying(Out, Done),
    Started(Done),
}

impl<Out> State<Out> {
    fn take(&mut self) -> Self {
        std::mem::take(self)
    }
}

#[pin_project]
struct Unicast<S, Out, F> {
    #[pin]
    stream: S,
    #[pin]
    list: OwnedMutexLockFuture<Option<Node<Out>>>,
    state: State<Out>,
    callback: F,
}

impl<In, Out: Clone, E, S: Stream<Item = Result<In, E>> + Sink<Out, Error = E>, F: Callback<E>>
    Unicast<S, Out, F>
{
    fn poll_list(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut this = self.project();
        if !this.list.is_terminated() {
            match this.list.as_mut().poll(cx) {
                Poll::Ready(guard) => match guard.as_ref() {
                    Some(Node(out, done, list)) => {
                        *this.state = State::Readying(out.clone(), done.clone());
                        *this.list = list.0.clone().lock_owned();
                        Poll::Ready(())
                    }
                    None => Poll::Pending,
                },
                Poll::Pending => Poll::Pending,
            }
        } else {
            Poll::Pending
        }
    }

    fn poll_send(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        out: Out,
        done: Done,
    ) -> Poll<Result<(), E>> {
        let mut this = self.project();
        match this.stream.as_mut().poll_ready(cx)? {
            Poll::Ready(()) => {
                this.stream.start_send(out)?;
                *this.state = State::Started(done);
                Poll::Ready(Ok(()))
            }
            Poll::Pending => {
                *this.state = State::Readying(out, done);
                Poll::Pending
            }
        }
    }

    fn state(self: Pin<&mut Self>) -> &mut State<Out> {
        self.project().state
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>, done: Done) -> Poll<Result<(), E>> {
        let this = self.project();
        match this.stream.poll_flush(cx)? {
            Poll::Ready(()) => Poll::Ready(Ok(())),
            Poll::Pending => {
                *this.state = State::Started(done);
                Poll::Pending
            }
        }
    }

    fn pre_poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), E>> {
        loop {
            match self.as_mut().state().take() {
                State::Flushed => ready!(self.as_mut().poll_list(cx)),
                State::Readying(out, done) => ready!(self.as_mut().poll_send(cx, out, done))?,
                State::Started(done) => ready!(self.as_mut().poll_flush(cx, done))?,
            }
        }
    }

    fn poll_inner(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<In, E>>> {
        let _ = self.as_mut().pre_poll(cx)?;
        self.project().stream.poll_next(cx)
    }
}

impl<In, Out: Clone, E, S: Stream<Item = Result<In, E>> + Sink<Out, Error = E>, F: Callback<E>>
    Stream for Unicast<S, Out, F>
{
    type Item = In;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.as_mut().poll_inner(cx) {
            Poll::Ready(Some(Ok(out))) => Poll::Ready(Some(out)),
            Poll::Ready(Some(Err(e))) => {
                self.callback.on_close(Some(e));
                Poll::Ready(None)
            }
            Poll::Ready(None) => {
                self.callback.on_close(None);
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

#[pin_project]
pub struct Multicast<S, Out, F, R> {
    #[pin]
    streams: Fuse<R>,
    #[pin]
    select: SelectAll<Unicast<S, Out, F>>,
    #[pin]
    closing: FuturesUnordered<OwnedClose<S, Out>>,
    #[pin]
    done: OwnedMutexLockFuture<()>,
    list_guard: OwnedMutexGuard<Option<Node<Out>>>,
    list_mutex: Arc<Mutex<Option<Node<Out>>>>,
    callback: F,
}

impl<
        In,
        Out: Clone,
        E,
        S: Unpin + Stream<Item = Result<In, E>> + Sink<Out, Error = E>,
        F: Callback<E>,
        R: Stream<Item = S>,
    > Multicast<S, Out, F, R>
{
    fn poll_next_infallible(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<In>> {
        let mut this = self.project();
        while let Poll::Ready(Some(stream)) = this.streams.as_mut().poll_next(cx) {
            this.select.push(Unicast {
                stream,
                list: this.list_mutex.clone().lock_owned(),
                state: Default::default(),
                callback: this.callback.clone(),
            });
        }
        match this.select.poll_next(cx) {
            Poll::Ready(None) if !this.streams.is_done() => Poll::Pending,
            poll => poll,
        }
    }
}

impl<
        In,
        Out: Clone,
        E,
        S: Unpin + Stream<Item = Result<In, E>> + Sink<Out, Error = E>,
        F: Callback<E>,
        R: Stream<Item = S>,
    > Stream for Multicast<S, Out, F, R>
{
    type Item = Result<In, Infallible>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.poll_next_infallible(cx).map(|o| o.map(Ok))
    }
}

impl<
        In,
        Out: Clone,
        E,
        S: Unpin + Stream<Item = Result<In, E>> + Sink<Out, Error = E>,
        F: Callback<E>,
        R: Stream<Item = S>,
    > Sink<Out> for Multicast<S, Out, F, R>
{
    type Error = Infallible;

    fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, item: Out) -> Result<(), Self::Error> {
        let mut this = self.project();
        let list_mutex = Arc::new(Mutex::new(None));
        let list = List(list_mutex.clone());
        let done_mutex = Arc::new(Mutex::new(()));
        let list_guard = list_mutex.clone().try_lock_owned().unwrap();
        **this.list_guard = Some(Node(
            item,
            Done(Arc::new(done_mutex.clone().try_lock_owned().unwrap())),
            list,
        ));
        *this.list_guard = list_guard;
        *this.done = done_mutex.lock_owned();
        *this.list_mutex = list_mutex;
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let future = self.project().done;
        if future.is_terminated() || future.poll(cx).is_ready() {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let mut this = self.project();
        if !this.select.is_empty() {
            for unicast in std::mem::take(this.select.get_mut()) {
                this.closing.push(unicast.stream.into())
            }
        }
        loop {
            match this.closing.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(()))) => this.callback.on_close(None),
                Poll::Ready(Some(Err(e))) => this.callback.on_close(Some(e)),
                Poll::Ready(None) => break Poll::Ready(Ok(())),
                Poll::Pending => break Poll::Pending,
            }
        }
    }
}

impl<
        In,
        Out: Clone,
        E,
        S: Unpin + Stream<Item = Result<In, E>> + Sink<Out, Error = E>,
        F: Callback<E>,
        R: Stream<Item = S>,
    > Multicast<S, Out, F, R>
{
    pub fn new(streams: R, callback: F) -> Self {
        let list_mutex = Arc::new(Mutex::new(None));
        let list_guard = list_mutex.clone().try_lock_owned().unwrap();
        Self {
            streams: streams.fuse(),
            select: Default::default(),
            closing: Default::default(),
            done: Arc::new(Mutex::new(())).lock_owned(),
            list_guard,
            list_mutex,
            callback,
        }
    }
}

pub trait MulticastBuffered<Out>: Sized {
    type S;

    type E;

    fn multicast_buffered<F: Callback<Self::E>>(
        self,
        callback: F,
    ) -> Multicast<Self::S, Out, F, Self>;
}

impl<
        In,
        Out: Clone,
        E,
        S: Unpin + Stream<Item = Result<In, E>> + Sink<Out, Error = E>,
        R: Stream<Item = S>,
    > MulticastBuffered<Out> for R
{
    type S = S;

    type E = E;

    fn multicast_buffered<F: Callback<Self::E>>(
        self,
        callback: F,
    ) -> Multicast<Self::S, Out, F, Self> {
        Multicast::new(self, callback)
    }
}

use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Wake},
};

use futures_util::{Sink, Stream, TryStream, ready, task::AtomicWaker};
use pin_project::pin_project;

pub trait ReplyBufferFilter<Item> {
    type Reply;
    fn reply_and_item(&mut self, item: Item) -> (Option<Self::Reply>, Option<Item>);
}

impl<Item, Reply, F: ?Sized + FnMut(Item) -> (Option<Reply>, Option<Item>)> ReplyBufferFilter<Item>
    for F
{
    type Reply = Reply;

    fn reply_and_item(&mut self, item: Item) -> (Option<Self::Reply>, Option<Item>) {
        self(item)
    }
}

#[derive(Default)]
struct Wakers {
    next: AtomicWaker,
    ready: AtomicWaker,
    flush: AtomicWaker,
}

impl Wake for Wakers {
    fn wake(self: Arc<Self>) {
        self.next.wake();
        self.ready.wake();
        self.flush.wake();
    }
}

#[pin_project]
pub struct ReplyBuffer<S, T, F> {
    #[pin]
    stream: S,
    buffer: Option<T>,
    needs_flush: bool,
    wakers: Arc<Wakers>,
    filter: F,
}

impl<S: Sink<T>, T, F> ReplyBuffer<S, T, F> {
    fn flush_buffer(mut self: Pin<&mut Self>, flush: bool) -> Poll<Result<(), S::Error>> {
        let mut this = self.as_mut().project();
        let waker = this.wakers.clone().into();
        let mut cx = Context::from_waker(&waker);
        if this.buffer.is_some() {
            ready!(this.stream.as_mut().poll_ready(&mut cx))?;
        }
        if let Some(item) = this.buffer.take() {
            this.stream.start_send(item)?;
            this.wakers.wake_by_ref();
        }
        if flush && *this.needs_flush {
            ready!(self.poll_flush_raw(&mut cx))?;
        }
        Poll::Ready(Ok(()))
    }

    fn poll_flush_raw(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), S::Error>> {
        let this = self.project();
        ready!(this.stream.poll_flush(cx))?;
        *this.needs_flush = false;
        this.wakers.wake_by_ref();
        Poll::Ready(Ok(()))
    }

    fn set_buffer(self: Pin<&mut Self>, reply: T) {
        assert!(self.buffer.is_none());
        let this = self.project();
        *this.buffer = Some(reply);
        *this.needs_flush = true;
    }

    fn new(stream: S, buffer: Option<T>, filter: F) -> Self {
        let needs_flush = buffer.is_some();
        Self {
            stream,
            buffer,
            needs_flush,
            wakers: Default::default(),
            filter,
        }
    }
}

pub trait IntoReplyBuffer<T>: Sized + Sink<T> {
    fn into_reply_buffer<F: ReplyBufferFilter<U>, U>(
        self,
        buffer: Option<T>,
        filter: F,
    ) -> ReplyBuffer<Self, T, F> {
        ReplyBuffer::new(self, buffer, filter)
    }
}

impl<S: Sink<T>, T> IntoReplyBuffer<T> for S {}

impl<
    S: TryStream<Ok = U, Error = E> + Sink<T, Error = E>,
    T,
    U,
    E,
    F: ReplyBufferFilter<U, Reply = T>,
> Stream for ReplyBuffer<S, T, F>
{
    type Item = Result<U, E>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.wakers.next.register(cx.waker());
        loop {
            ready!(self.as_mut().flush_buffer(true))?;
            let Some(item) = ready!(self.as_mut().project().stream.try_poll_next(cx)?) else {
                break Poll::Ready(None);
            };
            let (reply, item) = self.as_mut().project().filter.reply_and_item(item);
            if let Some(reply) = reply {
                self.as_mut().set_buffer(reply);
            }
            if let Some(item) = item {
                break Poll::Ready(Some(Ok(item)));
            }
        }
    }
}

impl<S: Sink<T, Error = E>, T, E, F> Sink<T> for ReplyBuffer<S, T, F> {
    type Error = E;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.wakers.ready.register(cx.waker());
        ready!(self.as_mut().flush_buffer(false))?;
        self.project().stream.poll_ready(cx)
    }

    fn start_send(self: Pin<&mut Self>, item: T) -> Result<(), Self::Error> {
        assert!(self.buffer.is_none());
        self.project().stream.start_send(item)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.wakers.flush.register(cx.waker());
        ready!(self.as_mut().flush_buffer(false))?;
        self.poll_flush_raw(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().stream.poll_close(cx)
    }
}

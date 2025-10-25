use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Wake},
};

use futures_util::{Sink, Stream, TryStream, ready, task::AtomicWaker};
use option_entry::{Entry, OptionEntry};
use pin_project::pin_project;

pub type Filtered<F, Item> = Option<(
    Option<<F as ReplyBufferFilter<Item>>::Reply>,
    Option<<F as ReplyBufferFilter<Item>>::Filtered>,
)>;

pub trait ReplyBufferFilter<Item> {
    type Filtered;
    type Reply;
    fn reply_and_item(&mut self, item: Item) -> Filtered<Self, Item>;
}

impl<Item, Reply, Filtered, F: ?Sized + FnMut(Item) -> Option<(Option<Reply>, Option<Filtered>)>>
    ReplyBufferFilter<Item> for F
{
    type Filtered = Filtered;
    type Reply = Reply;

    fn reply_and_item(
        &mut self,
        item: Item,
    ) -> Option<(Option<Self::Reply>, Option<Self::Filtered>)> {
        self(item)
    }
}

#[derive(Debug, Default)]
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
#[derive(Debug)]
pub struct WithReply<S, T, F> {
    #[pin]
    stream: S,
    reply: Option<T>,
    start: Option<T>,
    needs_flush: bool,
    wakers: Arc<Wakers>,
    filter: F,
}

impl<S: Sink<T>, T, F> WithReply<S, T, F> {
    fn poll_reply(self: Pin<&mut Self>) -> Poll<Result<(), S::Error>> {
        let mut this = self.project();
        let waker = this.wakers.clone().into();
        let mut cx = Context::from_waker(&waker);
        if let Entry::Occupied(entry) = this.reply.entry() {
            ready!(this.stream.as_mut().poll_ready(&mut cx))?;
            this.stream.start_send(entry.remove())?;
            this.wakers.wake_by_ref();
        }
        Poll::Ready(Ok(()))
    }

    fn poll_start(self: Pin<&mut Self>) -> Poll<Result<(), S::Error>> {
        let mut this = self.project();
        let waker = this.wakers.clone().into();
        let mut cx = Context::from_waker(&waker);
        if let Entry::Occupied(entry) = this.start.entry() {
            ready!(this.stream.as_mut().poll_ready(&mut cx))?;
            this.stream.start_send(entry.remove())?;
            this.wakers.wake_by_ref();
        }
        Poll::Ready(Ok(()))
    }

    fn poll(mut self: Pin<&mut Self>) -> Poll<Result<(), S::Error>> {
        ready!(self.as_mut().poll_reply())?;
        self.poll_start()
    }

    fn poll_flush_if_needed(mut self: Pin<&mut Self>) -> Result<(), S::Error> {
        let this = self.as_mut().project();
        let waker = this.wakers.clone().into();
        let mut cx = Context::from_waker(&waker);
        if *this.needs_flush {
            let _ = self.poll_flush_raw(&mut cx)?;
        }
        Ok(())
    }

    fn poll_flush_raw(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), S::Error>> {
        let this = self.project();
        ready!(this.stream.poll_flush(cx))?;
        if *this.needs_flush {
            *this.needs_flush = false;
            this.wakers.wake_by_ref();
        }
        Poll::Ready(Ok(()))
    }

    fn set_reply(self: Pin<&mut Self>, reply: T) {
        assert!(self.reply.is_none());
        let this = self.project();
        *this.reply = Some(reply);
        *this.needs_flush = true;
    }

    #[must_use]
    fn new(stream: S, buffer: Option<T>, filter: F) -> Self {
        let needs_flush = buffer.is_some();
        Self {
            stream,
            reply: buffer,
            start: None,
            needs_flush,
            wakers: Default::default(),
            filter,
        }
    }
}

pub trait WithReplyExt<T>: Sized + Sink<T> + TryStream {
    #[must_use]
    fn with_reply<F: ReplyBufferFilter<Self::Ok, Reply = T>>(
        self,
        buffer: Option<T>,
        filter: F,
    ) -> WithReply<Self, T, F> {
        WithReply::new(self, buffer, filter)
    }
}

impl<S: TryStream<Ok = U, Error = E> + Sink<T, Error = E>, T, U, E> WithReplyExt<T> for S {}

impl<
    S: TryStream<Ok = U, Error = E> + Sink<T, Error = E>,
    T,
    U,
    E,
    F: ReplyBufferFilter<U, Reply = T>,
> Stream for WithReply<S, T, F>
{
    type Item = Result<F::Filtered, E>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.wakers.next.register(cx.waker());
        loop {
            ready!(self.as_mut().poll_reply())?;
            self.as_mut().poll_flush_if_needed()?;
            let Some(item) = ready!(self.as_mut().project().stream.try_poll_next(cx)?) else {
                break Poll::Ready(None);
            };
            let Some((reply, item)) = self.as_mut().project().filter.reply_and_item(item) else {
                break Poll::Ready(None);
            };
            if let Some(reply) = reply {
                self.as_mut().set_reply(reply);
            }
            if let Some(item) = item {
                break Poll::Ready(Some(Ok(item)));
            }
        }
    }
}

impl<S: Sink<T, Error = E>, T, E, F> Sink<T> for WithReply<S, T, F> {
    type Error = E;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.wakers.ready.register(cx.waker());
        ready!(self.as_mut().poll())?;
        self.project().stream.poll_ready(cx)
    }

    fn start_send(self: Pin<&mut Self>, item: T) -> Result<(), Self::Error> {
        assert!(self.start.is_none());
        *self.project().start = Some(item);
        Ok(())
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.wakers.flush.register(cx.waker());
        ready!(self.as_mut().poll())?;
        self.poll_flush_raw(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.wakers.flush.register(cx.waker());
        ready!(self.as_mut().poll())?;
        self.project().stream.poll_close(cx)
    }
}

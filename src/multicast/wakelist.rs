use std::{
    collections::VecDeque,
    convert::Infallible,
    pin::Pin,
    task::{Context, Poll},
};

use extend_pinned::ExtendPinned;
use futures_util::{Sink, Stream, TryStream, stream::FusedStream};
use ruchei_extend::{Extending, ExtendingExt};
use ruchei_wakelist::{Queue, Ref};

use crate::connection_item::ConnectionItem;

const OP_WAKE_NEXT: usize = 0;
const OP_WAKE_READY: usize = 1;
const OP_WAKE_FLUSH: usize = 2;
const OP_WAKE_CLOSE: usize = 3;
const OP_WAKE: usize = 4;
const OP_IS_STARTED: usize = 4;
const OP_IS_READIED: usize = 5;
const OP_IS_FLUSHING: usize = 6;
const OP_COUNT: usize = 7;

#[derive(Debug)]
pub struct Multicast<S, E = <S as TryStream>::Error> {
    connections: Queue<S, OP_WAKE, OP_COUNT>,
    closed: VecDeque<(S, Option<E>)>,
}

impl<S, E> Default for Multicast<S, E> {
    fn default() -> Self {
        Self {
            connections: Default::default(),
            closed: Default::default(),
        }
    }
}

impl<S, E> Unpin for Multicast<S, E> {}

type Key<S> = Ref<S, OP_WAKE, OP_COUNT>;

impl<S, E> Multicast<S, E> {
    fn remove(&mut self, key: Key<S>, error: Option<E>) {
        let connection = self.connections.remove(&key).expect("unknown connection");
        self.closed.push_back((connection, error));
        self.connections.wake::<OP_WAKE_NEXT>();
        self.connections.wake::<OP_WAKE_READY>();
        self.connections.wake::<OP_WAKE_FLUSH>();
        self.connections.wake::<OP_WAKE_CLOSE>();
    }

    pub fn push(&mut self, stream: S) {
        let key = self.connections.insert(stream);
        self.connections.wake_push::<OP_WAKE_NEXT>(&key);
        self.connections.wake_push::<OP_WAKE_READY>(&key);
        self.connections.wake_push::<OP_WAKE_CLOSE>(&key);
    }
}

impl<In, E, S: TryStream<Ok = In, Error = E>> Stream for Multicast<S, E> {
    type Item = ConnectionItem<S>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if let Some((stream, error)) = this.closed.pop_front() {
            return Poll::Ready(Some(ConnectionItem::Closed(stream, error)));
        }
        this.connections.queue_poll::<OP_WAKE_NEXT>(cx);
        while let Some(key) = this.connections.link_pop_front::<OP_WAKE_NEXT>() {
            let (connection, waker) = this.connections.context::<OP_WAKE_NEXT>(&key);
            if let Poll::Ready(o) = connection.try_poll_next(&mut Context::from_waker(&waker)) {
                match o {
                    Some(Ok(item)) => {
                        this.connections.wake_push::<OP_WAKE_NEXT>(&key);
                        return Poll::Ready(Some(ConnectionItem::Item(item)));
                    }
                    Some(Err(e)) => {
                        this.remove(key, Some(e));
                    }
                    None => {
                        this.remove(key, None);
                    }
                }
            }
        }
        if this.connections.is_empty() {
            Poll::Ready(None)
        } else {
            Poll::Pending
        }
    }
}

impl<In, E, S: Unpin + TryStream<Ok = In, Error = E>> FusedStream for Multicast<S, E> {
    fn is_terminated(&self) -> bool {
        self.connections.is_empty()
    }
}

impl<Out: Clone, E, S: Unpin + Sink<Out, Error = E>> Sink<Out> for Multicast<S, E> {
    type Error = Infallible;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        this.connections.queue_poll::<OP_WAKE_READY>(cx);
        while let Some(key) = this.connections.link_pop_front::<OP_WAKE_READY>() {
            if !this.connections.link_contains::<OP_IS_READIED>(&key)
                && let (connection, waker) = this.connections.context::<OP_WAKE_READY>(&key)
                && let Poll::Ready(r) = connection.poll_ready(&mut Context::from_waker(&waker))
            {
                if let Err(e) = r {
                    this.remove(key, Some(e));
                } else {
                    this.connections.link_push_back::<OP_IS_READIED>(&key);
                }
            }
        }
        if this.connections.link_len::<OP_IS_READIED>() == this.connections.len() {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }

    fn start_send(self: Pin<&mut Self>, item: Out) -> Result<(), Self::Error> {
        let this = self.get_mut();
        while let Some(key) = this.connections.link_pop_front::<OP_IS_READIED>() {
            let connection = this.connections.index_pin_mut(&key);
            if let Err(e) = connection.start_send(item.clone()) {
                this.remove(key, Some(e));
            } else {
                this.connections.link_push_back::<OP_IS_STARTED>(&key);
                if this.connections.link_pop_at::<OP_IS_FLUSHING>(&key) {
                    this.connections.wake::<OP_WAKE_FLUSH>();
                }
                this.connections.link_push_back::<OP_WAKE_READY>(&key);
            }
        }
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        this.connections.queue_poll::<OP_WAKE_FLUSH>(cx);
        while let Some(key) = this.connections.link_pop_front::<OP_IS_STARTED>() {
            this.connections.link_push_back::<OP_WAKE_FLUSH>(&key);
        }
        while let Some(key) = this.connections.link_pop_front::<OP_WAKE_FLUSH>() {
            let (connection, waker) = this.connections.context::<OP_WAKE_FLUSH>(&key);
            match connection.poll_flush(&mut Context::from_waker(&waker)) {
                Poll::Ready(Ok(())) => {
                    this.connections.link_pop_at::<OP_IS_FLUSHING>(&key);
                }
                Poll::Ready(Err(e)) => {
                    this.remove(key, Some(e));
                }
                Poll::Pending => {
                    this.connections.link_push_back::<OP_IS_FLUSHING>(&key);
                }
            }
        }
        if this.connections.link_empty::<OP_IS_FLUSHING>() {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        this.connections.queue_poll::<OP_WAKE_CLOSE>(cx);
        while let Some(key) = this.connections.link_pop_front::<OP_WAKE_CLOSE>() {
            let (connection, waker) = this.connections.context::<OP_WAKE_CLOSE>(&key);
            if let Poll::Ready(r) = connection.poll_close(&mut Context::from_waker(&waker)) {
                match r {
                    Ok(()) => {
                        this.remove(key, None);
                    }
                    Err(e) => {
                        this.remove(key, Some(e));
                    }
                }
            }
        }
        if this.connections.is_empty() {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }
}

impl<S, E> Extend<S> for Multicast<S, E> {
    fn extend<T: IntoIterator<Item = S>>(&mut self, iter: T) {
        for stream in iter {
            self.push(stream);
        }
    }
}

impl<S, E> ExtendPinned<S> for Multicast<S, E> {
    fn extend_pinned<T: IntoIterator<Item = S>>(self: Pin<&mut Self>, iter: T) {
        self.get_mut().extend(iter);
    }
}

pub type MulticastExtending<R> = Extending<Multicast<<R as Stream>::Item>, R>;

pub trait MulticastWakeList: Sized + Stream<Item: TryStream> {
    #[must_use]
    fn multicast_wakelist(self) -> MulticastExtending<Self> {
        self.extending_default()
    }
}

impl<S: Unpin + TryStream, R: Stream<Item = S>> MulticastWakeList for R {}

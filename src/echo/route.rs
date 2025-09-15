use std::{
    collections::{HashMap, VecDeque},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use futures_util::{Future, TryStream};
use pin_project::pin_project;
use ruchei_collections::{as_linked_slab::AsLinkedSlab, linked_slab::LinkedSlab};
use ruchei_route::RouteSink;

use crate::{
    ready_slab::{ConnectionWaker, Ready},
    route::Key,
};

const OP_WAKE_SEND: usize = 0;
const OP_COUNT: usize = 1;

struct Connection<K, T> {
    key: K,
    send: Arc<ConnectionWaker>,
    msgs: VecDeque<T>,
}

#[pin_project]
#[must_use = "futures must be awaited"]
pub struct Echo<S, K, T> {
    #[pin]
    router: S,
    connections: LinkedSlab<Connection<K, T>, OP_COUNT>,
    map: HashMap<K, usize>,
    #[pin]
    send: Ready,
}

impl<K: Key, T, S> Echo<S, K, T> {
    fn remove(self: Pin<&mut Self>, ix: usize) {
        let this = self.project();
        let key = this.connections.remove(ix).key;
        assert_eq!(this.map.remove(&key).expect("unknown key"), ix);
    }

    fn push(self: Pin<&mut Self>, key: K, msg: T) {
        let this = self.project();
        if let Some(&ix) = this.map.get(&key) {
            this.connections[ix].msgs.push_back(msg);
        } else {
            let ix = this.connections.vacant_key();
            let send = this.send.downgrade();
            let connection = Connection {
                key: key.clone(),
                send: ConnectionWaker::new(ix, send),
                msgs: [msg].into(),
            };
            assert_eq!(this.connections.insert(connection), ix);
            this.map.insert(key, ix);
            this.send.downgrade().insert(ix);
        }
    }
}

impl<K: Key, T, E, S: TryStream<Ok = (K, T), Error = E> + RouteSink<K, T, Error = E>> Future
    for Echo<S, K, T>
{
    type Output = Result<(), E>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.as_mut().project();
        while let Poll::Ready(o) = this.router.as_mut().try_poll_next(cx)? {
            if let Some((key, msg)) = o {
                self.as_mut().push(key, msg);
                this = self.as_mut().project();
            } else {
                return Poll::Ready(Ok(()));
            }
        }
        while let Some(ix) = this.send.as_mut().next::<OP_WAKE_SEND>(this.connections) {
            let mut ready = false;
            while let Some(connection) = this.connections.get_mut(ix) {
                if connection.msgs.is_empty() {
                    match connection.send.poll(cx, |cx| {
                        this.router.as_mut().poll_flush(&connection.key, cx)
                    })? {
                        Poll::Ready(()) => {
                            ready = true;
                            self.as_mut().remove(ix);
                            this = self.as_mut().project();
                        }
                        Poll::Pending => {
                            break;
                        }
                    }
                } else {
                    match connection.send.poll(cx, |cx| {
                        this.router.as_mut().poll_ready(&connection.key, cx)
                    })? {
                        Poll::Ready(()) => {
                            ready = true;
                            this.router.as_mut().start_send(
                                connection.key.clone(),
                                connection
                                    .msgs
                                    .pop_front()
                                    .expect("no first item but not empty?"),
                            )?;
                        }
                        Poll::Pending => {
                            break;
                        }
                    }
                }
            }
            if ready
                && !this.router.is_routing()
                && let Some(&ix) = this.map.values().next()
            {
                this.send.downgrade().insert(ix);
            }
        }
        Poll::Pending
    }
}

pub trait EchoRoute: Sized {
    /// Per-connection unique key.
    type K;
    /// Item yielded and accepted by `self` as [`Stream`]/[`RouteSink`].
    type T;

    fn echo_route(self) -> Echo<Self, Self::K, Self::T> {
        Echo {
            router: self,
            connections: Default::default(),
            map: Default::default(),
            send: Default::default(),
        }
    }
}

impl<K: Key, T, E, S: TryStream<Ok = (K, T), Error = E> + RouteSink<K, T, Error = E>> EchoRoute
    for S
{
    type K = K;
    type T = T;
}

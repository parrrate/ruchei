use std::{
    collections::{HashMap, VecDeque},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use futures_util::{Future, TryStream};
use pin_project::pin_project;
use route_sink::ReadyRoute;
use ruchei_collections::{
    as_linked_slab::{AsLinkedSlab, SlabKey},
    linked_slab::LinkedSlab,
};
use ruchei_connection::{ConnectionWaker, Ready};

use crate::{merge::pair_item::PairItem, route::Key};

const OP_WAKE_SEND: usize = 0;
const OP_COUNT: usize = 1;

#[derive(Debug)]
struct Connection<K, T> {
    key: K,
    send: Arc<ConnectionWaker>,
    msgs: VecDeque<T>,
}

#[derive(Debug)]
#[pin_project]
#[must_use = "futures must be awaited"]
pub struct Echo<
    S,
    K = <<S as TryStream>::Ok as PairItem>::K,
    T = <<S as TryStream>::Ok as PairItem>::V,
> {
    #[pin]
    router: S,
    connections: LinkedSlab<Connection<K, T>, OP_COUNT>,
    map: HashMap<K, SlabKey>,
    #[pin]
    send: Ready,
}

impl<K: Key, T, S> Echo<S, K, T> {
    fn remove(self: Pin<&mut Self>, ix: SlabKey) {
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

impl<K: Key, T, E, S: TryStream<Ok = (K, T), Error = E> + ReadyRoute<K, T, Error = E>> Future
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
            while let Some(connection) = this.connections.get_mut(ix) {
                if connection.msgs.is_empty() {
                    match connection.send.poll(cx, |cx| {
                        this.router.as_mut().poll_flush_route(&connection.key, cx)
                    })? {
                        Poll::Ready(()) => {
                            self.as_mut().remove(ix);
                            this = self.as_mut().project();
                        }
                        Poll::Pending => {
                            break;
                        }
                    }
                } else {
                    match connection.send.poll(cx, |cx| {
                        this.router.as_mut().poll_ready_route(&connection.key, cx)
                    })? {
                        Poll::Ready(()) => {
                            this.router.as_mut().start_send((
                                connection.key.clone(),
                                connection
                                    .msgs
                                    .pop_front()
                                    .expect("no first item but not empty?"),
                            ))?;
                        }
                        Poll::Pending => {
                            break;
                        }
                    }
                }
            }
        }
        Poll::Pending
    }
}

pub trait EchoRoute:
    Sized
    + TryStream<Ok = (Self::K, Self::T), Error = Self::E>
    + ReadyRoute<Self::K, Self::T, Error = Self::E>
{
    type K: Key;
    type T;
    type E;

    fn echo_route(self) -> Echo<Self> {
        Echo {
            router: self,
            connections: Default::default(),
            map: Default::default(),
            send: Default::default(),
        }
    }
}

impl<K: Key, T, E, S: TryStream<Ok = (K, T), Error = E> + ReadyRoute<K, T, Error = E>> EchoRoute
    for S
{
    type K = K;
    type T = T;
    type E = E;
}

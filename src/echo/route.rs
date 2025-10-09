use std::{
    collections::{HashMap, VecDeque},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use futures_util::{Future, TryStream, ready};
use pin_project::pin_project;
use route_sink::ReadyRoute;
use ruchei_collections::{
    as_linked_slab::{AsLinkedSlab, SlabKey},
    linked_slab::LinkedSlab,
};
use ruchei_connection::{ConnectionWaker, Ready};

use crate::{merge::pair_item::PairItem, route::Key};

const OP_WAKE_SEND: usize = 0;
const OP_IS_READYING: usize = 1;
const OP_IS_FLUSHING: usize = 2;
const OP_COUNT: usize = 3;

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

impl<S: Default, K, T> Default for Echo<S, K, T> {
    fn default() -> Self {
        S::default().into()
    }
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
            if this.connections.link_pop_at::<OP_IS_FLUSHING>(ix) {
                assert!(this.connections.link_push_back::<OP_IS_READYING>(ix));
                this.send.downgrade().insert(ix);
            }
        } else {
            let ix = this.connections.vacant_key();
            let send = this.send.downgrade();
            let connection = Connection {
                key: key.clone(),
                send: ConnectionWaker::new(ix, send),
                msgs: [msg].into(),
            };
            this.connections.insert_at(ix, connection);
            this.map.insert(key, ix);
            assert!(this.connections.link_push_back::<OP_IS_READYING>(ix));
            this.send.downgrade().insert(ix);
        }
    }

    fn poll_connection<E>(
        mut self: Pin<&mut Self>,
        ix: SlabKey,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), E>>
    where
        S: ReadyRoute<K, T, Error = E>,
    {
        let mut this = self.as_mut().project();
        while let Some(connection) = this.connections.get_mut(ix) {
            if connection.msgs.is_empty() {
                ready!(connection.send.poll(cx, |cx| {
                    this.router.as_mut().poll_flush_route(&connection.key, cx)
                }))?;
                assert!(this.connections.link_pop_at::<OP_IS_FLUSHING>(ix));
                self.as_mut().remove(ix);
                this = self.as_mut().project();
            } else {
                ready!(connection.send.poll(cx, |cx| {
                    this.router.as_mut().poll_ready_route(&connection.key, cx)
                }))?;
                this.router.as_mut().start_send((
                    connection.key.clone(),
                    connection
                        .msgs
                        .pop_front()
                        .expect("no first item but not empty?"),
                ))?;
                let empty = connection.msgs.is_empty();
                assert!(this.connections.link_pop_at::<OP_IS_READYING>(ix));
                if empty {
                    assert!(this.connections.link_push_back::<OP_IS_FLUSHING>(ix));
                } else {
                    assert!(this.connections.link_push_back::<OP_IS_READYING>(ix));
                }
            }
        }
        Poll::Ready(Ok(()))
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
            let _: Poll<()> = self.as_mut().poll_connection(ix, cx)?;
            this = self.as_mut().project();
        }
        Poll::Pending
    }
}

impl<S, K, T> From<S> for Echo<S, K, T> {
    fn from(router: S) -> Self {
        Echo {
            router,
            connections: Default::default(),
            map: Default::default(),
            send: Default::default(),
        }
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
        self.into()
    }
}

impl<K: Key, T, E, S: TryStream<Ok = (K, T), Error = E> + ReadyRoute<K, T, Error = E>> EchoRoute
    for S
{
    type K = K;
    type T = T;
    type E = E;
}

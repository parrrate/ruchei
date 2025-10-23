//! Grouping of items by key.
//!
//! Sends new items to a group as long as it is alive. Otherwise, creates a new group.
//!
//! ## Example
//! * First message is treated as a group key.
//! * All messages within a group are replayed on new connections joining.
//! * When all participants leave, the history is cleared (by deleting the group).
//!
//! ```rust
//! # mod __ {
//! # use std::marker::PhantomData;
//! # use async_net::TcpListener;
//! # use futures_channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
//! # use futures_util::{future::ready, StreamExt};
//! # use ruchei::{
//! #     concurrent::ConcurrentExt,
//! #     connection_item::ConnectionItemExt,
//! #     echo::buffered::EchoBuffered,
//! #     liveness::{group_concurrent::{Group, GroupConcurrent}, timeout_unused::TimeoutUnused},
//! #     multicast::replay::MulticastReplay,
//! #     poll_on_wake::PollOnWakeExt,
//! # };
//! /* imports omitted */
//!
//! struct ChannelGroup<Item>(PhantomData<Item>);
//!
//! impl<Item> Group for ChannelGroup<Item> {
//!     type Item = Item;
//!
//!     type Sender = UnboundedSender<Item>;
//!
//!     type Receiver = UnboundedReceiver<Item>;
//!
//!     fn send(&mut self, sender: &mut Self::Sender, item: Self::Item) {
//!         let _ = sender.unbounded_send(item);
//!     }
//!
//!     fn pair(&mut self) -> (Self::Sender, Self::Receiver) {
//!         unbounded()
//!     }
//! }
//!
//! #[async_std::main]
//! async fn main() {
//!     TcpListener::bind("127.0.0.1:8080")
//!         .await
//!         .unwrap()
//!         .incoming()
//!         .poll_on_wake()
//!         .filter_map(|r| async { r.ok() })
//!         .map(|stream| async move {
//!             let mut stream = async_tungstenite::accept_async(stream).await.ok()?;
//!             // First message is treated as group key
//!             let group = stream.next().await?.ok()?;
//!             Some((group, stream))
//!         })
//!         .concurrent()
//!         .filter_map(|o| async move {
//!             o.map(|(group, s)| (group.into_data(), s.poll_on_wake()))
//!         })
//!         .group_concurrent(ChannelGroup(PhantomData))
//!         .for_each_concurrent(None, |(receiver, guard)| async move {
//!             let _guard = guard;
//!             receiver
//!                 // When all participants leave, the group is deleted
//!                 .timeout_unused(|| ready(()))
//!                 // All messages within a group are replayed on new connection joining
//!                 .multicast_replay()
//!                 .connection_item_ignore()
//!                 .echo_buffered()
//!                 .await
//!                 .unwrap();
//!         })
//!         .await;
//! }
//! # }
//! ```

use std::{
    collections::HashMap,
    hash::Hash,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use futures_util::{
    Stream,
    lock::{Mutex, OwnedMutexGuard, OwnedMutexLockFuture},
    ready,
    stream::FuturesUnordered,
};
use pin_project::pin_project;

use crate::merge::pair_item::PairStream;

/// Closes the [`Group`] when [`Drop`]ped.
#[derive(Debug)]
#[must_use = "GroupGuard must be dropped explicitly"]
pub struct GroupGuard<K>(OwnedMutexGuard<K>);

impl<K> AsRef<K> for GroupGuard<K> {
    fn as_ref(&self) -> &K {
        &self.0
    }
}

/// Strategy for grouping items.
pub trait Group {
    type Item;

    /// [`Grouped`] holds this
    type Sender;

    /// [`Grouped`] yields this
    type Receiver;

    /// [`Grouped`] calls this when the group already exists
    fn send(&mut self, sender: &mut Self::Sender, item: Self::Item);

    /// [`Grouped`] calls this when a new group is created
    #[must_use]
    fn pair(&mut self) -> (Self::Sender, Self::Receiver);
}

#[derive(Debug)]
#[pin_project]
pub struct Grouped<S, G, Sender = <G as Group>::Sender, K = <S as PairStream>::K> {
    #[pin]
    stream: S,
    #[pin]
    select: FuturesUnordered<OwnedMutexLockFuture<K>>,
    senders: HashMap<K, Sender>,
    group: G,
}

impl<S: Default, G: Default, Sender, K> Default for Grouped<S, G, Sender, K> {
    fn default() -> Self {
        S::default().into()
    }
}

impl<
    Item,
    Sender,
    K: Eq + Hash + Clone,
    S: Stream<Item = (K, Item)>,
    G: Group<Item = Item, Sender = Sender>,
> Stream for Grouped<S, G, Sender, K>
{
    type Item = (G::Receiver, GroupGuard<K>);

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        while let Poll::Ready(Some(key)) = this.select.as_mut().poll_next(cx) {
            this.senders.remove(&key);
        }
        while let Some((key, item)) = ready!(this.stream.as_mut().poll_next(cx)) {
            if let Some(sender) = this.senders.get_mut(&key) {
                this.group.send(sender, item);
            } else {
                let (mut sender, receiver) = this.group.pair();
                this.group.send(&mut sender, item);
                let mutex = Arc::new(Mutex::new(key.clone()));
                let guard = mutex.clone().try_lock_owned().unwrap();
                this.select.push(mutex.lock_owned());
                this.senders.insert(key, sender);
                return Poll::Ready(Some((receiver, GroupGuard(guard))));
            }
        }
        Poll::Ready(None)
    }
}

impl<S, G, Sender, K> Grouped<S, G, Sender, K> {
    #[must_use]
    pub fn new(stream: S, group: G) -> Self {
        Self {
            stream,
            select: Default::default(),
            senders: Default::default(),
            group,
        }
    }
}

impl<S, G: Default, Sender, K> From<S> for Grouped<S, G, Sender, K> {
    fn from(stream: S) -> Self {
        Grouped::new(stream, Default::default())
    }
}

pub trait GroupConcurrent: Sized + Stream<Item = (Self::Key, Self::GroupItem)> {
    type Key: Eq + Hash + Clone;
    type GroupItem;

    #[must_use]
    fn group_concurrent<G: Group<Item = Self::GroupItem>>(self, group: G) -> Grouped<Self, G> {
        Grouped::new(self, group)
    }
}

impl<Item, K: Eq + Hash + Clone, S: Stream<Item = (K, Item)>> GroupConcurrent for S {
    type Key = K;
    type GroupItem = Item;
}

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
//! #     echo::buffered::EchoBuffered,
//! #     group_concurrent::{Group, GroupConcurrent},
//! #     multi_item::MultiItemExt,
//! #     multicast::replay_slab::MulticastReplaySlab,
//! #     poll_on_wake::PollOnWakeExt,
//! #     timeout_unused::TimeoutUnused,
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
//!         .fuse()
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
//!                 .multicast_replay_slab()
//!                 .multi_item_ignore()
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

/// Closes the [`Group`] when [`Drop`]ped.
#[derive(Debug)]
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
    fn pair(&mut self) -> (Self::Sender, Self::Receiver);
}

#[derive(Debug)]
#[pin_project]
pub struct Grouped<S, Sender, K, G> {
    #[pin]
    stream: S,
    #[pin]
    select: FuturesUnordered<OwnedMutexLockFuture<K>>,
    senders: HashMap<K, Sender>,
    group: G,
}

impl<
    Item,
    Sender,
    K: Eq + Hash + Clone,
    S: Stream<Item = (K, Item)>,
    G: Group<Item = Item, Sender = Sender>,
> Stream for Grouped<S, Sender, K, G>
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

pub trait GroupConcurrent: Sized {
    type Item;

    type Key;

    #[must_use]
    fn group_concurrent<G: Group<Item = Self::Item>>(
        self,
        group: G,
    ) -> Grouped<Self, G::Sender, Self::Key, G>;
}

impl<Item, K: Eq + Hash + Clone, S: Stream<Item = (K, Item)>> GroupConcurrent for S {
    type Item = Item;

    type Key = K;

    fn group_concurrent<G: Group<Item = Self::Item>>(
        self,
        group: G,
    ) -> Grouped<Self, G::Sender, Self::Key, G> {
        Grouped {
            stream: self,
            select: Default::default(),
            senders: Default::default(),
            group,
        }
    }
}

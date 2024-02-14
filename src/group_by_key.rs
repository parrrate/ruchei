use std::{
    collections::HashMap,
    hash::Hash,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use futures_util::{
    lock::{Mutex, OwnedMutexGuard, OwnedMutexLockFuture},
    ready,
    stream::FuturesUnordered,
    Stream,
};
use pin_project::pin_project;

#[derive(Debug)]
pub struct GroupGuard<K>(OwnedMutexGuard<K>);

impl<K> AsRef<K> for GroupGuard<K> {
    fn as_ref(&self) -> &K {
        &self.0
    }
}

pub trait Group {
    type Item;

    type Sender;

    type Receiver;

    fn send(&mut self, sender: &mut Self::Sender, item: Self::Item);

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

pub trait GroupByKey: Sized {
    type Item;

    type Key;

    fn group_by_key<G: Group<Item = Self::Item>>(
        self,
        group: G,
    ) -> Grouped<Self, G::Sender, Self::Key, G>;
}

impl<Item, Key, S: Stream<Item = (Key, Item)>> GroupByKey for S {
    type Item = Item;

    type Key = Key;

    fn group_by_key<G: Group<Item = Self::Item>>(
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

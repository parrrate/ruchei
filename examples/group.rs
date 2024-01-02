use std::marker::PhantomData;

use async_std::net::TcpListener;
use futures_channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use futures_util::{future::ready, StreamExt};
use ruchei::{
    concurrent::ConcurrentExt,
    echo_buffered::EchoBuffered,
    fanout_replay::MulticastReplay,
    group_by_key::{Group, GroupByKey},
    poll_on_wake::PollOnWakeExt,
    timeout_unused::TimeoutUnused,
};

struct ChannelGroup<Item>(PhantomData<Item>);

impl<Item> Group for ChannelGroup<Item> {
    type Item = Item;

    type Sender = UnboundedSender<Item>;

    type Receiver = UnboundedReceiver<Item>;

    fn send(&mut self, sender: &mut Self::Sender, item: Self::Item) {
        let _ = sender.unbounded_send(item);
    }

    fn pair(&mut self) -> (Self::Sender, Self::Receiver) {
        unbounded()
    }
}

#[async_std::main]
async fn main() {
    TcpListener::bind("127.0.0.1:8080")
        .await
        .unwrap()
        .incoming()
        .poll_on_wake()
        .filter_map(|r| async { r.ok() })
        .map(async_tungstenite::accept_async)
        .fuse()
        .concurrent()
        .filter_map(|r| async { r.ok() })
        .map(|s| s.poll_on_wake())
        .map(|s| ("group", s))
        .group_by_key(ChannelGroup(PhantomData))
        .for_each_concurrent(None, |(receiver, guard)| async move {
            let _guard = guard;
            receiver
                .timeout_unused(|| ready(()))
                .multicast_replay(|_| {})
                .echo_buffered()
                .await
                .unwrap();
        })
        .await;
}

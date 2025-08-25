//! [`ruchei::group_concurrent`] and [`ruchei::timeout_unused`]

use std::marker::PhantomData;

use async_net::TcpListener;
use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use futures_util::{StreamExt, future::ready};
use ruchei::{
    concurrent::ConcurrentExt,
    echo::buffered::EchoBuffered,
    group_concurrent::{Group, GroupConcurrent},
    multicast::replay::MulticastReplay,
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
        .map(|stream| async move {
            let mut stream = async_tungstenite::accept_async(stream).await.ok()?;
            let group = stream.next().await?.ok()?;
            Some((group, stream))
        })
        .fuse()
        .concurrent()
        .filter_map(|o| async move { o.map(|(group, s)| (group.into_data(), s.poll_on_wake())) })
        .group_concurrent(ChannelGroup(PhantomData))
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

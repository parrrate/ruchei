//! [`ruchei::multicast::replay`] with [`ruchei::echo::interleaved`]

use async_net::TcpListener;
use futures_util::StreamExt;
use ruchei::{
    concurrent::ConcurrentExt, echo::interleaved::EchoInterleaved, multi_item::MultiItemExt,
    multicast::replay_slab::MulticastReplaySlab, poll_on_wake::PollOnWakeExt,
};

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
        .multicast_replay_slab()
        .multi_item_ignore()
        .echo_interleaved()
        .await
        .unwrap();
}

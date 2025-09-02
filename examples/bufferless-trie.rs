//! [`ruchei::multicast::bufferless_keyed`] with [`ruchei::echo::buffered`]

use async_net::TcpListener;
use futures_util::{StreamExt, TryStreamExt};
use ruchei::{
    concurrent::ConcurrentExt,
    echo::bufferless::EchoBufferless,
    multicast::trie::{MulticastTrie, SubRequest},
    poll_on_wake::PollOnWakeExt,
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
        .map(|s| s.map_ok(SubRequest::<String, _>::Other))
        .multicast_trie(|_| {})
        .echo_bufferless()
        .await
        .unwrap();
}

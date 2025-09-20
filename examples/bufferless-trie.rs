//! [`ruchei::multicast::bufferless_keyed`] with [`ruchei::echo::buffered`]

use async_net::TcpListener;
use futures_util::{SinkExt, StreamExt, TryStreamExt, future::ready};
use ruchei::{
    concurrent::ConcurrentExt,
    connection_item::ConnectionItemExt,
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
        .map(|s| {
            s.map_ok(|m| {
                futures_util::stream::iter([SubRequest::Sub("prefix"), SubRequest::Other(m)])
                    .map(Ok)
            })
            .try_flatten()
        })
        .map(|s| s.with(|(_, m)| ready(Ok::<_, async_tungstenite::tungstenite::Error>(m))))
        .multicast_trie()
        .connection_item_ignore()
        .map_ok(|m| ("prefix", m))
        .echo_bufferless()
        .await
        .unwrap();
}

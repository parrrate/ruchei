//! [`ruchei::multicast::buffered`] with [`ruchei::echo::buffered`]

use async_net::TcpListener;
use futures_util::StreamExt;
use ruchei::{
    concurrent::ConcurrentExt, connection_item::ConnectionItemExt, echo::buffered::EchoBuffered,
    multicast::buffered_wakelist::MulticastBufferedWl, poll_on_wake::PollOnWakeExt,
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
        .concurrent()
        .filter_map(|r| async { r.ok() })
        .multicast_buffered_wakelist()
        .connection_item_ignore()
        .echo_buffered()
        .await
        .unwrap();
}

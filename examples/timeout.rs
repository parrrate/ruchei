//! [`ruchei::timeout_unused`]

use std::time::Duration;

use async_std::{net::TcpListener, task};
use futures_util::StreamExt;
use ruchei::{
    concurrent::ConcurrentExt, connection_item::ConnectionItemExt, echo::buffered::EchoBuffered,
    liveness::timeout_unused::TimeoutUnused, multicast::replay::MulticastReplay,
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
        .concurrent()
        .filter_map(|r| async { r.ok() })
        .timeout_unused(|| async {
            task::sleep(Duration::from_secs(10)).await;
        })
        .map(|s| s.poll_on_wake())
        .multicast_replay()
        .connection_item_ignore()
        .echo_buffered()
        .await
        .unwrap();
}

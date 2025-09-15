//! [`ruchei::timeout_unused`]

use std::time::Duration;

use async_std::{net::TcpListener, task};
use futures_util::StreamExt;
use ruchei::{
    concurrent::ConcurrentExt, echo::buffered::EchoBuffered, multicast::replay::MulticastReplay,
    poll_on_wake::PollOnWakeExt, timeout_unused::TimeoutUnused,
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
        .timeout_unused(|| async {
            task::sleep(Duration::from_secs(10)).await;
        })
        .map(|s| s.poll_on_wake())
        .multicast_replay(|_| {})
        .map(Ok)
        .echo_buffered()
        .await
        .unwrap();
}

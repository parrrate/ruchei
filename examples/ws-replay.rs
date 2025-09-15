//! [`ruchei::multicast::replay`] with [`ruchei::echo::buffered`]

use async_net::TcpListener;
use futures_util::{StreamExt, future::ready};
use ruchei::{
    concurrent::ConcurrentExt, echo::buffered::EchoBuffered, multicast::replay::MulticastReplay,
    poll_on_wake::PollOnWakeExt,
};

#[async_std::main]
async fn main() {
    TcpListener::bind("127.0.0.1:8080")
        .await
        .unwrap()
        .incoming()
        .poll_on_wake()
        .filter_map(|r| ready(r.ok()))
        .map(async_tungstenite::accept_async)
        .fuse()
        .concurrent()
        .filter_map(|r| ready(r.ok()))
        .map(|s| s.poll_on_wake())
        .multicast_replay(|_| {})
        .map(Ok)
        .echo_buffered()
        .await
        .unwrap();
}

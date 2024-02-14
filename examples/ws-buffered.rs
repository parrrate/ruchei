use async_std::net::TcpListener;
use futures_util::StreamExt;
use ruchei::{
    concurrent::ConcurrentExt, echo::buffered::EchoBuffered, fanout_buffered::MulticastBuffered,
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
        .multicast_buffered(|_| {})
        .echo_buffered()
        .await
        .unwrap();
}

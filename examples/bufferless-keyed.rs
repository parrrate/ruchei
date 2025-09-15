//! [`ruchei::multicast::bufferless_keyed`] with [`ruchei::echo::buffered`]

use async_net::TcpListener;
use futures_util::StreamExt;
use ruchei::{
    concurrent::ConcurrentExt, echo::bufferless::EchoBufferless,
    multicast::bufferless_keyed::MulticastBufferlessKeyed, poll_on_wake::PollOnWakeExt,
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
        .map(|s| (rand::random::<u64>(), s))
        .multicast_bufferless_keyed(|_| {})
        .map(Ok)
        .echo_bufferless()
        .await
        .unwrap();
}

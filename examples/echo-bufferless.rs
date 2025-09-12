//! [`ruchei::multicast::bufferless`] with [`ruchei::echo::bufferless`]

use async_net::TcpListener;
use futures_util::StreamExt;
use ruchei::{
    concurrent::ConcurrentExt,
    echo::bufferless::EchoBufferless,
    multicast::bufferless::MulticastBufferless,
    poll_on_wake::PollOnWakeExt,
    rw_isolation::{IsolateInner, IsolateOuter, isolation},
};

#[async_std::main]
async fn main() {
    let (inner, outer) = isolation();
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
        .map(|s| s.isolate_inner(inner.clone()).poll_on_wake())
        .multicast_bufferless(|_| {})
        .isolate_outer(outer)
        .echo_bufferless()
        .await
        .unwrap();
}

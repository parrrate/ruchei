use async_std::net::TcpListener;
use futures_util::StreamExt;
use ruchei::{
    concurrent::ConcurrentExt,
    echo_simple::EchoSimple,
    fanout_bufferless::MulticastBufferless,
    rw_isolation::{isolation, IsolateInner, IsolateOuter},
};

#[async_std::main]
async fn main() {
    let (inner, outer) = isolation();
    TcpListener::bind("127.0.0.1:8080")
        .await
        .unwrap()
        .incoming()
        .filter_map(|r| async { r.ok() })
        .map(async_tungstenite::accept_async)
        .fuse()
        .concurrent()
        .filter_map(|r| async { r.ok() })
        .map(|s| s.isolate_inner(inner.clone()))
        .multicast_bufferless(|_| {})
        .isolate_outer(outer)
        .echo_simple()
        .await
        .unwrap();
}

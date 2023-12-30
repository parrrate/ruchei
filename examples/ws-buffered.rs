use async_std::net::TcpListener;
use futures_util::StreamExt;
use ruchei::{concurrent::ConcurrentExt, echo::EchoExt, fanout_buffered::MulticastBuffered};

#[async_std::main]
async fn main() {
    let streams = TcpListener::bind("127.0.0.1:8080").await.unwrap();
    streams
        .incoming()
        .filter_map(|r| async { r.ok() })
        .map(async_tungstenite::accept_async)
        .concurrent()
        .filter_map(|r| async { r.ok() })
        .multicast_buffered(|_| {})
        .echo()
        .await
        .unwrap();
}

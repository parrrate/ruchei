//! [`ruchei::route`] with [`ruchei::echo::route`]

use async_net::TcpListener;
use futures_util::{SinkExt, StreamExt, TryStreamExt, future::ready};
use ruchei::{
    concurrent::ConcurrentExt, connection_item::ConnectionItemExt,
    echo::bufferless::EchoBufferless, poll_on_wake::PollOnWakeExt,
    route::multicast::RouteMulticast,
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
        .route_multicast()
        .connection_item_ignore()
        .map_ok(|(_, x)| x)
        .with(|x| ready(Ok((x,))))
        .fuse()
        .echo_bufferless()
        .await
        .unwrap();
}

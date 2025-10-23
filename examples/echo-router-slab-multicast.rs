//! [`ruchei::route`] with [`ruchei::echo::route`]

use async_net::TcpListener;
use futures_util::StreamExt;
use ruchei::{
    concurrent::ConcurrentExt, connection_item::ConnectionItemExt, echo::route::EchoRoute,
    poll_on_wake::PollOnWakeExt, route::multicast::RouteMulticast,
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
        .route_multicast()
        .connection_item_ignore()
        .echo_route()
        .await
        .unwrap();
}

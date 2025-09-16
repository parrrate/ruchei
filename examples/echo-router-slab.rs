//! [`ruchei::route`] with [`ruchei::echo::route`]

use async_net::TcpListener;
use futures_util::StreamExt;
use ruchei::{
    concurrent::ConcurrentExt, echo::route::EchoRoute, multi_item::MultiItemExt,
    poll_on_wake::PollOnWakeExt, route::slab::RouterSlabExt,
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
        .route_slab()
        .multi_item_ignore()
        .echo_route()
        .await
        .unwrap();
}

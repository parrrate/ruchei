//! [`ruchei::deal`]

use async_net::TcpListener;
use futures_util::StreamExt;
use ruchei::{
    concurrent::ConcurrentExt, connection_item::ConnectionItem,
    deal::without_multicast::DealWithoutMulticast, poll_on_wake::PollOnWakeExt,
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
        .deal_without_multicast()
        .for_each(|item| {
            match item {
                ConnectionItem::Item(item) => {
                    eprintln!("{item:?}");
                }
                ConnectionItem::Closed(_, _) => {
                    eprintln!("closed");
                }
            }
            async {}
        })
        .await;
}

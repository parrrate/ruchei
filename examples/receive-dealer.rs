//! [`ruchei::deal`]

use async_net::TcpListener;
use futures_util::StreamExt;
use ruchei::{
    concurrent::ConcurrentExt, deal::slab::DealerSlabExt, multi_item::MultiItem,
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
        .deal_slab()
        .for_each(|item| {
            match item {
                MultiItem::Item(item) => {
                    eprintln!("{item:?}");
                }
                MultiItem::Closed(_, _) => {
                    eprintln!("closed");
                }
            }
            async {}
        })
        .await;
}

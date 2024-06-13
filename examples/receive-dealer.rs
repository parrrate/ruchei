//! [`ruchei::deal`]

use async_std::net::TcpListener;
use futures_util::{StreamExt, TryStreamExt};
use ruchei::{
    concurrent::ConcurrentExt, deal::DealerExt,
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
        .map(|s| (rand::random::<u64>(), s))
        .deal(|_| {})
        .try_for_each(|msg| {
            eprintln!("{msg:?}");
            async { Ok(()) }
        })
        .await
        .unwrap();
}

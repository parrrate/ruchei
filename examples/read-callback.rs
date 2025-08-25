//! [`ruchei::read_callback`]

use std::{pin::pin, time::Duration};

use async_std::{net::TcpListener, task::sleep};
use futures_util::{SinkExt, StreamExt};
use ruchei::{
    concurrent::ConcurrentExt, multicast::buffered::MulticastBuffered,
    read_callback::ReadCallbackExt,
};

#[async_std::main]
async fn main() {
    let streams = TcpListener::bind("127.0.0.1:8080").await.unwrap();
    let mut sink = pin!(
        streams
            .incoming()
            .filter_map(|r| async { r.ok() })
            .map(async_tungstenite::accept_async)
            .fuse()
            .concurrent()
            .filter_map(|r| async { r.ok() })
            .multicast_buffered(|_| {})
            .read_callback(|message| print!("{message}"))
    );
    loop {
        sink.send("ping".into()).await.unwrap();
        sleep(Duration::from_secs(5)).await;
    }
}

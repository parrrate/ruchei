//! Read many messages.

use std::{pin::pin, time::Instant};

use async_std::stream::StreamExt;

#[async_std::main]
async fn main() {
    let mut stream = pin!(
        async_tungstenite::client_async(
            "ws://127.0.0.1:8080/",
            async_net::TcpStream::connect("127.0.0.1:8080")
                .await
                .unwrap()
        )
        .await
        .unwrap()
        .0
    );
    stream.next().await.unwrap().unwrap();
    let start = Instant::now();
    for _ in 1..1_000_000 {
        stream.next().await.unwrap().unwrap();
    }
    println!("{:?}", start.elapsed());
}

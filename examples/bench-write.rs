use std::{pin::pin, time::Instant};

use async_std::stream::StreamExt;
use futures_util::SinkExt;

#[async_std::main]
async fn main() {
    let mut stream = pin!(
        async_tungstenite::async_std::connect_async("ws://127.0.0.1:8080/")
            .await
            .unwrap()
            .0
    );
    stream.send("".into()).await.unwrap();
    let start = Instant::now();
    stream
        .send_all(&mut futures_util::stream::iter(
            (1..1_000_000).map(|i| Ok(format!("{i}").into())),
        ))
        .await
        .unwrap();
    println!("{:?}", start.elapsed());
    stream.next().await.unwrap().unwrap();
    let start = Instant::now();
    for _ in 1..1_000_000 {
        stream.next().await.unwrap().unwrap();
    }
    println!("{:?}", start.elapsed());
}

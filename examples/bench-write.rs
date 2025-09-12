//! Send-receive many messages concurrently.

use std::time::Instant;

use async_std::task;
use futures_util::{SinkExt, StreamExt};

#[async_std::main]
async fn main() {
    let stream = async_tungstenite::client_async(
        "ws://127.0.0.1:8080/",
        async_net::TcpStream::connect("127.0.0.1:8080")
            .await
            .unwrap(),
    )
    .await
    .unwrap()
    .0;
    let (mut writer, mut reader) = stream.split();
    let task = task::spawn(async move {
        reader.next().await.unwrap().unwrap();
        let start = Instant::now();
        for _ in 1..1_000_000 {
            reader.next().await.unwrap().unwrap();
        }
        println!("{:?}", start.elapsed());
    });
    writer.send("0".into()).await.unwrap();
    let start = Instant::now();
    writer
        .send_all(&mut futures_util::stream::iter(
            (1..1_000_000).map(|i| Ok(format!("{i}").into())),
        ))
        .await
        .unwrap();
    println!("{:?}", start.elapsed());
    task.await;
    writer.close().await.unwrap();
}

//! Send-receive many messages concurrently.

use std::time::{Duration, Instant};

use async_io::Timer;
use async_std::{channel::unbounded, task};
use futures_util::{SinkExt, StreamExt};

#[async_std::main]
async fn main() {
    let mut tasks = Vec::new();
    let (started_s, started_r) = unbounded();
    for _ in 0..16 {
        let started_s = started_s.clone();
        tasks.push(task::spawn(async move {
            let stream = async_tungstenite::client_async(
                "ws://127.0.0.1:8080/",
                async_net::TcpStream::connect("127.0.0.1:8080")
                    .await
                    .unwrap(),
            )
            .await
            .unwrap()
            .0;
            Timer::after(Duration::from_millis(500)).await;
            let (mut writer, mut reader) = stream.split();
            let task = task::spawn(async move {
                let n: i32 = reader
                    .next()
                    .await
                    .unwrap()
                    .unwrap()
                    .into_text()
                    .unwrap()
                    .parse()
                    .unwrap();
                assert_eq!(n, 0);
                let start = Instant::now();
                for i in 1..1_000_000 {
                    let n: i32 = reader
                        .next()
                        .await
                        .unwrap()
                        .unwrap()
                        .into_text()
                        .unwrap()
                        .parse()
                        .unwrap();
                    assert_eq!(i, n);
                }
                println!("{:?}", start.elapsed());
            });
            writer.send("0".into()).await.unwrap();
            let start = Instant::now();
            let _ = started_s.send(start).await;
            writer
                .send_all(&mut futures_util::stream::iter(
                    (1..1_000_000).map(|i| Ok(format!("{i}").into())),
                ))
                .await
                .unwrap();
            println!("{:?}", start.elapsed());
            task.await;
            writer.close().await.unwrap();
        }));
    }
    let start = started_r.recv().await.unwrap();
    for task in tasks {
        task.await;
    }
    println!("{:?} @ total", start.elapsed());
}

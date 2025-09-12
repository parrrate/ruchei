use std::{pin::pin, time::Instant};

use async_std::{channel::unbounded, stream::StreamExt, task};

#[async_std::main]
async fn main() {
    let mut tasks = Vec::new();
    let (started_s, started_r) = unbounded();
    for _ in 0..16 {
        let started_s = started_s.clone();
        tasks.push(task::spawn(async move {
            let mut stream = pin!(
                async_tungstenite::async_std::connect_async("ws://127.0.0.1:8080/")
                    .await
                    .unwrap()
                    .0
            );
            stream.next().await.unwrap().unwrap();
            let start = Instant::now();
            let _ = started_s.send(start).await;
            for _ in 1..1_000_000 {
                stream.next().await.unwrap().unwrap();
            }
            println!("{:?}", start.elapsed());
        }));
    }
    let start = started_r.recv().await.unwrap();
    for task in tasks {
        task.await;
    }
    println!("{:?} @ total", start.elapsed());
}

//! Send once, receive forever.

use std::pin::pin;

use async_std::stream::StreamExt;
use futures_util::SinkExt;

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
    stream.send("hi".into()).await.unwrap();
    loop {
        let msg = stream.next().await.unwrap().unwrap();
        println!("{msg}");
    }
}

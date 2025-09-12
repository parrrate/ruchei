use async_std::{net::TcpListener, task};
use ruchei::echo_buffered::EchoBuffered;

#[async_std::main]
async fn main() {
    let listener = TcpListener::bind("127.0.0.1:8080").await.unwrap();
    while let Ok((stream, _)) = listener.accept().await {
        task::spawn(async {
            async_tungstenite::accept_async(stream)
                .await
                .unwrap()
                .echo_buffered()
                .await
                .unwrap()
        });
    }
}

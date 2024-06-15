//! Client-side [`ruchei::echo::buffered`]

use ruchei::echo::buffered::EchoBuffered;

#[async_std::main]
async fn main() {
    async_tungstenite::client_async(
        "ws://127.0.0.1:8080/",
        async_net::TcpStream::connect("127.0.0.1:8080")
            .await
            .unwrap(),
    )
    .await
    .unwrap()
    .0
    .echo_buffered()
    .await
    .unwrap();
}

use ruchei::echo::buffered::EchoBuffered;

#[async_std::main]
async fn main() {
    async_tungstenite::async_std::connect_async("ws://127.0.0.1:8080/")
        .await
        .unwrap()
        .0
        .echo_buffered()
        .await
        .unwrap();
}

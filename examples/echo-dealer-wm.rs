use async_net::TcpListener;
use futures_util::{StreamExt, future::ready};
use ruchei::{
    concurrent::ConcurrentExt, connection_item::ConnectionItemExt,
    deal::without_multicast::DealWithoutMulticast, echo::bufferless::EchoBufferless,
    poll_on_wake::PollOnWakeExt,
};

#[async_std::main]
async fn main() {
    TcpListener::bind("127.0.0.1:8080")
        .await
        .unwrap()
        .incoming()
        .poll_on_wake()
        .filter_map(|r| async { r.ok() })
        .map(async_tungstenite::accept_async)
        .concurrent()
        .filter_map(|r| async { r.ok() })
        .map(|s| s.filter(|m| ready(m.as_ref().is_ok_and(|m| !m.is_close()))))
        .deal_without_multicast()
        .connection_item_ignore()
        .echo_bufferless()
        .await
        .unwrap();
}

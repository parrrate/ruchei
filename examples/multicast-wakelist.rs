//! [`ruchei::multicast::bufferless`] with [`ruchei::echo::buffered`]

use async_net::TcpListener;
use futures_util::{StreamExt, TryStreamExt, future::ready};
use ruchei::{
    concurrent::ConcurrentExt, connection_item::ConnectionItemExt,
    echo::bufferless::EchoBufferless, multicast::bufferless_wakelist::MulticastBufferlessWl,
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
        .multicast_bufferless_wakelist()
        .connection_item_ignore()
        .try_filter(|m| ready(!m.is_close()))
        .echo_bufferless()
        .await
        .unwrap();
}

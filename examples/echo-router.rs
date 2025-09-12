use std::pin::pin;

use async_std::net::TcpListener;
use futures_util::{SinkExt, Stream, StreamExt};
use ruchei::{concurrent::ConcurrentExt, poll_on_wake::PollOnWakeExt, route::RouterExt};
use ruchei_route::{RouteExt, RouteSink};

async fn with_router<
    K: Clone,
    T,
    E,
    S: Stream<Item = Result<(K, T), E>> + RouteSink<K, T, Error = E>,
>(
    mut router: S,
) -> Result<(), E> {
    let mut router = pin!(router);
    while let Some((key, msg)) = router.next().await.transpose()? {
        router.route(key).send(msg).await?;
    }
    Ok(())
}

#[async_std::main]
async fn main() {
    with_router(
        TcpListener::bind("127.0.0.1:8080")
            .await
            .unwrap()
            .incoming()
            .poll_on_wake()
            .filter_map(|r| async { r.ok() })
            .map(async_tungstenite::accept_async)
            .fuse()
            .concurrent()
            .filter_map(|r| async { r.ok() })
            .map(|s| s.poll_on_wake())
            .map(|s| (rand::random::<u128>(), s))
            .route(|_| {}),
    )
    .await
    .unwrap();
}

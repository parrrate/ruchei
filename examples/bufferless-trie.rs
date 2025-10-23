//! [`ruchei::multicast::bufferless`] with [`ruchei::echo::buffered`]

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use async_net::TcpListener;
use futures_util::{Sink, SinkExt, Stream, StreamExt, TryStream, TryStreamExt, future::ready};
use pin_project::pin_project;
use ruchei::{
    concurrent::ConcurrentExt,
    connection_item::ConnectionItemExt,
    echo::bufferless::EchoBufferless,
    multicast::trie::{MulticastTrie, SubRequest},
    poll_on_wake::PollOnWakeExt,
};

#[pin_project]
struct AutoSub<S> {
    #[pin]
    inner: S,
    subbed: bool,
}

impl<S> AutoSub<S> {
    fn new(inner: S) -> Self {
        Self {
            inner,
            subbed: false,
        }
    }
}

impl<S: TryStream> Stream for AutoSub<S> {
    type Item = Result<SubRequest<&'static str, S::Ok>, S::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        if !std::mem::replace(this.subbed, true) {
            Poll::Ready(Some(Ok(SubRequest::Sub("prefix"))))
        } else {
            this.inner.try_poll_next(cx).map_ok(SubRequest::Other)
        }
    }
}

impl<S: Sink<T>, T> Sink<T> for AutoSub<S> {
    type Error = S::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().inner.poll_ready(cx)
    }

    fn start_send(self: Pin<&mut Self>, item: T) -> Result<(), Self::Error> {
        self.project().inner.start_send(item)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().inner.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().inner.poll_close(cx)
    }
}

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
        .map(AutoSub::new)
        .map(|s| s.with(|(_, m)| ready(Ok::<_, async_tungstenite::tungstenite::Error>(m))))
        .multicast_trie()
        .connection_item_ignore()
        .map_ok(|m| ("prefix", m))
        .echo_bufferless()
        .await
        .unwrap();
}

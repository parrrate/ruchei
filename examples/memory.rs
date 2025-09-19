//! Various benchmarks for in-memory multicast.

use std::{
    pin::{Pin, pin},
    task::{Context, Poll},
    time::Instant,
};

use futures_channel::mpsc::{SendError, UnboundedReceiver, UnboundedSender, unbounded};
use futures_util::{
    Future, Sink, SinkExt, Stream, StreamExt,
    future::{select, select_all},
};
use pin_project::pin_project;
use ruchei::{
    echo::buffered::EchoBuffered,
    multi_item::MultiItemExt,
    multicast::{
        buffered::MulticastBuffered, bufferless::MulticastBufferless, replay::MulticastReplay,
    },
};

#[pin_project]
struct Channel<T> {
    #[pin]
    sender: UnboundedSender<T>,
    #[pin]
    receiver: UnboundedReceiver<T>,
}

impl<T> Stream for Channel<T> {
    type Item = Result<T, SendError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.project().receiver.poll_next(cx).map(|o| o.map(Ok))
    }
}

impl<T> Sink<T> for Channel<T> {
    type Error = SendError;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().sender.poll_ready(cx)
    }

    fn start_send(self: Pin<&mut Self>, item: T) -> Result<(), Self::Error> {
        self.project().sender.start_send(item)
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().receiver.close();
        Poll::Ready(Ok(()))
    }
}

fn channel<T>() -> (Channel<T>, Channel<T>) {
    let (sender0, receiver1) = unbounded();
    let (sender1, receiver0) = unbounded();
    (
        Channel {
            sender: sender0,
            receiver: receiver0,
        },
        Channel {
            sender: sender1,
            receiver: receiver1,
        },
    )
}

const N: usize = 100_000;

async fn single(mut remote: Channel<usize>, factor: usize) {
    for i in 0..N {
        remote.send(i).await.unwrap();
    }
    for _ in 0..N * factor {
        remote.next().await.unwrap().unwrap();
    }
}

fn setup<Fut: Future<Output = ()>>(
    f: impl Fn(Vec<Channel<usize>>) -> Fut,
) -> (UnboundedReceiver<Channel<usize>>, impl Future<Output = ()>) {
    let (streams_s, streams_r) = unbounded();
    let mut remotes = Vec::new();
    for _ in 0..10 {
        let (local, remote) = channel();
        remotes.push(remote);
        streams_s.unbounded_send(local).unwrap();
    }
    (streams_r, async move {
        let start = Instant::now();
        f(remotes).await;
        println!("{:?}", start.elapsed());
    })
}

async fn test_one<F1: Future<Output = ()>, F2: Future<Output = ()>>(
    f1: impl Fn(UnboundedReceiver<Channel<usize>>) -> F1,
    f2: impl Fn(Vec<Channel<usize>>) -> F2,
) {
    let (streams_r, future) = setup(f2);
    select(pin!(future), pin!(f1(streams_r))).await;
}

async fn test_sequential(mut remotes: Vec<Channel<usize>>) {
    for i in 0..N {
        for remote in remotes.iter_mut() {
            remote.send(i).await.unwrap();
        }
    }
    for _ in 0..N {
        for _ in 0..remotes.len() {
            for remote in remotes.iter_mut() {
                remote.next().await.unwrap().unwrap();
            }
        }
    }
}

async fn test_select(remotes: Vec<Channel<usize>>) {
    let factor = remotes.len();
    select_all(
        remotes
            .into_iter()
            .map(|remote| Box::pin(single(remote, factor))),
    )
    .await;
}

async fn test_two<Fut: Future<Output = ()>>(f: impl Fn(UnboundedReceiver<Channel<usize>>) -> Fut) {
    test_one(&f, test_sequential).await;
    test_one(&f, test_select).await;
}

#[async_std::main]
async fn main() {
    test_two(|streams| async move {
        streams
            .multicast_replay()
            .multi_item_ignore()
            .echo_buffered()
            .await
            .unwrap()
    })
    .await;
    test_two(|streams| async move {
        streams
            .multicast_buffered()
            .multi_item_ignore()
            .echo_buffered()
            .await
            .unwrap()
    })
    .await;
    test_two(|streams| async move {
        streams
            .multicast_bufferless()
            .multi_item_ignore()
            .echo_buffered()
            .await
            .unwrap()
    })
    .await;
    test_two(|streams| async move {
        streams
            .multicast_bufferless()
            .multi_item_ignore()
            .echo_buffered()
            .await
            .unwrap()
    })
    .await;
}

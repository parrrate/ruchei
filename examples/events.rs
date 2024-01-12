use std::{
    collections::HashMap,
    convert::Infallible,
    hash::Hash,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use async_std::{
    channel::{unbounded, Receiver, Sender},
    net::TcpListener,
};
use async_tungstenite::tungstenite::{self, Message};
use futures_util::{
    future::Ready,
    lock::{Mutex, OwnedMutexGuard, OwnedMutexLockFuture},
    ready,
    stream::{FusedStream, FuturesUnordered, SelectAll, SplitSink, SplitStream},
    Future, FutureExt, Sink, SinkExt, Stream, StreamExt,
};
use pin_project::pin_project;
use ruchei::{
    callback::{FutureFactory, OnClose},
    concurrent::ConcurrentExt,
    fanout_buffered::{Multicast, MulticastBuffered},
    pinned_extend::Extending,
    poll_on_wake::PollOnWakeExt,
    timeout_unused::{KeepAlive, TimeoutUnused, WithTimeout},
    with_extra::WithExtra,
};

enum Command<K, T> {
    Publish(K, T),
    Join(K),
    Leave(K),
}

enum Event<S, K, T> {
    Publish(K, T),
    Join(K, Active<S>),
}

struct Alive;

struct Joined {
    _alive: OwnedMutexGuard<Alive>,
}

struct Client<S, K> {
    stream: Arc<Mutex<S>>,
    joined: HashMap<K, Joined>,
}

impl<
        E,
        K: Unpin + Clone + Eq + Hash,
        T,
        S: Unpin + FusedStream<Item = Result<Command<K, T>, E>>,
    > Stream for Client<S, K>
{
    type Item = Event<S, K, T>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        let mut stream = this.stream.try_lock().unwrap();
        if stream.is_terminated() {
            this.joined.clear();
            return Poll::Ready(None);
        }
        Poll::Ready(loop {
            match ready!(stream.poll_next_unpin(cx)) {
                Some(Ok(Command::Publish(k, t))) => break Some(Event::Publish(k, t)),
                Some(Ok(Command::Join(k))) => {
                    if this.joined.contains_key(&k) {
                        continue;
                    }
                    let mutex = Arc::new(Mutex::new(Alive));
                    let guard = mutex.clone().try_lock_owned().unwrap();
                    let future = mutex.lock_owned();
                    this.joined.insert(k.clone(), Joined { _alive: guard });
                    break Some(Event::Join(
                        k,
                        Active {
                            stream: this.stream.clone(),
                            alive: future,
                        },
                    ));
                }
                Some(Ok(Command::Leave(k))) => {
                    this.joined.remove(&k);
                }
                Some(Err(_)) | None => {
                    this.joined.clear();
                    break None;
                }
            }
        })
    }
}

struct Active<S> {
    stream: Arc<Mutex<S>>,
    alive: OwnedMutexLockFuture<Alive>,
}

struct Error;

impl<S> Stream for Active<S> {
    type Item = Result<Infallible, Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        match this.alive.poll_unpin(cx) {
            Poll::Ready(_) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<Item, S: Unpin + FusedStream + Sink<Item>> Sink<Item> for Active<S> {
    type Error = Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        if this.alive.poll_unpin(cx).is_ready() {
            return Poll::Ready(Err(Error));
        }
        let mut stream = this.stream.try_lock().unwrap();
        if stream.is_terminated() {
            return Poll::Ready(Err(Error));
        }
        stream.poll_ready_unpin(cx).map_err(|_| Error)
    }

    fn start_send(self: Pin<&mut Self>, item: Item) -> Result<(), Self::Error> {
        let this = self.get_mut();
        let mut stream = this.stream.try_lock().unwrap();
        if stream.is_terminated() {
            return Err(Error);
        }
        stream.start_send_unpin(item).map_err(|_| Error)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        if this.alive.poll_unpin(cx).is_ready() {
            return Poll::Ready(Err(Error));
        }
        let mut stream = this.stream.try_lock().unwrap();
        if stream.is_terminated() {
            return Poll::Ready(Err(Error));
        }
        stream.poll_flush_unpin(cx).map_err(|_| Error)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        if this.alive.poll_unpin(cx).is_ready() {
            return Poll::Ready(Ok(()));
        }
        let mut stream = this.stream.try_lock().unwrap();
        if stream.is_terminated() {
            return Poll::Ready(Ok(()));
        }
        stream.poll_close_unpin(cx).map_err(|_| Error)
    }
}

#[derive(Clone)]
struct Ignore;

impl<E> OnClose<E> for Ignore {
    fn on_close(&self, _: Option<E>) {}
}

struct ReadyFactory;

impl FutureFactory for ReadyFactory {
    type Fut = Ready<()>;

    fn make(&mut self) -> Self::Fut {
        futures_util::future::ready(())
    }
}

type ActiveReceiver<S> = WithTimeout<Receiver<Active<S>>, Ready<()>, ReadyFactory>;

type ActiveMulticast<S, K, T> =
    Extending<Multicast<WithExtra<Active<S>, KeepAlive>, (K, T), Ignore>, ActiveReceiver<S>>;

#[pin_project]
struct Finalize<S, K, T>(#[pin] SplitStream<ActiveMulticast<S, K, T>>, Option<K>);

impl<
        E,
        K: Unpin + Clone + Eq + Hash,
        T: Clone,
        S: Unpin + FusedStream<Item = Result<Command<K, T>, E>> + Sink<(K, T), Error = E>,
    > Future for Finalize<S, K, T>
{
    type Output = K;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        match this.0.poll_next(cx) {
            Poll::Ready(Some(Ok(i) | Err(i))) => match i {},
            Poll::Ready(None) => Poll::Ready(this.1.take().unwrap()),
            Poll::Pending => Poll::Pending,
        }
    }
}

type ActiveEntry<S, K, T> = (
    SplitSink<ActiveMulticast<S, K, T>, (K, T)>,
    Sender<Active<S>>,
);

#[pin_project]
struct Server<R, S, K, T> {
    #[pin]
    streams: R,
    #[pin]
    select: SelectAll<Client<S, K>>,
    #[pin]
    finalizing: FuturesUnordered<Finalize<S, K, T>>,
    active: HashMap<K, ActiveEntry<S, K, T>>,
}

impl<
        E,
        K: Unpin + Clone + Eq + Hash,
        T: Clone,
        S: Unpin + FusedStream<Item = Result<Command<K, T>, E>> + Sink<(K, T), Error = E>,
        R: FusedStream<Item = S>,
    > Future for Server<R, S, K, T>
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        if !this.streams.is_terminated() {
            while let Poll::Ready(Some(stream)) = this.streams.as_mut().poll_next(cx) {
                this.select.push(Client {
                    stream: Arc::new(Mutex::new(stream)),
                    joined: Default::default(),
                });
            }
        }
        loop {
            while let Poll::Ready(Some(k)) = this.finalizing.as_mut().poll_next(cx) {
                this.active.remove(&k);
            }
            let Some(event) = ready!(this.select.as_mut().poll_next(cx)) else {
                break if this.streams.is_terminated() {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                };
            };
            match event {
                Event::Publish(k, t) => {
                    if let Some((sink, _)) = this.active.get_mut(&k) {
                        sink.start_send_unpin((k, t)).unwrap();
                        match sink.poll_ready_unpin(cx) {
                            Poll::Ready(r) => r.unwrap(),
                            Poll::Pending => panic!("oops"),
                        }
                    }
                }
                Event::Join(k, active) => {
                    Pin::new(this.active.entry(k).or_insert_with_key(|k| {
                        let (sender, receiver) = unbounded();
                        let (sink, stream) = receiver
                            .timeout_unused(ReadyFactory)
                            .multicast_buffered(Ignore)
                            .split();
                        this.finalizing.push(Finalize(stream, Some(k.clone())));
                        (sink, sender)
                    }))
                    .1
                    .try_send(active)
                    .unwrap();
                }
            }
        }
    }
}

trait ServeEvents: Sized {
    type S;

    type K;

    type T;

    fn serve_events(self) -> Server<Self, Self::S, Self::K, Self::T>;
}

impl<
        E,
        K: Unpin + Clone + Eq + Hash,
        T,
        S: Unpin + FusedStream<Item = Result<Command<K, T>, E>>,
        R: Stream<Item = S>,
    > ServeEvents for R
{
    type S = S;

    type K = K;

    type T = T;

    fn serve_events(self) -> Server<Self, Self::S, Self::K, Self::T> {
        Server {
            streams: self,
            select: Default::default(),
            finalizing: Default::default(),
            active: Default::default(),
        }
    }
}

#[pin_project]
struct EventStream<S>(#[pin] S);

enum EventError {
    WebSocket(tungstenite::Error),
    NoCommand,
    NoKey,
    UknownCommand,
}

impl From<tungstenite::Error> for EventError {
    fn from(value: tungstenite::Error) -> Self {
        Self::WebSocket(value)
    }
}

impl<S: Stream<Item = Result<Message, tungstenite::Error>>> Stream for EventStream<S> {
    type Item = Result<Command<String, String>, EventError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.project().0.poll_next(cx).map(|o| {
            o.map(|r| {
                let s = r?.into_text()?;
                let s = s.trim();
                let (command, s) = s.split_once(' ').ok_or(EventError::NoCommand)?;
                let s = s.trim();
                match command {
                    "j" => Ok(Command::Join(s.into())),
                    "l" => Ok(Command::Leave(s.into())),
                    "p" => {
                        let (key, s) = s.split_once(' ').ok_or(EventError::NoKey)?;
                        let s = s.trim();
                        Ok(Command::Publish(key.into(), s.into()))
                    }
                    _ => Err(EventError::UknownCommand),
                }
            })
        })
    }
}

impl<S: FusedStream> FusedStream for EventStream<S>
where
    Self: Stream,
{
    fn is_terminated(&self) -> bool {
        self.0.is_terminated()
    }
}

impl<S: Sink<Message, Error = tungstenite::Error>> Sink<(String, String)> for EventStream<S> {
    type Error = EventError;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().0.poll_ready(cx).map_err(Into::into)
    }

    fn start_send(self: Pin<&mut Self>, (k, t): (String, String)) -> Result<(), Self::Error> {
        self.project()
            .0
            .start_send(format!("{k} {t}").into())
            .map_err(Into::into)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().0.poll_flush(cx).map_err(Into::into)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().0.poll_close(cx).map_err(Into::into)
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
        .fuse()
        .concurrent()
        .filter_map(|r| async { r.ok() })
        .map(|s| s.poll_on_wake())
        .map(EventStream)
        .serve_events()
        .await;
}

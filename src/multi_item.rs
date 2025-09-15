use std::{
    convert::Infallible,
    pin::Pin,
    task::{Context, Poll},
};

use futures_util::{Sink, Stream, StreamExt, future::ready, ready, stream::FusedStream};
use pin_project::pin_project;
use ruchei_route::RouteSink;

#[derive(Debug)]
pub enum MultiItem<T, S, E> {
    Item(T),
    Closed(S, Option<E>),
}

#[pin_project]
pub struct MultiItemIgnoreRoute<R>(#[pin] R);

impl<T, S, E, R: Stream<Item = MultiItem<T, S, E>>> Stream for MultiItemIgnoreRoute<R> {
    type Item = Result<T, Infallible>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        while let Some(item) = ready!(this.0.as_mut().poll_next(cx)) {
            match item {
                MultiItem::Item(item) => return Poll::Ready(Some(Ok(item))),
                MultiItem::Closed(_, _) => {}
            }
        }
        Poll::Ready(None)
    }
}

impl<T, S, E, R: FusedStream<Item = MultiItem<T, S, E>>> FusedStream for MultiItemIgnoreRoute<R> {
    fn is_terminated(&self) -> bool {
        self.0.is_terminated()
    }
}

impl<Route, Msg, R: RouteSink<Route, Msg>> RouteSink<Route, Msg> for MultiItemIgnoreRoute<R> {
    type Error = R::Error;

    fn poll_ready(
        self: Pin<&mut Self>,
        route: &Route,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.project().0.poll_ready(route, cx)
    }

    fn start_send(self: Pin<&mut Self>, route: Route, msg: Msg) -> Result<(), Self::Error> {
        self.project().0.start_send(route, msg)
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        route: &Route,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.project().0.poll_flush(route, cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.project().0.poll_close(cx)
    }
}

pub trait MultiItemExt: Sized + FusedStream<Item = MultiItem<Self::T, Self::S, Self::E>> {
    type T;
    type S;
    type E;

    fn multi_item_ignore<I>(
        self,
    ) -> impl FusedStream<Item = Result<Self::T, Infallible>> + Sink<I, Error = Infallible>
    where
        Self: Sink<I, Error = Infallible>,
    {
        self.filter_map(|item| {
            ready(match item {
                MultiItem::Item(item) => Some(Ok(item)),
                MultiItem::Closed(_, _) => None,
            })
        })
    }

    fn multi_item_ignore_route<Route, Msg>(
        self,
    ) -> impl FusedStream<Item = Result<Self::T, Infallible>> + RouteSink<Route, Msg, Error = Infallible>
    where
        Self: RouteSink<Route, Msg, Error = Infallible>,
    {
        MultiItemIgnoreRoute(self)
    }
}

impl<T, S, E, R: FusedStream<Item = MultiItem<T, S, E>>> MultiItemExt for R {
    type T = T;
    type S = S;
    type E = E;
}

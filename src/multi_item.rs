use std::convert::Infallible;

use futures_util::{
    StreamExt, TryStream,
    future::{Ready, ready},
    stream::{FilterMap, FusedStream},
};

#[derive(Debug)]
pub enum MultiItem<S, T = <S as TryStream>::Ok, E = <S as TryStream>::Error> {
    Item(T),
    Closed(S, Option<E>),
}

pub type MultiRouteItem<K, S, T = <S as TryStream>::Ok, E = <S as TryStream>::Error> =
    MultiItem<(K, S), (K, T), E>;

type Fut<R> = Ready<Option<Result<<R as MultiItemExt>::T, Infallible>>>;

type Mi<R> = MultiItem<<R as MultiItemExt>::S, <R as MultiItemExt>::T, <R as MultiItemExt>::E>;

pub trait MultiItemExt: Sized + FusedStream<Item = MultiItem<Self::S, Self::T, Self::E>> {
    type T;
    type S;
    type E;

    fn multi_item_ignore(self) -> FilterMap<Self, Fut<Self>, impl FnMut(Mi<Self>) -> Fut<Self>> {
        self.filter_map(|item| {
            ready(match item {
                MultiItem::Item(item) => Some(Ok(item)),
                MultiItem::Closed(_, _) => None,
            })
        })
    }
}

impl<T, S, E, R: FusedStream<Item = MultiItem<S, T, E>>> MultiItemExt for R {
    type T = T;
    type S = S;
    type E = E;
}

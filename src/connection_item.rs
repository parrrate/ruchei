use std::convert::Infallible;

use futures_util::{
    Stream, StreamExt, TryStream,
    future::{Ready, ready},
    stream::FilterMap,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[must_use]
pub enum ConnectionItem<S, T = <S as TryStream>::Ok, E = <S as TryStream>::Error> {
    Item(T),
    Closed(S, Option<E>),
}

pub type MultiRouteItem<K, S, T = <S as TryStream>::Ok, E = <S as TryStream>::Error> =
    ConnectionItem<(K, S), (K, T), E>;

type Fut<R> = Ready<Option<Result<<R as ConnectionItemExt>::T, Infallible>>>;

type Mi<R> = ConnectionItem<
    <R as ConnectionItemExt>::S,
    <R as ConnectionItemExt>::T,
    <R as ConnectionItemExt>::E,
>;

pub trait ConnectionItemExt:
    Sized + Stream<Item = ConnectionItem<Self::S, Self::T, Self::E>>
{
    type T;
    type S;
    type E;

    fn connection_item_ignore(
        self,
    ) -> FilterMap<Self, Fut<Self>, impl FnMut(Mi<Self>) -> Fut<Self>> {
        self.filter_map(|item| {
            ready(match item {
                ConnectionItem::Item(item) => Some(Ok(item)),
                ConnectionItem::Closed(_, _) => None,
            })
        })
    }
}

impl<T, S, E, R: Stream<Item = ConnectionItem<S, T, E>>> ConnectionItemExt for R {
    type T = T;
    type S = S;
    type E = E;
}

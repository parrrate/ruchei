use std::convert::Infallible;

use futures_util::{
    StreamExt,
    future::{Ready, ready},
    stream::{FilterMap, FusedStream},
};

#[derive(Debug)]
pub enum MultiItem<T, S, E> {
    Item(T),
    Closed(S, Option<E>),
}

type Fut<R> = Ready<Option<Result<<R as MultiItemExt>::T, Infallible>>>;

type Mi<R> = MultiItem<<R as MultiItemExt>::T, <R as MultiItemExt>::S, <R as MultiItemExt>::E>;

pub trait MultiItemExt: Sized + FusedStream<Item = MultiItem<Self::T, Self::S, Self::E>> {
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

impl<T, S, E, R: FusedStream<Item = MultiItem<T, S, E>>> MultiItemExt for R {
    type T = T;
    type S = S;
    type E = E;
}

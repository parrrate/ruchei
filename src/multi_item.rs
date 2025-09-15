use std::convert::Infallible;

use futures_util::{Sink, StreamExt, future::ready, stream::FusedStream};

#[derive(Debug)]
pub enum MultiItem<T, S, E> {
    Item(T),
    Closed(S, Option<E>),
}

pub trait MultiItemExt<I>:
    Sized + FusedStream<Item = MultiItem<Self::T, Self::S, Self::E>> + Sink<I, Error = Infallible>
{
    type T;
    type S;
    type E;
    fn multi_item_ignore(
        self,
    ) -> impl FusedStream<Item = Result<Self::T, Infallible>> + Sink<I, Error = Infallible> {
        self.filter_map(|item| {
            ready(match item {
                MultiItem::Item(item) => Some(Ok(item)),
                MultiItem::Closed(_, _) => None,
            })
        })
    }
}

impl<T, S, E, I, R: FusedStream<Item = MultiItem<T, S, E>> + Sink<I, Error = Infallible>>
    MultiItemExt<I> for R
{
    type T = T;
    type S = S;
    type E = E;
}

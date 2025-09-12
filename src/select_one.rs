use futures_util::{stream::SelectAll, Stream};

pub trait SelectOne: Sized {
    fn select_one(self) -> SelectAll<Self>;
}

impl<S: Unpin + Stream> SelectOne for S {
    fn select_one(self) -> SelectAll<Self> {
        let mut select = SelectAll::new();
        select.push(self);
        select
    }
}

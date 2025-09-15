pub enum MultiItem<Item, S, E> {
    Item(Item),
    Closed(S, Option<E>),
}

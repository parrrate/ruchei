use std::borrow::Borrow;

pub trait MultiTrie<Collection: ?Sized> {
    fn mt_remove(&mut self, collection: &Collection, key: &[u8]);
    #[must_use]
    fn mt_contains(&self, collection: &Collection, key: &[u8]) -> bool;
    fn mt_clear(&mut self, collection: &Collection);
    #[must_use]
    fn mt_is_empty(&self, collection: &Collection) -> bool;
    #[must_use]
    fn mt_len(&self, collection: &Collection) -> usize;
}

pub trait MultiTrieAddOwned<Collection>: MultiTrie<Collection> {
    fn mt_add_owned(&mut self, collection: Collection, key: &[u8]);
}

pub trait MultiTrieAddRef<Collection: ?Sized>: MultiTrie<Collection> {
    fn mt_add_ref(&mut self, collection: &Collection, key: &[u8]);
}

pub trait MultiTriePrefix: MultiTrie<Self::Collection> {
    type Collection: ?Sized;
    fn mt_prefix_collect<'a>(
        &'a self,
        suffix: &[u8],
    ) -> impl 'a + IntoIterator<Item: 'a + Borrow<Self::Collection>>;
}

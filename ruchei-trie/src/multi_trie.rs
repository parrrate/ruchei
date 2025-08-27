pub trait MultiTrie<Collection: ?Sized> {
    fn remove(&mut self, collection: &Collection, key: &[u8]);
    fn contains(&self, collection: &Collection, key: &[u8]) -> bool;
    fn clear(&mut self, collection: &Collection);
    fn is_empty(&self, collection: &Collection) -> bool;
    fn len(&self, collection: &Collection) -> usize;
}

pub trait MultiTrieAddOwned<Collection>: MultiTrie<Collection> {
    fn add_owned(&mut self, collection: Collection, key: &[u8]);
}

pub trait MultiTrieAddRef<Collection: ?Sized>: MultiTrie<Collection> {
    fn add_ref(&mut self, collection: &Collection, key: &[u8]);
}

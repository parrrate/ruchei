pub trait MultiTrie {
    type Collection: ?Sized;
    fn remove(&mut self, collection: &Self::Collection, key: &[u8]);
    fn contains(&self, collection: &Self::Collection, key: &[u8]) -> bool;
    fn clear(&mut self, collection: &Self::Collection);
    fn is_empty(&self, collection: &Self::Collection) -> bool;
    fn len(&self, collection: &Self::Collection) -> usize;
}

pub trait MultiTrieAddOwned: MultiTrie {
    fn add_owned(&mut self, collection: Self::Collection, key: &[u8]);
}

pub trait MultiTrieAddRef: MultiTrie {
    fn add_ref(&mut self, collection: &Self::Collection, key: &[u8]);
}

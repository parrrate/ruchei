use futures_channel::mpsc::UnboundedSender;

pub(crate) struct KeyDropCopy<K: Copy> {
    key: K,
    key_s: UnboundedSender<K>,
}

impl<K: Copy> Drop for KeyDropCopy<K> {
    fn drop(&mut self) {
        let _ = self.key_s.unbounded_send(self.key);
    }
}

impl<K: Copy> KeyDropCopy<K> {
    pub(crate) fn new(key: K, key_s: UnboundedSender<K>) -> Self {
        Self { key, key_s }
    }

    pub(crate) fn key(&self) -> K {
        self.key
    }
}

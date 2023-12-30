pub trait OnClose<E>: Clone {
    fn on_close(&self, error: Option<E>);
}

impl<E, F: Clone + Fn(Option<E>)> OnClose<E> for F {
    fn on_close(&self, error: Option<E>) {
        self(error)
    }
}

pub trait OnItem<T> {
    fn on_item(&self, message: T);
}

impl<T, F: Fn(T)> OnItem<T> for F {
    fn on_item(&self, message: T) {
        self(message)
    }
}

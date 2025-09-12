pub trait Callback<E>: Clone {
    fn on_close(&self, error: Option<E>);
}

impl<E, F: Clone + Fn(Option<E>)> Callback<E> for F {
    fn on_close(&self, error: Option<E>) {
        self(error)
    }
}

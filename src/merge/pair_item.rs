use futures_util::Stream;

pub trait PairCategory {
    type Pair<K, V>: PairItem<Category = Self, K = K, V = V>;
}

pub type Pair<C, K, V> = <C as PairCategory>::Pair<K, V>;

pub type PairResult<P, V> = Result<
    (<P as PairItem>::K, <P as PairItem>::V),
    Pair<<P as PairItem>::Category, <P as PairItem>::K, V>,
>;

pub trait PairItem: Sized {
    type Category: PairCategory<Pair<Self::K, Self::V> = Self>;
    type K;
    type V;

    fn into_kv<V0>(self) -> PairResult<Self, V0>;
    fn from_kv(k: Self::K, v: Self::V) -> Self;
}

impl PairCategory for () {
    type Pair<K, V> = (K, V);
}

impl<K, V> PairItem for (K, V) {
    type Category = ();
    type K = K;
    type V = V;

    fn into_kv<V0>(self) -> PairResult<Self, V0> {
        Ok(self)
    }

    fn from_kv(k: Self::K, v: Self::V) -> Self {
        (k, v)
    }
}

impl<E> PairCategory for Result<(), E> {
    type Pair<K, V> = Result<(K, V), E>;
}

impl<K, V, E> PairItem for Result<(K, V), E> {
    type Category = Result<(), E>;
    type K = K;
    type V = V;

    fn into_kv<V0>(self) -> PairResult<Self, V0> {
        self.map_err(Err)
    }

    fn from_kv(k: Self::K, v: Self::V) -> Self {
        Ok((k, v))
    }
}

pub trait PairStream: Stream<Item: PairItem<K = Self::K, V = Self::V>> {
    type K;
    type V;
}

impl<K, V, S: Stream<Item: PairItem<K = K, V = V>>> PairStream for S {
    type K = K;
    type V = V;
}

pub trait StreamPair {
    type K;
    type L: PairStream<K = Self::K>;
    type R: PairStream<K = Self::K>;
}

impl<K, L: PairStream<K = K>, R: PairStream<K = K>> StreamPair for (L, R) {
    type K = K;
    type L = L;
    type R = R;
}

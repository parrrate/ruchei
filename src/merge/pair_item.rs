use futures_util::Stream;

pub trait PairCategory {
    type Pair<K, V>: PairItem<C = Self, K = K, V = V>;
}

pub type Pair<C, K, V> = <C as PairCategory>::Pair<K, V>;

pub type PairResult<P, V> = Result<
    (<P as PairItem>::K, <P as PairItem>::V),
    Pair<<P as PairItem>::C, <P as PairItem>::K, V>,
>;

pub trait PairItem: Sized {
    type C: PairCategory<Pair<Self::K, Self::V> = Self>;
    type K;
    type V;

    fn into_kv<V0>(self) -> PairResult<Self, V0>;
    #[must_use]
    fn from_kv(k: Self::K, v: Self::V) -> Self;
}

impl PairCategory for () {
    type Pair<K, V> = (K, V);
}

impl<K, V> PairItem for (K, V) {
    type C = ();
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
    type C = Result<(), E>;
    type K = K;
    type V = V;

    fn into_kv<V0>(self) -> PairResult<Self, V0> {
        self.map_err(Err)
    }

    fn from_kv(k: Self::K, v: Self::V) -> Self {
        Ok((k, v))
    }
}

pub trait PairStream: Stream<Item: PairItem<C = Self::C, K = Self::K, V = Self::V>> {
    type C: PairCategory<Pair<Self::K, Self::V> = Self::Item>;
    type K;
    type V;
}

impl<
    C: PairCategory<Pair<K, V> = Item>,
    K,
    V,
    Item: PairItem<C = C, K = K, V = V>,
    S: Stream<Item = Item>,
> PairStream for S
{
    type C = C;
    type K = K;
    type V = V;
}

pub trait StreamPair {
    type C;
    type K;
    type L: PairStream<C = Self::C, K = Self::K>;
    type R: PairStream<C = Self::C, K = Self::K>;
}

impl<C, K, L: PairStream<C = C, K = K>, R: PairStream<C = C, K = K>> StreamPair for (L, R) {
    type C = C;
    type K = K;
    type L = L;
    type R = R;
}

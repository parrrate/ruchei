//! [`Sink`]s with round-robin distribution inspired by ZeroMQ's `DEALER` sockets.
//!
//! [`Sink`]: futures_util::Sink

pub mod keyed;
pub mod slab;

#[deprecated]
pub type Dealer<K, S, F> = keyed::Dealer<K, S, F>;

#[deprecated]
pub type DealerExtending<F, R> = keyed::DealerExtending<F, R>;

#[deprecated]
pub trait DealerExt:
    keyed::DealerKeyedExt<
    K = <Self as DealerExt>::K,
    S = <Self as DealerExt>::S,
    E = <Self as DealerExt>::E,
>
{
    type K;
    type S;
    type E;

    #[must_use]
    #[deprecated]
    fn deal<F: crate::callback::OnClose<<Self as keyed::DealerKeyedExt>::E>>(
        self,
        callback: F,
    ) -> keyed::DealerExtending<F, Self>;
}

#[allow(deprecated)]
impl<R: keyed::DealerKeyedExt> DealerExt for R {
    type K = <R as keyed::DealerKeyedExt>::K;
    type S = <R as keyed::DealerKeyedExt>::S;
    type E = <R as keyed::DealerKeyedExt>::E;

    #[must_use]
    fn deal<F: crate::callback::OnClose<<Self as keyed::DealerKeyedExt>::E>>(
        self,
        callback: F,
    ) -> keyed::DealerExtending<F, Self> {
        keyed::DealerKeyedExt::deal_keyed(self, callback)
    }
}

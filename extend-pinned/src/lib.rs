#![no_std]

use core::pin::Pin;

/// [`Extend`] equivalent for [`Pin<&mut I>`].
pub trait ExtendPinned<A> {
    /// [`Extend::extend`] equivalent for [`Pin<&mut I>`].
    fn extend_pinned<T: IntoIterator<Item = A>>(self: Pin<&mut Self>, iter: T);
}

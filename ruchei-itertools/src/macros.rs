macro_rules! forward {
    ($i:ident, $($t:ident),*) => {
        impl<S, $($t),*> $i<S, $($t),*> {
            pub fn get_ref(&self) -> &S {
                &self.inner
            }

            pub fn get_mut(&mut self) -> &mut S {
                &mut self.inner
            }

            pub fn get_pin_mut(self: Pin<&mut Self>) -> Pin<&mut S> {
                self.project().inner
            }

            pub fn into_inner(self) -> S {
                self.inner
            }
        }

        #[cfg(feature = "sink")]
        const _: () = {
            use futures_sink::Sink;
            impl<S: Sink<Item>, Item, $($t),*> Sink<Item> for $i<S, $($t),*> {
                type Error = S::Error;

                fn poll_ready(
                    self: Pin<&mut Self>,
                    cx: &mut Context<'_>,
                ) -> Poll<Result<(), Self::Error>> {
                    self.get_pin_mut().poll_ready(cx)
                }

                fn start_send(self: Pin<&mut Self>, item: Item) -> Result<(), Self::Error> {
                    self.get_pin_mut().start_send(item)
                }

                fn poll_flush(
                    self: Pin<&mut Self>,
                    cx: &mut Context<'_>,
                ) -> Poll<Result<(), Self::Error>> {
                    self.get_pin_mut().poll_flush(cx)
                }

                fn poll_close(
                    self: Pin<&mut Self>,
                    cx: &mut Context<'_>,
                ) -> Poll<Result<(), Self::Error>> {
                    self.get_pin_mut().poll_close(cx)
                }
            }
        };

        #[cfg(feature = "route-sink")]
        const _: () = {
            use route_sink::{FlushRoute, ReadyRoute, ReadySome};
            impl<S: FlushRoute<Route, Msg>, Route, Msg, $($t),*> FlushRoute<Route, Msg> for $i<S, $($t),*> {
                fn poll_flush_route(
                    self: Pin<&mut Self>,
                    route: &Route,
                    cx: &mut Context<'_>,
                ) -> Poll<Result<(), Self::Error>> {
                    self.get_pin_mut().poll_flush_route(route, cx)
                }

                fn poll_close_route(
                    self: Pin<&mut Self>,
                    route: &Route,
                    cx: &mut Context<'_>,
                ) -> Poll<Result<(), Self::Error>> {
                    self.get_pin_mut().poll_close_route(route, cx)
                }
            }

            impl<S: ReadyRoute<Route, Msg>, Route, Msg, $($t),*> ReadyRoute<Route, Msg> for $i<S, $($t),*> {
                fn poll_ready_route(
                    self: Pin<&mut Self>,
                    route: &Route,
                    cx: &mut Context<'_>,
                ) -> Poll<Result<(), Self::Error>> {
                    self.get_pin_mut().poll_ready_route(route, cx)
                }
            }

            impl<S: ReadySome<Route, Msg>, Route, Msg, $($t),*> ReadySome<Route, Msg> for $i<S, $($t),*> {
                fn poll_ready_some(
                    self: Pin<&mut Self>,
                    cx: &mut Context<'_>,
                ) -> Poll<Result<Route, Self::Error>> {
                    self.get_pin_mut().poll_ready_some(cx)
                }
            }
        };
    };
}

pub(crate) use forward;

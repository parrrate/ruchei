//! Ad hoc utility for debugging `ruchei`. Not thread safe

use std::{
    pin::Pin,
    sync::atomic::{AtomicBool, Ordering},
    task::{Context, Poll},
    thread,
    time::Duration,
};

use futures_util::{stream::FusedStream, Sink, Stream};
use pin_project::pin_project;

static SAMPLE: AtomicBool = AtomicBool::new(false);

#[doc(hidden)]
pub struct Reset(bool);

impl Drop for Reset {
    fn drop(&mut self) {
        set(self.0);
    }
}

/// Create an RAII guard for enabling sampling, resetting to previous value on drop
pub fn enable() -> Reset {
    let value = sample();
    set(true);
    Reset(value)
}

/// Create an RAII guard for disabling sampling, resetting to previous value on drop
pub fn disable() -> Reset {
    let value = sample();
    set(false);
    Reset(value)
}

fn set(sample: bool) {
    SAMPLE.store(sample, Ordering::Release)
}

fn sample() -> bool {
    SAMPLE.load(Ordering::Acquire)
}

fn run() {
    let mut counts = Vec::new();
    loop {
        for _ in 0..10 {
            let mut count = 0;
            for _ in 0..1000 {
                if sample() {
                    count += 1;
                }
                thread::sleep(Duration::from_micros(100));
            }
            counts.push(count);
        }
        eprint!("sample:");
        for count in &counts {
            let r = 255;
            let g = 255 - (255 * count) / 1000;
            let b = (255 - (255 * count) / 500).max(0);
            eprint!(" \x1b[40;38;2;{r};{g};{b}m{count:4}\x1b[0m");
        }
        eprintln!();
        counts.clear();
    }
}

#[doc(hidden)]
pub fn start() {
    thread::spawn(run);
}

#[doc(hidden)]
#[pin_project]
pub struct Exclude<S>(#[pin] pub S);

impl<S: Stream> Stream for Exclude<S> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let _guard = crate::disable();
        self.project().0.poll_next(cx)
    }
}

impl<S: FusedStream> FusedStream for Exclude<S> {
    fn is_terminated(&self) -> bool {
        self.0.is_terminated()
    }
}

impl<Item, S: Sink<Item>> Sink<Item> for Exclude<S> {
    type Error = S::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let _guard = crate::disable();
        self.project().0.poll_ready(cx)
    }

    fn start_send(self: Pin<&mut Self>, item: Item) -> Result<(), Self::Error> {
        let _guard = crate::disable();
        self.project().0.start_send(item)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let _guard = crate::disable();
        self.project().0.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let _guard = crate::disable();
        self.project().0.poll_close(cx)
    }
}

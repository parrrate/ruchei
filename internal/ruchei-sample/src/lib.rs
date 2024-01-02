//! Utility for debugging `ruchei`

use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};

static SAMPLE: AtomicBool = AtomicBool::new(false);

pub struct Enable;

impl Drop for Enable {
    fn drop(&mut self) {
        set(false);
    }
}

pub fn enable() -> Enable {
    set(true);
    Enable
}

pub struct Disable;

impl Drop for Disable {
    fn drop(&mut self) {
        set(true);
    }
}

pub fn disable() -> Disable {
    set(false);
    Disable
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

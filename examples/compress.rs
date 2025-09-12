//! [`ruchei::compress`]

use std::time::Duration;

use async_std::stream::StreamExt;
use ruchei::compress::{CompressExt, Credit};

#[async_std::main]
async fn main() {
    let inner = async_io::Timer::interval(Duration::from_secs(1));
    let credits = async_io::Timer::interval(Duration::from_secs_f64(1.0 + 5.0f64.sqrt()));
    inner
        .map(|_| vec![0])
        .compress(credits.map(|_| Credit))
        .for_each(|vec| println!("{vec:?}"))
        .await;
}

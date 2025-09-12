//! `async-std`?????????
//!
//! So this is apparently broken in `--release`

#[async_std::main]
async fn main() -> async_tungstenite::tungstenite::Result<()> {
    let listener = async_std::net::TcpListener::bind("127.0.0.1:8080").await?;
    let (stream, _) = listener.accept().await?;
    async_tungstenite::accept_async(stream).await?;
    Ok(())
}

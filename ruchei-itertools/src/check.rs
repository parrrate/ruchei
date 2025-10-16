use futures_util::Stream;

pub fn assert_stream<S: Stream>(stream: S) -> S {
    stream
}

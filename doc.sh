set -xe
cargo doc --no-deps --workspace
mkdir -p ./target/doc_copy
cp -rT ./target/doc ./target/doc_copy
echo ok

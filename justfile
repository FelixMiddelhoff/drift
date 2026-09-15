check:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets --all-features -- -D warnings
    cargo test --workspace --all-features

fmt:
    cargo fmt --all

test:
    cargo test --workspace --all-features

default:
    @just --list

build:
    cargo build --release --locked

test:
    cargo test --locked

lint:
    cargo clippy --locked -- -D warnings

clean:
    cargo clean

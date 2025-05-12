run *args:
    cargo run --features cli -- {{args}}

test:
    cargo test

install:
    cargo install --path . --features cli
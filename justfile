run *args:
    cargo run --features=cli -- {{args}}

test:
    cargo test --features=cli

install:
    cargo install --path . --features=cli
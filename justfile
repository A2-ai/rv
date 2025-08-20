run *args:
    cargo run --features=cli -- {{args}}

build: 
    cargo build --features=cli

test:
    cargo test --features=cli

install:
    cargo install --path . --features=cli
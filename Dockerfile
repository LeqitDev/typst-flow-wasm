FROM rust:latest

COPY ./ ./


RUN apt-get update && apt-get install -y \
    openssl
RUN rustup target add wasm32-unknown-unknown
RUN cargo install wasm-pack
RUN wasm-pack build --target web
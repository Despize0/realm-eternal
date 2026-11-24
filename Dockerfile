FROM rust:1-bookworm AS build
WORKDIR /app
COPY . .
RUN rustup target add wasm32-unknown-unknown
RUN cargo install wasm-bindgen-cli --version 0.2.130 --locked
RUN cargo build --release -p realm-client --target wasm32-unknown-unknown
RUN wasm-bindgen --target web --no-typescript --out-dir web/pkg \
    target/wasm32-unknown-unknown/release/realm_client.wasm
RUN cargo build --release -p realm-server

FROM debian:bookworm-slim
RUN useradd -r -u 10001 realm
WORKDIR /app
COPY --from=build /app/target/release/realm-server /app/server
COPY --from=build /app/web /app/web
ENV STATIC_DIR=/app/web
ENV PORT=8080
USER realm
EXPOSE 8080
CMD ["/app/server"]

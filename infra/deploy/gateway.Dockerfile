FROM rust:1.85-slim AS builder
WORKDIR /app
COPY gateway/ gateway/
COPY infra/gen/rust/ infra/gen/rust/
RUN cd gateway && cargo build --release

FROM debian:bookworm-slim
COPY --from=builder /app/gateway/target/release/gateway /usr/local/bin/
CMD ["gateway"]

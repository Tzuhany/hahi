FROM rust:1.85-slim AS builder
WORKDIR /app
COPY apps/gateway/ apps/gateway/
COPY contracts/gen/rust/ contracts/gen/rust/
RUN cd apps/gateway && cargo build --release

FROM debian:bookworm-slim
COPY --from=builder /app/apps/gateway/target/release/gateway /usr/local/bin/
CMD ["gateway"]

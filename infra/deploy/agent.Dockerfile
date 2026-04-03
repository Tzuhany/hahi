FROM rust:1.85-slim AS builder
WORKDIR /app
COPY agent/ agent/
COPY infra/gen/rust/ infra/gen/rust/
RUN cd agent && cargo build --release

FROM debian:bookworm-slim
COPY --from=builder /app/agent/target/release/agent /usr/local/bin/
CMD ["agent"]

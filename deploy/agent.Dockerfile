FROM rust:1.85-slim AS builder
WORKDIR /app
COPY apps/agent/ apps/agent/
COPY contracts/gen/rust/ contracts/gen/rust/
RUN cd apps/agent && cargo build --release

FROM debian:bookworm-slim
COPY --from=builder /app/apps/agent/target/release/agent /usr/local/bin/
CMD ["agent"]

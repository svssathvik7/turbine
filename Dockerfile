FROM rust:1.86-slim AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Cache dependencies by building them first
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && echo "" > src/lib.rs
RUN cargo build --release && rm -rf src

# Build the actual binary
COPY src/ src/
RUN touch src/main.rs src/lib.rs && cargo build --release

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/turbine /usr/local/bin/turbine

EXPOSE 8080

ENTRYPOINT ["turbine"]
CMD ["--config", "/etc/turbine/config.toml"]

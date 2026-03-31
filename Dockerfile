FROM rust:1.87 AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY template.html ./
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/panopticon /usr/local/bin/panopticon
ENV PORT=3000
ENV DATA_DIR=/data
EXPOSE 3000
CMD ["panopticon"]

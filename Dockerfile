FROM rust:1.83-slim AS builder

RUN apt-get update && apt-get install -y musl-tools && rm -rf /var/lib/apt/lists/*
RUN rustup target add x86_64-unknown-linux-musl

WORKDIR /build
COPY Cargo.toml Cargo.lock* ./
COPY src/ src/

RUN cargo build --release --target x86_64-unknown-linux-musl
RUN strip /build/target/x86_64-unknown-linux-musl/release/miniminio

FROM scratch

COPY --from=builder /build/target/x86_64-unknown-linux-musl/release/miniminio /miniminio

EXPOSE 9000
VOLUME ["/data"]

ENTRYPOINT ["/miniminio"]
CMD ["--data-dir", "/data"]

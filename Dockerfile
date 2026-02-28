FROM rust:1.85-slim AS builder

ARG TARGETARCH
RUN apt-get update && apt-get install -y musl-tools && rm -rf /var/lib/apt/lists/*

RUN case "${TARGETARCH}" in \
      amd64) MUSL_TARGET=x86_64-unknown-linux-musl ;; \
      arm64) MUSL_TARGET=aarch64-unknown-linux-musl ;; \
      *)     MUSL_TARGET=x86_64-unknown-linux-musl ;; \
    esac && \
    rustup target add ${MUSL_TARGET} && \
    echo "${MUSL_TARGET}" > /tmp/musl-target

WORKDIR /build
COPY Cargo.toml Cargo.lock* ./
COPY src/ src/

RUN MUSL_TARGET=$(cat /tmp/musl-target) && \
    cargo build --release --target ${MUSL_TARGET} && \
    cp /build/target/${MUSL_TARGET}/release/miniminio /build/miniminio && \
    strip /build/miniminio

FROM scratch

COPY --from=builder /build/miniminio /miniminio

EXPOSE 9000
VOLUME ["/data"]

ENTRYPOINT ["/miniminio"]
CMD ["--data-dir", "/data"]

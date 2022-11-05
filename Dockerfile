FROM rust:1.65.0-alpine3.16 AS builder

FROM builder AS oci-srm-server-mock-binary

COPY --link Cargo.toml \
    Cargo.lock \
    /build/
COPY --link src \
    /build/src

RUN cd /build && \
    cargo install --path .

EXPOSE 80
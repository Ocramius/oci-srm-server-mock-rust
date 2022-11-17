FROM rust:1.65.0-slim-bullseye AS builder

FROM builder AS oci-srm-server-mock-binary

COPY --link Cargo.toml \
    Cargo.lock \
    /build/
COPY --link src \
    /build/src

RUN cd /build && \
    RUSTFLAGS='-C target-feature=+crt-static' \
      cargo build \
        --release \
        --target x86_64-unknown-linux-gnu \
    && \
    cp /build/target/x86_64-unknown-linux-gnu/release/oci-srm-server-mock /oci-srm-server-mock && \
    cargo clean && \
    rm -rf /usr/local/cargo/registry/{cache,index,src}


FROM scratch AS oci-srm-server-mock

COPY --link --from=oci-srm-server-mock-binary /oci-srm-server-mock /oci-srm-server-mock

EXPOSE 80

CMD ["/oci-srm-server-mock"]
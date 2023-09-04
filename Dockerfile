# @TODO this file is to be removed: it only exists for reference, until we can get static compilation to work in Nix too
FROM rust:1.72.0-slim-bullseye AS builder

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

ENV OCI_SRM_SERVER_MOCK_PORT="80" \
    OCI_SRM_SERVER_MOCK_BASE_URL="http://oci-srm-server-mock/" \
    PUNCHOUT_SERVER_LOGIN_URI="http://punchout-server/punch-in?foo=bar&pass=example-supersecret" \
    PUNCHOUT_SERVER_CONFIRMATION_URI="http://punchout-server/cxml-order-request-endpoint"

COPY --link --from=builder /oci-srm-server-mock /oci-srm-server-mock

EXPOSE 80

CMD ["/oci-srm-server-mock"]

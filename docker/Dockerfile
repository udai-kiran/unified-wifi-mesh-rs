# S0 harness image (docs/0011 D4). Build context: the repository root.
#   docker build -f docker/Dockerfile .

# Stage A: ieee1905-rs at a pinned commit, fetched by hash. Needs Rust >= 1.91 (str::floor_char_boundary, 0011 erratum E1).
FROM rust:1.91-slim-bookworm AS ieee1905-build
ARG IEEE1905_COMMIT=a6443225db82f7e8aed1c29644d2ed0b947b4d6e
RUN apt-get update \
    && apt-get install -y --no-install-recommends git ca-certificates
WORKDIR /src
RUN git init -q . \
    && git fetch -q --depth 1 https://github.com/rdkcentral/ieee1905-rs "${IEEE1905_COMMIT}" \
    && git checkout -q FETCH_HEAD \
    && test "$(git rev-parse HEAD)" = "${IEEE1905_COMMIT}"
# Default features only: no rbus, so no clang.
RUN cargo build --release --locked -p ieee1905

# Stage B: the workspace probe, on the workspace MSRV.
FROM rust:1.85-slim-bookworm AS probe-build
WORKDIR /work
COPY . .
RUN cargo build --release --locked -p em-transport --example alsap_probe

# Runtime: both binaries plus the supervising entrypoint.
FROM debian:bookworm-slim AS runtime
COPY --from=ieee1905-build /src/target/release/ieee1905 /usr/local/bin/ieee1905
COPY --from=probe-build /work/target/release/examples/alsap_probe /usr/local/bin/alsap_probe
COPY docker/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod 0755 /usr/local/bin/entrypoint.sh
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]

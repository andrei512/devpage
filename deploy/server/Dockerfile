# syntax=docker/dockerfile:1
# An example image for `devpage server`: documents published one at a time,
# every version kept in a bucket, the store in SQLite backed up by Litestream.
# Copy it and change it.
#
#   docker build deploy/server --build-arg DEVPAGE_VERSION=<version>  # from crates.io
#   docker build deploy/server --build-context source=. \
#     --build-arg DEVPAGE_FROM=checkout                 # devpage from this checkout
ARG DEVPAGE_FROM=release

# devpage as anyone installs it: the released crate.
FROM rust:1.98.1-slim-bookworm AS release
ARG DEVPAGE_VERSION
RUN test -n "$DEVPAGE_VERSION" \
      || { echo "set --build-arg DEVPAGE_VERSION to a release of devpage"; exit 1; } \
 && cargo install devpage --version "$DEVPAGE_VERSION" --locked

# devpage built from the repository the build runs in, to try a change before
# it is released, with the compiler rust-toolchain.toml pins: what `cargo
# install` reads from the repository, given as the build context `source`.
FROM rust:1.98.1-slim-bookworm AS checkout
WORKDIR /src
COPY --from=source Cargo.toml Cargo.lock ./
COPY --from=source src src
COPY --from=source core core
COPY --from=source design design
COPY --from=source assets assets
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/target \
    CARGO_TARGET_DIR=/target cargo install --path /src --locked

FROM ${DEVPAGE_FROM} AS devpage

FROM litestream/litestream:0.5.2 AS litestream

FROM debian:bookworm-slim
# ca-certificates: the bucket is reached over HTTPS. The libraries are
# chrome-headless-shell's, which devpage downloads to print a PDF download.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
      libnss3 libatk1.0-0 libatk-bridge2.0-0 libcups2 libdrm2 libxkbcommon0 \
      libxcomposite1 libxdamage1 libxfixes3 libxrandr2 libgbm1 libpango-1.0-0 \
      libcairo2 libasound2 fonts-liberation \
 && rm -rf /var/lib/apt/lists/*
COPY --from=devpage /usr/local/cargo/bin/devpage /usr/local/bin/devpage
COPY --from=litestream /usr/local/bin/litestream /usr/local/bin/litestream
COPY litestream.yml /etc/litestream.yml
COPY devpage-server.sh /usr/local/bin/devpage-server
# chrome-headless-shell, fetched now rather than on the first download — it
# has builds for x86-64 Linux only, so an arm64 image prints no PDF.
RUN if [ "$(uname -m)" = x86_64 ]; then \
      printf '# Ready\n' > /tmp/ready.md \
      && devpage pdf /tmp/ready.md -o /tmp/ready.pdf && rm /tmp/ready.*; \
    fi
ENV DEVPAGE_HOST=0.0.0.0 \
    DEVPAGE_DATA_DIR=/data
VOLUME /data
EXPOSE 3030
CMD ["devpage-server"]

# An example image serving devpage's own docs, deployed: installed from
# crates.io like anyone would, behind an auth proxy that names each reader.
# It reads nothing from the build context: docker build deploy/bundled
FROM rust:1.98.1-slim-bookworm AS build
ARG DEVPAGE_VERSION=0.2.3
# The docs and licence come from the published crate's own source, so they
# always match the binary.
RUN cargo install devpage --version "$DEVPAGE_VERSION" --locked \
 && cp -r "$CARGO_HOME"/registry/src/*/devpage-"$DEVPAGE_VERSION" /devpage-src

FROM debian:bookworm-slim
# ca-certificates: the link checker fetches https links.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*
COPY --from=build /usr/local/cargo/bin/devpage /usr/local/bin/devpage
COPY --from=build /devpage-src/docs /srv/docs
COPY --from=build /devpage-src/LICENSE /srv/LICENSE
WORKDIR /srv
EXPOSE 3030
CMD ["devpage", "docs", "--host", "0.0.0.0", "--port", "3030"]

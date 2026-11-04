# An example image serving devpage's own docs, deployed: installed from
# crates.io like anyone would, behind an auth proxy that names each reader.
# It reads nothing from the build context: docker build deploy/bundled
FROM rust:1.98.1-slim-bookworm AS build
ARG DEVPAGE_VERSION=0.3.1
# The docs, licence, changelog and the code their reference reads come from
# the published crates' own source, so they always match the binary.
RUN cargo install devpage --version "$DEVPAGE_VERSION" --locked \
 && cd "$CARGO_HOME"/registry/src/* \
 && cp -r devpage-"$DEVPAGE_VERSION" /devpage-src \
 && cp -r devpage-core-"$DEVPAGE_VERSION" /devpage-src/core \
 && cp -r devpage-design-"$DEVPAGE_VERSION" /devpage-src/design \
 && cp -r devpage-extension-api-"$DEVPAGE_VERSION" /devpage-src/extension-api

FROM debian:bookworm-slim
# ca-certificates: the link checker fetches https links.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*
COPY --from=build /usr/local/cargo/bin/devpage /usr/local/bin/devpage
COPY --from=build /devpage-src /srv
WORKDIR /srv
EXPOSE 3030
CMD ["devpage", "docs", "--host", "0.0.0.0", "--port", "3030"]

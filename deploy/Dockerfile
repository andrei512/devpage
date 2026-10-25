# An example image serving devpage's own docs, deployed: installed from
# crates.io like anyone would, behind an auth proxy that names each reader.
# Build from the repository root: docker build -f deploy/Dockerfile .
FROM rust:1.89-slim-bookworm AS build
RUN cargo install devpage --version 0.1.0 --locked

FROM debian:bookworm-slim
COPY --from=build /usr/local/cargo/bin/devpage /usr/local/bin/devpage
COPY docs /srv/docs
COPY LICENSE /srv/LICENSE
WORKDIR /srv
EXPOSE 3030
CMD ["devpage", "docs", "--host", "0.0.0.0", "--port", "3030"]

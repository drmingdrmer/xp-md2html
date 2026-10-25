# docker run --rm --user "$(id -u):$(id -g)" -v "$PWD:/work" IMAGE process -i post.md -o out.md
FROM rust:1.90.0-slim-trixie AS build

WORKDIR /src
COPY rust-toolchain.toml ./
RUN rustup toolchain install nightly-2026-08-28 --profile minimal
COPY Cargo.toml Cargo.lock github-markdown.css ./
COPY src ./src
COPY vendor ./vendor
RUN cargo +nightly-2026-08-28 build --locked --release --bin xpmd

FROM debian:trixie-slim

ARG VERSION=dev
ARG REVISION=unknown
LABEL org.opencontainers.image.source=https://github.com/drmingdrmer/xp-md2html \
      org.opencontainers.image.version=$VERSION \
      org.opencontainers.image.revision=$REVISION

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates chromium imagemagick fontconfig \
        fonts-noto-core fonts-noto-cjk fonts-noto-mono fonts-noto-color-emoji \
    && rm -rf /var/lib/apt/lists/* \
    && install -d -m 0777 /work

ENV XPMD_CHROME=/usr/bin/chromium \
    XPMD_MAGICK=/usr/bin/magick \
    HOME=/tmp

COPY --from=build /src/target/release/xpmd /usr/local/bin/xpmd
COPY --chmod=755 tests/container/smoke.sh /usr/local/libexec/xpmd-smoke

USER 10001:10001
WORKDIR /work
# Run on the target CPU: Chrome's rendering must pass on each native architecture.
RUN HOME=/tmp/smoke-home /usr/local/libexec/xpmd-smoke /tmp/xpmd-smoke \
    && rm -rf /tmp/xpmd-smoke /tmp/smoke-home

ENTRYPOINT ["xpmd"]
CMD ["--help"]

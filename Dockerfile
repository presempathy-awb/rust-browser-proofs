FROM rust:1.95-trixie

ARG WASM_PACK_VERSION=0.15.0

RUN apt-get update \
    && apt-get upgrade --yes \
    && apt-get install --yes --no-install-recommends \
        ca-certificates \
        chromium \
        chromium-driver \
        firefox-esr \
        git \
        nodejs \
    && rm -rf /var/lib/apt/lists/*

RUN rustup target add wasm32-unknown-unknown \
    && cargo install wasm-pack --version "${WASM_PACK_VERSION}" --locked

RUN mkdir --parents /opt/rust-browser-proofs/crates /opt/rust-browser-proofs/fixtures
COPY container/generic-workspace/Cargo.toml /opt/rust-browser-proofs/Cargo.toml
COPY crates/rust-browser-proofs /opt/rust-browser-proofs/crates/rust-browser-proofs
COPY fixtures/consumer-battery /opt/rust-browser-proofs/fixtures/consumer-battery
RUN cargo install --path /opt/rust-browser-proofs/crates/rust-browser-proofs --root /usr/local
RUN rm -rf /usr/local/cargo/registry /usr/local/cargo/git

RUN useradd --create-home --uid 10001 --shell /bin/bash browser
RUN chown --recursive browser:browser /opt/rust-browser-proofs

WORKDIR /workspace
COPY --chown=browser:browser . .

USER browser
ENV HOME=/home/browser
ENV CARGO_HOME=/home/browser/.cargo
ENV PATH=/usr/local/cargo/bin:/home/browser/.cargo/bin:${PATH}
ENV CHROME_BIN=/usr/bin/chromium
ENV MOZ_HEADLESS=1
ENV RUST_BROWSER_PROOFS_WORKSPACE=/opt/rust-browser-proofs

CMD ["bash"]

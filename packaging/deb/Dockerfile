ARG DEBIAN_VERSION=latest
FROM debian:${DEBIAN_VERSION}

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
  && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    build-essential \
    pkg-config \
    libssl-dev \
    dpkg-dev \
    fakeroot \
    git \
    xz-utils \
  && rm -rf /var/lib/apt/lists/*

# Install Rust toolchain (stable) via rustup.
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal \
  && /root/.cargo/bin/rustup default stable

ENV PATH=/root/.cargo/bin:${PATH}

WORKDIR /work

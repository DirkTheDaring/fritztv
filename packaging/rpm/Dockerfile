
ARG FEDORA_VERSION=latest
FROM fedora:${FEDORA_VERSION}

RUN dnf install -y \
    rpm-build \
    rpmdevtools \
    systemd-rpm-macros \
    git \
    cargo \
    rust \
    make \
    openssl-devel \
    && dnf clean all

WORKDIR /work

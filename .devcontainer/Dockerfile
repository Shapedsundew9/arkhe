FROM mcr.microsoft.com/devcontainers/rust:latest

COPY rust-toolchain.toml /tmp/rust-toolchain.toml

RUN cd /tmp \
    && rustup toolchain install --no-self-update \
    && rustup default stable \
    && rm rust-toolchain.toml

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
    build-essential \
    libssl-dev \
    lldb \
    pkg-config \
    ripgrep \
    vim \
    xclip \
    && rm -rf /var/lib/apt/lists/*

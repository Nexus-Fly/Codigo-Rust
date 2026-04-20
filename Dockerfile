# ─────────────────────────────────────────────
# vertex_swarm_demo – Dockerfile (dev)
# Base: Rust stable on Debian Bookworm
# ─────────────────────────────────────────────
FROM rust:1-bookworm

# ── System dependencies ──────────────────────
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential \
        curl \
        git \
        pkg-config \
        ca-certificates \
        unzip \
        xz-utils \
    && rm -rf /var/lib/apt/lists/*

# ── CMake 4.3.1 (Kitware official tarball) ───
COPY scripts/install-cmake.sh /tmp/install-cmake.sh
# Strip Windows CRLF line endings before executing (safe on Linux too)
RUN sed -i 's/\r$//' /tmp/install-cmake.sh \
    && chmod +x /tmp/install-cmake.sh \
    && /tmp/install-cmake.sh

# ── Working directory ────────────────────────
WORKDIR /workspace

# ── Bootstrap Cargo metadata for layer caching ──
# Copies only manifest files first so dependency layers are cached
# separately from source changes.
COPY Cargo.toml Cargo.lock ./

# Create stub lib so `cargo fetch` succeeds without full source
RUN mkdir -p src/bin \
    && echo 'fn main() {}' > src/main.rs \
    && echo 'fn main() {}' > src/bin/keygen.rs \
    && echo 'fn main() {}' > src/bin/keyparse.rs \
    && cargo fetch \
    && rm -rf src

# ── Entrypoint scripts ────────────────────────
COPY scripts/dev-entrypoint.sh /usr/local/bin/dev-entrypoint.sh
COPY scripts/node-entrypoint.sh /usr/local/bin/node-entrypoint.sh
# Strip Windows CRLF and set executable
RUN sed -i 's/\r$//' /usr/local/bin/dev-entrypoint.sh /usr/local/bin/node-entrypoint.sh \
    && chmod +x /usr/local/bin/dev-entrypoint.sh /usr/local/bin/node-entrypoint.sh

# ── Default: interactive shell via entrypoint ─
ENTRYPOINT ["/usr/local/bin/dev-entrypoint.sh"]
CMD ["bash"]

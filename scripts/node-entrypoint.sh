#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# node-entrypoint.sh
# Entrypoint for vertex_live_node container services.
#
# Discovers the path of libtashi-vertex.so (produced by the tashi-vertex
# build script) and adds it to LD_LIBRARY_PATH before exec-ing the binary.
# This mirrors the LD_LIBRARY_PATH setup in run_live_cluster.sh so the
# pre-built binary can find its native shared library at runtime.
# ─────────────────────────────────────────────────────────────────────────────
set -euo pipefail

LIB_PATH=$(find /workspace/target/debug/build \
    -name "libtashi-vertex.so" \
    -path "*/out/lib/*" \
    2>/dev/null | head -1)

if [ -z "$LIB_PATH" ]; then
    echo "[node-entrypoint] ERROR: libtashi-vertex.so not found under /workspace/target/debug/build." >&2
    echo "[node-entrypoint] Run 'docker compose --profile cluster up node-build' first." >&2
    exit 1
fi

LIB_DIR=$(dirname "$LIB_PATH")
export LD_LIBRARY_PATH="$LIB_DIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
echo "[node-entrypoint] LD_LIBRARY_PATH=$LD_LIBRARY_PATH"

exec "$@"

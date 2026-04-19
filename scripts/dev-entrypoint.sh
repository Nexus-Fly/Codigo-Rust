#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# dev-entrypoint.sh
# Container entrypoint for local development.
# Prints tool versions and then execs the supplied command (default: bash).
# ─────────────────────────────────────────────────────────────────────────────
set -euo pipefail

# ── Ensure the workspace directory is writable ───────────────────────────────
# (Relevant when the bind-mount comes from a Windows/WSL host.)
if [ -d /workspace ]; then
    chmod -R u+rw /workspace 2>/dev/null || true
fi

# ── Tool versions ─────────────────────────────────────────────────────────────
echo "────────────────────────────────────────"
echo " vertex_swarm_demo – dev container"
echo "────────────────────────────────────────"
echo "  rustc  : $(rustc --version)"
echo "  cargo  : $(cargo --version)"
echo "  cmake  : $(cmake --version | head -1)"
echo "────────────────────────────────────────"

# ── Exec the requested command, defaulting to bash ───────────────────────────
if [ "$#" -eq 0 ]; then
    exec bash
else
    exec "$@"
fi

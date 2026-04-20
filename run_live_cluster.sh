#!/bin/bash
# Start 3 vertex_live_node instances — real distributed delivery swarm.
# Each node loads its config from config/nodeN.toml and communicates
# through Tashi Vertex BFT consensus. Ctrl+C stops all three.

set -e
cd /workspace

# Locate the native shared library.
LIB_DIR=$(find /workspace/target/debug/build -name "libtashi-vertex.so" -path "*/out/lib/*" 2>/dev/null | head -1 | xargs dirname)
if [ -z "$LIB_DIR" ]; then
  echo "ERROR: libtashi-vertex.so not found. Run 'cargo build' first."
  exit 1
fi
export LD_LIBRARY_PATH="$LIB_DIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
echo ":: LD_LIBRARY_PATH=$LD_LIBRARY_PATH"

BINARY="./target/debug/vertex_live_node"

echo "=== Building vertex_live_node ==="
cargo build --bin vertex_live_node 2>&1
echo ""
echo "=== Starting 3-node live cluster ==="
echo "  node1 (Drone) → 127.0.0.1:8001  [config/node1.toml]"
echo "  node2 (Robot) → 127.0.0.1:8002  [config/node2.toml]"
echo "  node3 (Ebike) → 127.0.0.1:8003  [config/node3.toml]"
echo ""

# node2 — background
$BINARY --config config/node2.toml 2>&1 | sed -u 's/^/[node2] /' &
PID_2=$!

# node3 — background
$BINARY --config config/node3.toml 2>&1 | sed -u 's/^/[node3] /' &
PID_3=$!

# Give node2 and node3 a moment to bind before node1 connects.
sleep 1

cleanup() {
  echo ""
  echo "=== Shutting down live cluster ==="
  kill $PID_2 $PID_3 2>/dev/null || true
  wait $PID_2 $PID_3 2>/dev/null || true
  echo "=== Done ==="
}
trap cleanup EXIT INT TERM

# node1 — foreground so Ctrl+C reaches the shell.
$BINARY --config config/node1.toml 2>&1 | sed -u 's/^/[node1] /'

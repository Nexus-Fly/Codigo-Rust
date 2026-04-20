#!/bin/bash
# Levanta 3 nodos Vertex reales en el mismo contenedor.
# Ctrl+C detiene los 3.

set -e
cd /workspace

# Locate the native shared library produced by the tashi-vertex build script.
LIB_DIR=$(find /workspace/target/debug/build -name "libtashi-vertex.so" -path "*/out/lib/*" 2>/dev/null | head -1 | xargs dirname)
if [ -z "$LIB_DIR" ]; then
  echo "ERROR: libtashi-vertex.so not found. Run 'cargo build' first."
  exit 1
fi
export LD_LIBRARY_PATH="$LIB_DIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
echo ":: LD_LIBRARY_PATH=$LD_LIBRARY_PATH"

BINARY="./target/debug/vertex_swarm_demo"

KEY_A="3d1RiRMXUVdMdRdveF4K1XRqi5KPwJumqcExcx3wqVjr7jEx2JohfJbSJHgr9Q1zHJEmsk"
KEY_B="3d1RiRMXUVCfqv8GaH5m5cob4nhe8xaHiLeTBR8r9sE47vXrQsccZVFcubBbx2UksrzvPY"
KEY_C="3d1RiRMXUVZT2AvWiNyTmEvA3qzRu3Sv4YqoXbzrm2fzQwuZrXhJzZCyKGnEhLqN5fCXTL"

PUB_A="aSq9DsNNvGhYxYyqA9wd2eduEAZ5AXWgJTbTGRjUeqfLSEigUpaT8fuqbFfRsZFgTDvV7M1ePN6ppcs2UEXwf5G8VD2Vvs8Rq2e8A2Gg1kDCixGisETVZpwTDjio"
PUB_B="aSq9DsNNvGhYxYyqA9wd2eduEAZ5AXWgJTbTHE68iyDATq2j16ksvDLMNjVLug6X42Fp8EBE7wf313kxvbBvvc7iFc6ddFCKLyV6GaHtFBgSWDKwBi7N2BPygype"
PUB_C="aSq9DsNNvGhYxYyqA9wd2eduEAZ5AXWgJTbTFwgkKE9h4Lv6FukUAcd1XAq1iLs2CP4gqgL8f2tgcaZEmLiWEq6fjwHVksnCWvE65HgPoTCQR3ee4JJxt5XdNMmZ"

echo "=== Building vertex_swarm_demo ==="
cargo build --bin vertex_swarm_demo 2>&1
echo ""
echo "=== Starting 3-node Vertex cluster ==="
echo "  Node A → 127.0.0.1:8001"
echo "  Node B → 127.0.0.1:8002"
echo "  Node C → 127.0.0.1:8003"
echo ""

# Node B — background
$BINARY \
  --bind 127.0.0.1:8002 \
  --key  "$KEY_B" \
  --peer "${PUB_A}@127.0.0.1:8001" \
  --peer "${PUB_C}@127.0.0.1:8003" \
  --message "NODE-B-ONLINE" 2>&1 | sed -u 's/^/[B] /' &
PID_B=$!

# Node C — background
$BINARY \
  --bind 127.0.0.1:8003 \
  --key  "$KEY_C" \
  --peer "${PUB_A}@127.0.0.1:8001" \
  --peer "${PUB_B}@127.0.0.1:8002" \
  --message "NODE-C-ONLINE" 2>&1 | sed -u 's/^/[C] /' &
PID_C=$!

# Give B and C a moment to bind their sockets before A connects.
sleep 1

# Cleanup on Ctrl+C / exit.
cleanup() {
  echo ""
  echo "=== Shutting down cluster ==="
  kill "$PID_B" "$PID_C" 2>/dev/null || true
  wait "$PID_B" "$PID_C" 2>/dev/null || true
  exit 0
}
trap cleanup INT TERM

# Node A — foreground (its output is the main window).
$BINARY \
  --bind 127.0.0.1:8001 \
  --key  "$KEY_A" \
  --peer "${PUB_B}@127.0.0.1:8002" \
  --peer "${PUB_C}@127.0.0.1:8003" \
  --message "NODE-A-ONLINE" 2>&1 | sed -u 's/^/[A] /'

cleanup

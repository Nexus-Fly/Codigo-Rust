# Docker Dev Environment – vertex_swarm_demo

All Rust builds **must** happen inside the container.
Do not run `cargo build` on the Windows host; linker and CMake issues will occur.

---

## Prerequisites

- [Docker Desktop](https://www.docker.com/products/docker-desktop/) with WSL 2 backend enabled
- WSL 2 distribution (Ubuntu recommended)

---

## Build the container image

```bash
docker compose build
```

This installs Rust (stable), CMake 4.3.1, and all system dependencies inside the Linux image.
The first build takes several minutes; subsequent builds use the layer cache.

---

## Start the dev environment

```bash
docker compose up -d
```

The container runs in the background with the source directory bind-mounted at `/workspace`.
Cargo registry, git cache, and the `target` directory are stored in named Docker volumes to
avoid Windows/WSL path-permission conflicts.

---

## Enter the container

```bash
docker compose exec vertex-dev bash
```

You will land in `/workspace` with `rustc`, `cargo`, and `cmake` on `PATH`.

---

## Common cargo commands (run inside the container)

### Build the entire workspace

```bash
cargo build
```

### Run the key generator

```bash
cargo run --bin keygen
```

### Parse an existing key

```bash
# Inspect a public key
cargo run --bin keyparse -- --public <PUBLIC_KEY>

# Derive public key from a secret key
cargo run --bin keyparse -- --secret <SECRET_KEY>
```

### Run the main binary

```bash
cargo run --bin vertex_swarm_demo -- --help
```

---

## Stop the environment

```bash
docker compose down
```

To also remove the named volumes (clears all build caches):

```bash
docker compose down -v
```

---

## VS Code Dev Container

Open the project in VS Code, install the
[Dev Containers](https://marketplace.visualstudio.com/items?itemName=ms-vscode-remote.remote-containers)
extension, then run **"Reopen in Container"** from the Command Palette (`Ctrl+Shift+P`).
VS Code will attach to the running `vertex-dev` service and install Rust Analyzer automatically.

---

## Multi-node distributed cluster (profile: cluster)

Three `vertex_live_node` containers run simultaneously over Tashi Vertex BFT consensus.
All containers share the host network stack (`network_mode: host`) so that they can reach
each other via `127.0.0.1:800X` exactly as the config files specify.

> **Important**: stop the `vertex-dev` container before starting the cluster, or the
> `run_live_cluster.sh` script inside it if running. Both compete for the same ports.
>
> ```bash
> docker compose down
> ```

### 1 — Build the binary (once per code change)

```bash
docker compose --profile cluster up node-build
```

This compiles `vertex_live_node` into the shared `target` volume and exits.
Wait for `Finished` before starting the nodes.

### 2 — Start the 3-node cluster

```bash
docker compose --profile cluster up node1 node2 node3
```

### 3 — Follow logs (each service prefixed automatically)

```bash
# all nodes
docker compose --profile cluster logs -f

# single node
docker compose --profile cluster logs -f node1
```

### 4 — Stop the cluster

```bash
docker compose --profile cluster down
```

### One-liner (build + run, waits for node-build to finish before nodes start)

```bash
docker compose --profile cluster up --wait node-build && docker compose --profile cluster up node1 node2 node3
```

### Expected output

Within ~15 seconds the cluster reaches quorum and begins the full order lifecycle:

```
node1  | [t=11] local intent emitted  OrderCreated  id=node1-order-1
node2  | [t=11] consensus event received  OrderCreated
node2  | [t=11] queueing follow-up intent  AuctionBid
node1  | [t=11] consensus event received  AuctionBid
node1  | [auction] order node1-order-1 → winner=node2
node2  | [node] order assigned  id=node1-order-1  winner=node2
node2  | [node] order delivered  id=node1-order-1  by=node2
```

All three nodes update their `App` state only from consensus-ordered events — never from
local intent generation.

---

## Multi-node distributed cluster (Docker Compose profiles)

The `cluster` Compose profile starts three live `vertex_live_node` processes as
separate Docker services, each loading its own TOML config.

### Why `network_mode: host`?

All node configs bind to `127.0.0.1:800X`.  Containers in their own network
namespace cannot reach each other via loopback, so every cluster service runs
with `network_mode: host` — they share the host's network stack, exactly like
the bash script `run_live_cluster.sh` does inside a single container.

> **Note:** do not start `vertex-dev` with an active cluster script at the same
> time as the `cluster` profile services — both would compete for ports
> 8001–8003 on the host network.

### First-time image rebuild (needed once after this change)

The Dockerfile now installs a second entrypoint script (`node-entrypoint.sh`).
Rebuild the image before using the cluster profile:

```bash
docker compose build
```

### Start the live 3-node cluster

```bash
docker compose --profile cluster up
```

Compose will:
1. Run `node-build` — compiles `vertex_live_node` once (exits 0 on success).
2. Start `node1`, `node2`, `node3` in parallel once the build succeeds.
3. Stream interleaved logs tagged `node1-1`, `node2-1`, `node3-1`.

### Stop the cluster

`Ctrl+C` in the same terminal, or from another shell:

```bash
docker compose --profile cluster down
```

### Run only specific nodes

```bash
docker compose --profile cluster up node-build node1 node2
```

### View logs for a single node

```bash
docker compose --profile cluster logs -f node1
```

---

## Port mapping

| Host port | Service        | Purpose              |
|-----------|----------------|----------------------|
| 8001      | vertex-dev / node1 | Vertex peer node 1 |
| 8002      | vertex-dev / node2 | Vertex peer node 2 |
| 8003      | vertex-dev / node3 | Vertex peer node 3 |
| 8004      | vertex-dev     | Vertex peer node 4   |

---

## Notes

- The `target/` directory on the Windows host is **intentionally ignored** by the container.
  The named volume `target` inside Docker is used instead, preventing cross-filesystem issues.
- Editing source files on the Windows host (or in WSL) is reflected immediately in the container
  via the bind-mount — no rebuild of the image is required.
- If you need to wipe only the build cache without removing registry downloads:
  ```bash
  docker volume rm vertex_swarm_demo_target
  ```

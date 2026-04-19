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

## Port mapping

| Host port | Container port | Purpose               |
|-----------|----------------|-----------------------|
| 8001      | 8001           | Vertex peer / node 1  |
| 8002      | 8002           | Vertex peer / node 2  |
| 8003      | 8003           | Vertex peer / node 3  |
| 8004      | 8004           | Vertex peer / node 4  |

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

# vertex_swarm_demo

A minimal MVP for a distributed delivery coordination system built on top of
[Tashi Vertex](https://tashi.dev) BFT consensus.

The project demonstrates how a swarm of autonomous delivery agents (drones,
robots, e-bikes) can coordinate order assignment, handoffs, failure recovery,
and payment settlement using a Byzantine-fault-tolerant consensus layer —
without a central server.

---

## Table of contents

1. [Architecture overview](#architecture-overview)
2. [What runs through Tashi Vertex](#what-runs-through-tashi-vertex)
3. [Module reference](#module-reference)
4. [Docker dev environment](#docker-dev-environment)
5. [Running the project](#running-the-project)
6. [MVP flow](#mvp-flow)
7. [Current status](#current-status)
8. [Known limitations](#known-limitations)
9. [Suggested next steps](#suggested-next-steps)

---

## Architecture overview

```
src/
├── main.rs                   ← Tashi Vertex node CLI (live network)
├── lib.rs                    ← Library crate (shared by binaries)
├── app.rs                    ← Application state + message dispatcher
├── codec.rs                  ← NexusMessage ↔ bytes (serde_json)
├── config.rs                 ← TOML config loader (AppConfig)
├── store.rs                  ← In-memory state store (placeholder)
├── types.rs                  ← All shared types and enums
│
├── consensus/
│   └── vertex.rs             ← Thin wrapper around Tashi Vertex Engine
│
├── domain/
│   ├── agent.rs              ← Agent identity stub
│   ├── order.rs              ← Order lifecycle state machine (FSM)
│   ├── auction.rs            ← Deterministic bid scoring and winner selection
│   ├── handoff.rs            ← Business-rule validation for order handoffs
│   ├── healing.rs            ← Heartbeat-based failure detection
│   ├── safety.rs             ← Safety zone management
│   └── ledger.rs             ← In-memory escrow and payment settlement
│
├── sim/
│   └── runner.rs             ← Local deterministic simulation + MVP flow
│
└── bin/
    ├── keygen.rs             ← Key pair generator
    ├── keyparse.rs           ← Key parser utility
    ├── mvp_demo.rs           ← Local end-to-end MVP demo (no config)
    └── mvp_config_demo.rs    ← Config-driven MVP demo (loads TOML)
```

### Separation of concerns

| Layer | Responsibility |
|---|---|
| `types.rs` | Canonical data model shared across all layers |
| `domain/` | Pure business logic; no I/O, no async, fully testable |
| `codec.rs` | Serialization boundary between domain and network |
| `consensus/vertex.rs` | Wraps the Tashi Vertex Engine; only used by `main.rs` |
| `app.rs` | Stateful dispatcher; connects domain to incoming messages |
| `sim/runner.rs` | Local, synchronous simulation; no network required |
| `bin/` | Entry points for different usage modes |

---

## What runs through Tashi Vertex

| Component | Uses Tashi Vertex? | Notes |
|---|---|---|
| `src/main.rs` | **Yes** | Live P2P node; requires network and real keys |
| `consensus/vertex.rs` | **Yes** | Engine, Socket, Transaction, Message |
| `app.rs` | No | Pure in-memory dispatcher |
| `domain/*` | No | Pure functions and state machines |
| `sim/runner.rs` | No | Deterministic local simulation |
| `bin/mvp_demo.rs` | No | Calls domain modules directly |
| `bin/mvp_config_demo.rs` | No | Loads TOML config; runs local simulation |
| `bin/keygen.rs` | Yes (key types only) | Generates `KeySecret` / `KeyPublic` |

`mvp_demo` and `mvp_config_demo` compile and run without a live Tashi Vertex
network. Only `vertex_swarm_demo` (the main binary) requires a real peer
topology and signed keys.

---

## Module reference

### `types.rs`
All shared primitives: `NodeId`, `Timestamp`, `AgentKind`, `AgentStatus`,
`OrderStatus`, `AgentState`, `Order`, `AuctionBid`, `SafetyZone`,
`LedgerEntry`, and `NexusMessage` (the consensus message envelope).

### `codec.rs`
`encode_message` / `decode_message` — converts `NexusMessage` to/from JSON
bytes for transport inside Tashi Vertex transactions. Handles null-byte
suffixes produced by `Transaction::allocate`.

### `config.rs`
`AppConfig` — full node configuration (identity, network, agent properties).  
`load_config(path)` — reads and parses a TOML file into `AppConfig`.

### `domain/order.rs`
Order lifecycle FSM. All state transitions are pure functions returning
`Result<Order>` or `Result<(Order, NexusMessage)>`:

```
Created → Bidding → Assigned → Pickup → InTransit → Delivered
                                                  ↘ HandoffPending → HandedOff → Delivered
```

### `domain/auction.rs`
Deterministic bid scoring (ETA, battery, optional reputation) and winner
selection. Ties are broken lexicographically by node ID.

### `domain/handoff.rs`
Validates and executes controlled order handoffs. Rules: order must be
`InTransit`; requester must own it; destination must be `Idle`.

### `domain/healing.rs`
`HeartbeatTracker` — detects silent agents and identifies in-flight orders
that need re-auctioning after a failure.

### `domain/safety.rs`
`SafetyMonitor` — maintains active safety zones and checks whether an agent's
GPS position falls within any restricted area (Euclidean distance check).

### `domain/ledger.rs`
`Ledger` — internal escrow state machine: `reserve_escrow` →
`transfer_for_handoff` → `release_final_payment` (or `refund_previous_agent`).
No blockchain or external payment system.

### `consensus/vertex.rs`
`VertexNode` — thin wrapper: `start()`, `send_message()`, `recv_messages()`.
Uses only the documented Tashi Vertex public API.

### `app.rs`
`App` — central in-memory state (orders, bids, peers). `from_config()` builds
it from an `AppConfig`. `handle_message()` dispatches `NexusMessage` values
to internal domain handlers.

### `sim/runner.rs`
`run_mvp_flow(configs)` — runs the full 10-step MVP scenario locally and
returns `MvpFlowResult`. `Runner` is a simpler scenario runner kept for
backward compatibility.

---

## Docker dev environment

All `cargo` commands must run **inside the container**. The native Windows
toolchain cannot link `tashi-vertex` (Linux-only native library).

### Prerequisites

- Docker Desktop with WSL 2 backend
- WSL 2 distribution (Ubuntu recommended)

### Build the image

```bash
docker compose build
```

### Start the dev container

```bash
docker compose up -d
```

### Enter the container shell

```bash
docker compose exec vertex-dev bash
```

You land in `/workspace` with `rustc`, `cargo`, and `cmake` on `PATH`.
Source files are bind-mounted from the Windows host; the `target` directory
lives in a named Docker volume to avoid permission conflicts.

---

## Running the project

All commands below assume you are **inside the container** (`docker compose exec vertex-dev bash`),
or prefixed with `docker compose exec vertex-dev` from PowerShell.

### Build everything

```bash
cargo build
```

### Run all tests

```bash
cargo test
```

Expected output: **61 passed; 0 failed** across all unit tests and 1 doctest.

### Generate a key pair

```bash
cargo run --bin keygen
```

Output:
```
Secret: <base58-secret-key>
Public: <base58-public-key>
```

### Vertex node CLI help

```bash
cargo run --bin vertex_swarm_demo -- --help
```

### Local MVP demo (no config file needed)

```bash
cargo run --bin mvp_demo
```

Runs a complete delivery scenario with 3 hardcoded agents and prints each stage.

### Config-driven MVP demo

```bash
cargo run --bin mvp_config_demo -- --config config/node1.toml
```

Loads the real `AppConfig` from TOML, initializes `App`, then runs the same
MVP flow using `node1` as the primary agent alongside two synthetic peers.

### Live Vertex node (requires real keys and peers)

```bash
# Generate keys first, then replace placeholders in config/node*.toml

cargo run --bin vertex_swarm_demo -- \
  --bind 127.0.0.1:8001 \
  --key <secret-key> \
  --peer <public-key-2>@127.0.0.1:8002 \
  --message PING
```

---

## MVP flow

The 10-step scenario exercised by `mvp_demo` and `run_mvp_flow`:

| Step | Action | Module |
|---|---|---|
| 1 | Customer creates a delivery order | `domain/order.rs` |
| 2 | All agents submit bids (ETA + battery) | `domain/auction.rs` |
| 3 | Winner selected deterministically | `domain/auction.rs` |
| 4 | Order assigned; escrow reserved | `domain/order.rs`, `domain/ledger.rs` |
| 5 | Agent picks up and goes in-transit | `domain/order.rs` |
| 6 | Agent hands off to a second agent (optional) | `domain/handoff.rs`, `domain/ledger.rs` |
| 7 | Final agent marks order delivered | `domain/order.rs` |
| 8 | Ledger releases final payment | `domain/ledger.rs` |
| 9 | One agent goes silent; re-auction candidates identified | `domain/healing.rs` |
| 10 | Safety zone declared at pickup; agent pause evaluated | `domain/safety.rs` |

Example output (3 agents, `node1` wins, handoff to `sim-peer-a`):

```
-- ORDER CREATED ----------------------------------------
  id      : mvp-order-1
  origin  : node1
-- WINNER CHOSEN ----------------------------------------
  winner : node1
-- HANDOFF COMPLETED ------------------------------------
  from     : node1
  to       : sim-peer-a
  fee paid : 100 units
-- ORDER DELIVERED ---------------------------------------
  delivered_by : sim-peer-a
  status       : Delivered
-- LEDGER UPDATED ----------------------------------------
  sim-peer-a credited 200 units (final payment)
-- REAUCTION CANDIDATES ----------------------------------
  order reauction-1 needs a new auction
-- SAFETY CHECK ------------------------------------------
  at pickup: PAUSED — agent inside safety zone
```

---

## Current status

| Feature | Status |
|---|---|
| Tashi Vertex node CLI | Working |
| Key generation / parsing | Working |
| TOML config loader | Working |
| Order state machine (FSM) | Working, 6 tests |
| Auction scoring and selection | Working, 8 tests |
| Handoff validation + FSM | Working, 5 tests |
| Heartbeat failure detection | Working, 6 tests |
| Safety zone monitoring | Working, 6 tests |
| Escrow ledger | Working, 8 tests |
| Codec (encode/decode) | Working, 7 tests |
| Local MVP simulation | Working, 12 tests |
| Config-driven demo binary | Working |
| Live multi-node consensus | Not wired (main.rs runs but App not yet integrated) |
| Persistent storage | Not implemented (Store is a placeholder) |
| Real GPS / routing | Not implemented (Euclidean distance only) |
| External payment | Not implemented (internal ledger only) |

---

## Known limitations

- **`main.rs` not yet wired to `App`**: the live Vertex node sends and receives
  raw messages but does not dispatch them through the domain handlers. This is
  intentionally deferred.
- **No persistent state**: all agent state, orders, and ledger balances are
  in-memory and lost on restart.
- **Euclidean distance only**: `SafetyMonitor` and ETA scoring use flat-plane
  geometry. Real GPS distances require the Haversine formula.
- **Single-node auction**: the MVP auction runs locally on one node. A
  distributed auction requires all nodes to agree on the winning bid via
  consensus before advancing the order FSM.
- **No authentication on messages**: any node can submit any `NexusMessage`.
  Signature verification is not implemented beyond what Tashi Vertex provides
  at the transport layer.
- **Internal ledger only**: `domain/ledger.rs` is an in-memory accounting
  system with no external payment integration.
- **config/node*.toml uses placeholder keys**: replace
  `REPLACE_WITH_REAL_SECRET_KEY_FROM_KEYGEN` with output from
  `cargo run --bin keygen` before running live nodes.

---

## Suggested next steps

1. **Wire `App` into `main.rs`**: call `App::from_config()` and
   `app.handle_message()` on each decoded Vertex event so the live node
   actually drives the domain FSM.

2. **Distributed auction**: broadcast `AuctionBid` messages through Vertex
   and let each node collect bids for a fixed window before calling
   `auction::choose_winner_id`. Use a `SyncPoint` to signal the end of the
   bidding round.

3. **Persistent storage**: replace `store::Store` with a real backend
   (SQLite via `rusqlite`, or a simple append-only log) so state survives
   node restarts.

4. **Real GPS distances**: replace the Euclidean distance in `SimAgent::eta_to`
   and `SafetyMonitor::is_paused_by_safety` with Haversine calculations.

5. **Message authentication**: verify that the sender of each `NexusMessage`
   matches the `node_id` claimed inside the message, using the public key
   already exchanged during Vertex peer setup.

6. **Re-auction flow**: connect `healing::HeartbeatTracker` to the live
   message loop so `orders_to_reauction` triggers real new auction rounds
   after a peer failure is confirmed.

7. **Replace placeholder keys**: run `cargo run --bin keygen` for each node,
   paste the output into `config/node*.toml`, and test with 4 live Docker
   containers using `docker compose up`.
